//! Program preparation: SPIR-V modules, merged reflection, the role of every descriptor,
//! descriptor-set and pipeline layouts, and the graphics/compute pipelines (one per
//! program and render state, built on demand with dynamic rendering).

use crate::device::Gpu;
use crate::error::{RuntimeError, VkResultExt};
use crate::resources::Arena;
use crate::scene::formats::VertexLayout;
use crate::uniforms::HOST_BLOCKS;
use ash::vk;
use sb_compile::{Descriptor, DescriptorKind, InterfaceVar, Reflection};
use sb_core::model::{BindingTable, BlobTable, DimensionPipeline, Program, ResourceKind, ResourceRef};
use sb_core::program::{BlendFactor, BlendMode};
use sb_core::{ScalarKind, ShaderStage};
use std::collections::{BTreeMap, HashMap};
use std::ffi::CString;

/// What the host binds to a descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Role {
    /// `sb_Frame`.
    Frame,
    /// `sb_Draw`.
    Draw,
    /// A draw-profile host block (`Globals`, `TerrainUniform`, ...).
    HostBlock(String),
    /// A uniform block the pack declares itself (zero-filled).
    PackUbo,
    /// A resource of the binding table (or a host sampler).
    Resource(ResourceRef),
}

/// One descriptor binding of a program (merged over its stages).
#[derive(Debug, Clone)]
pub(crate) struct Slot {
    pub set: u32,
    pub binding: u32,
    pub name: String,
    pub kind: DescriptorKind,
    pub count: u32,
    pub stages: vk::ShaderStageFlags,
    pub role: Role,
    /// Uniform-buffer range in bytes (UBOs only).
    pub range: u32,
    /// Read the alt buffer (`BindingUse::use_alt`), when the model says so.
    pub use_alt: Option<bool>,
}

impl Slot {
    pub fn vk_type(&self) -> vk::DescriptorType {
        match self.kind {
            DescriptorKind::UniformBuffer { .. } => vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
            DescriptorKind::CombinedImageSampler { .. } => vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            DescriptorKind::SampledImage { .. } => vk::DescriptorType::SAMPLED_IMAGE,
            DescriptorKind::Sampler => vk::DescriptorType::SAMPLER,
            DescriptorKind::StorageImage { .. } => vk::DescriptorType::STORAGE_IMAGE,
            DescriptorKind::StorageBuffer { .. } => vk::DescriptorType::STORAGE_BUFFER,
            // Rejected in `prepare`.
            _ => vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
        }
    }

    pub fn is_dynamic_ubo(&self) -> bool {
        matches!(self.kind, DescriptorKind::UniformBuffer { .. })
    }
}

/// Per-attachment colour state of a pipeline variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AttachmentState {
    pub write: bool,
    pub blend: Option<BlendMode>,
}

/// Render state that selects a pipeline variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct VariantKey {
    /// `UNDEFINED` = no attachment in that slot.
    pub color_formats: Vec<vk::Format>,
    pub attachments: Vec<AttachmentState>,
    /// `UNDEFINED` = no depth attachment.
    pub depth_format: vk::Format,
    pub depth_test: bool,
    pub depth_write: bool,
    pub depth_compare: vk::CompareOp,
    pub cull: vk::CullModeFlags,
    /// Vertex layout (profile name), `None` for fullscreen draws.
    pub layout: Option<&'static str>,
    pub neg_one_to_one: bool,
}

/// A prepared program.
pub(crate) struct PreparedProgram {
    pub name: String,
    pub compute: bool,
    stages: Vec<(ShaderStage, vk::ShaderModule)>,
    pub slots: Vec<Slot>,
    /// One layout per set `0..=max set`.
    pub set_layouts: Vec<vk::DescriptorSetLayout>,
    pub layout: vk::PipelineLayout,
    pub push_constants: Option<(vk::ShaderStageFlags, u32)>,
    /// Members of the push-constant block (union over the stages).
    pub push_members: Vec<sb_compile::BufferMember>,
    pub vertex_inputs: Vec<InterfaceVar>,
    pub fragment_outputs: Vec<InterfaceVar>,
    pub patch_control_points: Option<u32>,
    /// Primitive topology (patches with tessellation; a geometry shader's input
    /// primitive otherwise).
    pub topology: vk::PrimitiveTopology,
    pub local_size: [u32; 3],
    variants: HashMap<VariantKey, Result<vk::Pipeline, String>>,
    compute_pipeline: Option<vk::Pipeline>,
    /// Pipelines created so far (all variants).
    pub pipelines_created: u32,
    /// Inputs that the vertex layout could not provide (reported once).
    pub input_warnings: Vec<String>,
}

fn stage_flag(s: ShaderStage) -> vk::ShaderStageFlags {
    match s {
        ShaderStage::Vertex => vk::ShaderStageFlags::VERTEX,
        ShaderStage::TessControl => vk::ShaderStageFlags::TESSELLATION_CONTROL,
        ShaderStage::TessEval => vk::ShaderStageFlags::TESSELLATION_EVALUATION,
        ShaderStage::Geometry => vk::ShaderStageFlags::GEOMETRY,
        ShaderStage::Fragment => vk::ShaderStageFlags::FRAGMENT,
        ShaderStage::Compute => vk::ShaderStageFlags::COMPUTE,
    }
}

/// Host samplers of the draw profiles and what they provide.
pub(crate) fn host_sampler(name: &str) -> Option<ResourceRef> {
    Some(match name {
        "Sampler0" => ResourceRef::Atlas,
        "Sampler1" => ResourceRef::Overlay,
        "Sampler2" | "uLightMap" => ResourceRef::Lightmap,
        "uBlockAtlas" => ResourceRef::DhBlockAtlas,
        _ => return None,
    })
}

fn kind_matches(entry: &ResourceKind, d: &DescriptorKind) -> bool {
    matches!(
        (entry, d),
        (ResourceKind::Sampler { .. }, DescriptorKind::CombinedImageSampler { .. } | DescriptorKind::SampledImage { .. })
            | (ResourceKind::StorageImage { .. }, DescriptorKind::StorageImage { .. })
            | (ResourceKind::StorageBuffer, DescriptorKind::StorageBuffer { .. })
            | (ResourceKind::UniformBuffer, DescriptorKind::UniformBuffer { .. })
    )
}

/// Decide what a reflected descriptor binds.
pub(crate) fn resolve_role(d: &Descriptor, dim: &DimensionPipeline) -> Role {
    let table: &BindingTable = &dim.bindings;
    if let DescriptorKind::UniformBuffer { .. } = d.kind {
        let block = d.block_type_name.as_deref().unwrap_or(&d.name);
        let frame = &dim.uniforms.frame;
        let draw = &dim.uniforms.draw;
        let is = |layout: &sb_core::model::BlockLayout, default: &str| {
            let name = if layout.name.is_empty() { default } else { layout.name.as_str() };
            block == name || d.name == name
        };
        if is(frame, sb_uniforms::FRAME_BLOCK_NAME) {
            return Role::Frame;
        }
        if is(draw, sb_uniforms::DRAW_BLOCK_NAME) {
            return Role::Draw;
        }
        if HOST_BLOCKS.contains(&block) {
            return Role::HostBlock(block.to_string());
        }
        return Role::PackUbo;
    }
    let by_name = table.get(&d.name).filter(|e| kind_matches(&e.kind, &d.kind));
    let by_block = d.block_type_name.as_deref().and_then(|b| table.get(b)).filter(|e| kind_matches(&e.kind, &d.kind));
    let by_binding = table.entries.iter().find(|e| e.set == d.set && e.binding == d.binding && kind_matches(&e.kind, &d.kind));
    if let Some(e) = by_name.or(by_block) {
        return Role::Resource(e.resource.clone());
    }
    if let Some(r) = host_sampler(&d.name) {
        return Role::Resource(r);
    }
    match by_binding {
        Some(e) => Role::Resource(e.resource.clone()),
        None => Role::Resource(ResourceRef::Unknown(d.name.clone())),
    }
}

/// Merge the descriptors of all stages.
fn merge_descriptors(refls: &[(ShaderStage, Reflection)]) -> Result<Vec<(Descriptor, vk::ShaderStageFlags)>, String> {
    let mut map: BTreeMap<(u32, u32), (Descriptor, vk::ShaderStageFlags)> = BTreeMap::new();
    for (stage, r) in refls {
        for d in &r.descriptors {
            match map.get_mut(&(d.set, d.binding)) {
                Some((existing, flags)) => {
                    if existing.kind.vk_descriptor_type() != d.kind.vk_descriptor_type() || existing.count != d.count {
                        return Err(format!("set {} binding {} is declared with different types in different stages", d.set, d.binding));
                    }
                    // Keep the larger uniform block (stages may declare different members).
                    if let (DescriptorKind::UniformBuffer { size: a, .. }, DescriptorKind::UniformBuffer { size: b, .. }) = (&existing.kind, &d.kind)
                        && b > a
                    {
                        *existing = d.clone();
                    }
                    *flags |= stage_flag(*stage);
                }
                None => {
                    map.insert((d.set, d.binding), (d.clone(), stage_flag(*stage)));
                }
            }
        }
    }
    Ok(map.into_values().collect())
}

/// Prepare a program: modules, reflection, layouts. Errors are skip reasons.
pub(crate) fn prepare(gpu: &Gpu, arena: &mut Arena, dim: &DimensionPipeline, blobs: &BlobTable, program: &Program) -> Result<PreparedProgram, String> {
    let mut refls = Vec::new();
    let mut spirvs = Vec::new();
    if program.stages.is_empty() {
        return Err("program has no stages".into());
    }
    for sm in &program.stages {
        let blob = sm.spirv.ok_or_else(|| format!("{} stage has no SPIR-V", sm.stage))?;
        let words = blobs.get_spirv(blob).ok_or_else(|| format!("{} stage: SPIR-V blob {} is missing or malformed", sm.stage, blob.0))?;
        let refl = sb_compile::reflect(&words).map_err(|e| format!("{} stage: reflection failed: {e}", sm.stage))?;
        if refl.stage != sm.stage {
            return Err(format!("{} stage holds a {} module", sm.stage, refl.stage));
        }
        refls.push((sm.stage, refl));
        spirvs.push((sm.stage, words, sm.entry_point.clone()));
    }
    for (k, (stage, _)) in refls.iter().enumerate() {
        if refls[..k].iter().any(|(s, _)| s == stage) {
            return Err(format!("the program has two {stage} stages"));
        }
    }
    let compute = refls.iter().any(|(s, _)| *s == ShaderStage::Compute);
    if compute && refls.len() != 1 {
        return Err("a compute program must have exactly one stage".into());
    }
    let has = |s: ShaderStage| refls.iter().any(|(x, _)| *x == s);
    if !compute && !(has(ShaderStage::Vertex) && has(ShaderStage::Fragment)) {
        return Err("a graphics program needs a vertex and a fragment stage".into());
    }
    if (has(ShaderStage::Geometry) && gpu.features.geometry_shader != vk::TRUE)
        || ((has(ShaderStage::TessControl) || has(ShaderStage::TessEval)) && gpu.features.tessellation_shader != vk::TRUE)
    {
        return Err("the device lacks geometry/tessellation shader support".into());
    }
    if has(ShaderStage::TessControl) != has(ShaderStage::TessEval) {
        return Err("tessellation needs both a control and an evaluation stage".into());
    }
    if !compute {
        check_interfaces(&refls)?;
    }

    let merged = merge_descriptors(&refls)?;
    let limits = gpu.limits();
    let mut slots = Vec::new();
    for (d, stages) in merged {
        match d.kind {
            DescriptorKind::UniformBuffer { .. }
            | DescriptorKind::CombinedImageSampler { .. }
            | DescriptorKind::SampledImage { .. }
            | DescriptorKind::Sampler
            | DescriptorKind::StorageImage { .. }
            | DescriptorKind::StorageBuffer { .. } => {}
            ref other => return Err(format!("descriptor `{}` has an unsupported type ({other:?})", d.name)),
        }
        if d.count == 0 {
            return Err(format!("descriptor `{}` is a runtime-sized array", d.name));
        }
        if d.count > 64 {
            return Err(format!("descriptor `{}` is an array of {} elements", d.name, d.count));
        }
        if d.set >= limits.max_bound_descriptor_sets.min(8) {
            return Err(format!("descriptor `{}` uses set {}", d.name, d.set));
        }
        if let DescriptorKind::CombinedImageSampler { multisampled: true, .. } | DescriptorKind::SampledImage { multisampled: true, .. } = d.kind {
            return Err(format!("multisampled sampler `{}` is not supported", d.name));
        }
        let range = match &d.kind {
            DescriptorKind::UniformBuffer { size, .. } => size.div_ceil(16).max(1) * 16,
            _ => 0,
        };
        if range > limits.max_uniform_buffer_range {
            return Err(format!("uniform block `{}` is {range} bytes (device limit {})", d.name, limits.max_uniform_buffer_range));
        }
        let role = resolve_role(&d, dim);
        let use_alt = program.bindings_used.iter().find(|b| b.name == d.name || (b.set == d.set && b.binding == d.binding)).map(|b| b.use_alt);
        slots.push(Slot { set: d.set, binding: d.binding, name: d.name.clone(), kind: d.kind.clone(), count: d.count, stages, role, range, use_alt });
    }
    let dynamic = slots.iter().filter(|s| s.is_dynamic_ubo()).map(|s| s.count).sum::<u32>();
    if dynamic > limits.max_descriptor_set_uniform_buffers_dynamic {
        return Err(format!("{dynamic} uniform blocks exceed the device limit of {}", limits.max_descriptor_set_uniform_buffers_dynamic));
    }

    let push = refls.iter().filter(|(_, r)| r.push_constant_size > 0).fold(None, |acc: Option<(vk::ShaderStageFlags, u32)>, (s, r)| {
        let (f, size) = acc.unwrap_or((vk::ShaderStageFlags::empty(), 0));
        Some((f | stage_flag(*s), size.max(r.push_constant_size)))
    });
    if let Some((_, size)) = push
        && size > limits.max_push_constants_size
    {
        return Err(format!("push constants of {size} bytes exceed the device limit {}", limits.max_push_constants_size));
    }

    let mut push_members: Vec<sb_compile::BufferMember> = Vec::new();
    for m in refls.iter().filter_map(|(_, r)| r.push_constants.as_ref()).flat_map(|b| &b.members) {
        if !push_members.iter().any(|x| x.name == m.name && x.offset == m.offset) {
            push_members.push(m.clone());
        }
    }
    let vertex_inputs = refls.iter().find(|(s, _)| *s == ShaderStage::Vertex).map(|(_, r)| r.inputs.clone()).unwrap_or_default();
    if vertex_inputs.iter().any(|v| v.base_type != "float" && v.base_type != "int" && v.base_type != "uint") {
        return Err("vertex inputs of 64-bit, 16-bit or struct types are not supported".into());
    }
    if let Some(v) = vertex_inputs.iter().find(|v| v.location + v.location_count.max(1) > limits.max_vertex_input_attributes) {
        return Err(format!("vertex input `{}` uses location {} (device limit {})", v.name, v.location, limits.max_vertex_input_attributes));
    }
    let fragment_outputs = refls.iter().find(|(s, _)| *s == ShaderStage::Fragment).map(|(_, r)| r.outputs.clone()).unwrap_or_default();
    if let Some(o) = fragment_outputs.iter().find(|o| o.location + o.location_count.max(1) > limits.max_fragment_output_attachments) {
        return Err(format!("fragment output `{}` uses location {} (device limit {})", o.name, o.location, limits.max_fragment_output_attachments));
    }
    // Our meshes are triangle lists: patches of 3 control points.
    let patch_control_points = has(ShaderStage::TessControl).then_some(3);
    let topology = if patch_control_points.is_some() {
        vk::PrimitiveTopology::PATCH_LIST
    } else {
        match refls.iter().find(|(s, _)| *s == ShaderStage::Geometry) {
            Some((_, r)) if r.has_execution_mode("InputPoints") => vk::PrimitiveTopology::POINT_LIST,
            Some((_, r)) if r.has_execution_mode("InputLines") => vk::PrimitiveTopology::LINE_LIST,
            Some((_, r)) if r.has_execution_mode("InputLinesAdjacency") => vk::PrimitiveTopology::LINE_LIST_WITH_ADJACENCY,
            Some((_, r)) if r.has_execution_mode("InputTrianglesAdjacency") => vk::PrimitiveTopology::TRIANGLE_LIST_WITH_ADJACENCY,
            _ => vk::PrimitiveTopology::TRIANGLE_LIST,
        }
    };
    let local_size = refls.iter().find_map(|(_, r)| r.local_size).unwrap_or([1, 1, 1]);

    // Modules.
    let mut stages = Vec::new();
    for (stage, words, _) in &spirvs {
        let info = vk::ShaderModuleCreateInfo::default().code(words);
        let module = unsafe { gpu.device.create_shader_module(&info, None) }.map_err(|e| format!("{stage} stage: vkCreateShaderModule failed: {e:?}"))?;
        arena.modules.push(module);
        gpu.set_name(module, &format!("{} ({stage})", program.name));
        stages.push((*stage, module));
    }
    let entry_ok = spirvs.iter().all(|(_, _, e)| e == "main" || e.is_empty());
    if !entry_ok {
        return Err("only `main` entry points are supported".into());
    }

    // Set layouts (gaps get empty layouts).
    let max_set = slots.iter().map(|s| s.set).max();
    let mut set_layouts = Vec::new();
    if let Some(max_set) = max_set {
        for set in 0..=max_set {
            let bindings: Vec<vk::DescriptorSetLayoutBinding<'_>> = slots
                .iter()
                .filter(|s| s.set == set)
                .map(|s| vk::DescriptorSetLayoutBinding::default().binding(s.binding).descriptor_type(s.vk_type()).descriptor_count(s.count).stage_flags(s.stages))
                .collect();
            let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
            let l = unsafe { gpu.device.create_descriptor_set_layout(&info, None) }.map_err(|e| format!("vkCreateDescriptorSetLayout failed: {e:?}"))?;
            arena.set_layouts.push(l);
            set_layouts.push(l);
        }
    }
    let ranges: Vec<vk::PushConstantRange> = push.iter().map(|(f, size)| vk::PushConstantRange { stage_flags: *f, offset: 0, size: *size }).collect();
    let info = vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts).push_constant_ranges(&ranges);
    let layout = unsafe { gpu.device.create_pipeline_layout(&info, None) }.map_err(|e| format!("vkCreatePipelineLayout failed: {e:?}"))?;
    arena.pipeline_layouts.push(layout);
    Ok(PreparedProgram {
        name: program.name.clone(),
        compute,
        stages,
        slots,
        set_layouts,
        layout,
        push_constants: push,
        push_members,
        vertex_inputs,
        fragment_outputs,
        patch_control_points,
        topology,
        local_size,
        variants: HashMap::new(),
        compute_pipeline: None,
        pipelines_created: 0,
        input_warnings: Vec::new(),
    })
}

/// Every user input of a graphics stage must be written by the previous stage at the
/// same location and component, with the same component type and at least as many
/// components (Vulkan interface matching; violations are validation errors and
/// undefined values). The translator links stages, so a mismatch is a model
/// inconsistency (e.g. stages of different programs).
fn check_interfaces(refls: &[(ShaderStage, Reflection)]) -> Result<(), String> {
    let order = [ShaderStage::Vertex, ShaderStage::TessControl, ShaderStage::TessEval, ShaderStage::Geometry, ShaderStage::Fragment];
    let stages: Vec<&(ShaderStage, Reflection)> = order.iter().filter_map(|o| refls.iter().find(|(s, _)| s == o)).collect();
    for pair in stages.windows(2) {
        let ((prev, producer), (next, consumer)) = (pair[0], pair[1]);
        for input in &consumer.inputs {
            let Some(output) = producer.outputs.iter().find(|o| o.location == input.location && o.component == input.component && o.patch == input.patch) else {
                return Err(format!("{next} input `{}` (location {}) is not written by the {prev} stage", input.name, input.location));
            };
            let compatible = output.base_type == input.base_type
                && output.columns == input.columns
                && output.vec_size >= input.vec_size
                && output.array_len == input.array_len;
            if !compatible {
                return Err(format!(
                    "{next} input `{}` (location {}, {}) does not match the {prev} output `{}` ({})",
                    input.name,
                    input.location,
                    input.glsl_type_name(),
                    output.name,
                    output.glsl_type_name()
                ));
            }
        }
    }
    Ok(())
}

fn blend_factor(f: BlendFactor) -> vk::BlendFactor {
    match f {
        BlendFactor::Zero => vk::BlendFactor::ZERO,
        BlendFactor::One => vk::BlendFactor::ONE,
        BlendFactor::SrcColor => vk::BlendFactor::SRC_COLOR,
        BlendFactor::OneMinusSrcColor => vk::BlendFactor::ONE_MINUS_SRC_COLOR,
        BlendFactor::DstColor => vk::BlendFactor::DST_COLOR,
        BlendFactor::OneMinusDstColor => vk::BlendFactor::ONE_MINUS_DST_COLOR,
        BlendFactor::SrcAlpha => vk::BlendFactor::SRC_ALPHA,
        BlendFactor::OneMinusSrcAlpha => vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
        BlendFactor::DstAlpha => vk::BlendFactor::DST_ALPHA,
        BlendFactor::OneMinusDstAlpha => vk::BlendFactor::ONE_MINUS_DST_ALPHA,
        BlendFactor::SrcAlphaSaturate => vk::BlendFactor::SRC_ALPHA_SATURATE,
    }
}

/// GL enum of a blend factor (`blendFunc` uniform).
pub(crate) fn gl_blend_factor(f: BlendFactor) -> i32 {
    match f {
        BlendFactor::Zero => 0,
        BlendFactor::One => 1,
        BlendFactor::SrcColor => 0x0300,
        BlendFactor::OneMinusSrcColor => 0x0301,
        BlendFactor::SrcAlpha => 0x0302,
        BlendFactor::OneMinusSrcAlpha => 0x0303,
        BlendFactor::DstAlpha => 0x0304,
        BlendFactor::OneMinusDstAlpha => 0x0305,
        BlendFactor::DstColor => 0x0306,
        BlendFactor::OneMinusDstColor => 0x0307,
        BlendFactor::SrcAlphaSaturate => 0x0308,
    }
}

/// Numeric class of a vertex attribute format.
pub(crate) fn attribute_class(f: vk::Format) -> &'static str {
    match f {
        vk::Format::R8_UINT
        | vk::Format::R16_UINT
        | vk::Format::R32_UINT
        | vk::Format::R16G16_UINT
        | vk::Format::R32G32_UINT
        | vk::Format::R16G16B16_UINT
        | vk::Format::R8G8B8A8_UINT
        | vk::Format::R16G16B16A16_UINT
        | vk::Format::R32G32B32A32_UINT => "uint",
        vk::Format::R16G16_SINT | vk::Format::R8G8B8A8_SINT | vk::Format::R32G32B32_SINT | vk::Format::R32G32B32A32_SINT => "int",
        _ => "float",
    }
}

/// Binding used for vertex inputs the layout does not provide: stride 0 over
/// [`ATTRIBUTE_DEFAULTS`], so they read GL's default `(0, 0, 0, 1)`.
pub(crate) const NULL_VERTEX_BINDING: u32 = 7;

/// Contents of the buffer behind [`NULL_VERTEX_BINDING`]: `vec4(0, 0, 0, 1)` as floats
/// at offset 0 and as 32-bit integers at offset 16.
pub(crate) fn attribute_defaults() -> [u8; 32] {
    let mut b = [0u8; 32];
    b[12..16].copy_from_slice(&1.0f32.to_le_bytes());
    b[28..32].copy_from_slice(&1i32.to_le_bytes());
    b
}

impl PreparedProgram {
    /// Build the vertex input description for `layout`. Inputs the layout lacks (or
    /// provides with a different numeric type) read zeros from [`NULL_VERTEX_BINDING`].
    fn vertex_input(&mut self, gpu: &Gpu, layout: &VertexLayout) -> (Vec<vk::VertexInputBindingDescription>, Vec<vk::VertexInputAttributeDescription>) {
        let mut bindings: Vec<vk::VertexInputBindingDescription> = layout
            .bindings
            .iter()
            .map(|b| vk::VertexInputBindingDescription {
                binding: b.binding,
                stride: b.stride,
                input_rate: if b.per_instance { vk::VertexInputRate::INSTANCE } else { vk::VertexInputRate::VERTEX },
            })
            .collect();
        let mut attrs = Vec::new();
        let mut need_null = false;
        for input in &self.vertex_inputs {
            let found = layout.element(&input.name).filter(|e| attribute_class(e.format) == input.base_type);
            match found {
                Some(e) if input.columns <= 1 && input.array_len.is_none() => {
                    let mut format = e.format;
                    if format == vk::Format::R16G16B16_UINT && !gpu.buffer_format_features(format).contains(vk::FormatFeatureFlags::VERTEX_BUFFER) {
                        // Read 8 bytes instead (the 4th component is ignored by a uvec3).
                        format = vk::Format::R16G16B16A16_UINT;
                    }
                    attrs.push(vk::VertexInputAttributeDescription { location: input.location, binding: e.binding, format, offset: e.offset });
                }
                _ => {
                    let msg = format!("vertex input `{}` ({}) is not provided by the `{}` layout; it reads zero", input.name, input.glsl_type_name(), layout.profile);
                    if !self.input_warnings.contains(&msg) {
                        self.input_warnings.push(msg);
                    }
                    need_null = true;
                    // GL's current-attribute default (0, 0, 0, 1): floats at offset 0 of
                    // the defaults buffer, integers at offset 16.
                    let (format, offset) = match input.base_type.as_str() {
                        "int" => (vk::Format::R32G32B32A32_SINT, 16),
                        "uint" => (vk::Format::R32G32B32A32_UINT, 16),
                        _ => (vk::Format::R32G32B32A32_SFLOAT, 0),
                    };
                    for l in 0..input.location_count.max(1) {
                        attrs.push(vk::VertexInputAttributeDescription { location: input.location + l, binding: NULL_VERTEX_BINDING, format, offset });
                    }
                }
            }
        }
        if need_null {
            bindings.push(vk::VertexInputBindingDescription { binding: NULL_VERTEX_BINDING, stride: 0, input_rate: vk::VertexInputRate::VERTEX });
        }
        (bindings, attrs)
    }

    /// The graphics pipeline for `key` (built on first use; failures are cached).
    pub fn graphics_pipeline(&mut self, gpu: &Gpu, arena: &mut Arena, key: &VariantKey, layout: Option<&'static VertexLayout>) -> Result<vk::Pipeline, String> {
        if let Some(r) = self.variants.get(key) {
            return r.clone();
        }
        let result = self.build_graphics(gpu, arena, key, layout);
        if let Ok(p) = &result {
            arena.pipelines.push(*p);
            self.pipelines_created += 1;
        }
        self.variants.insert(key.clone(), result.clone());
        result
    }

    fn build_graphics(&mut self, gpu: &Gpu, _arena: &mut Arena, key: &VariantKey, layout: Option<&'static VertexLayout>) -> Result<vk::Pipeline, String> {
        let main = CString::new("main").unwrap_or_default();
        let stages: Vec<vk::PipelineShaderStageCreateInfo<'_>> =
            self.stages.iter().map(|(s, m)| vk::PipelineShaderStageCreateInfo::default().stage(stage_flag(*s)).module(*m).name(&main)).collect();
        let (bindings, attrs) = match layout {
            Some(l) => self.vertex_input(gpu, l),
            None => {
                if !self.vertex_inputs.is_empty() {
                    let names: Vec<&str> = self.vertex_inputs.iter().map(|i| i.name.as_str()).collect();
                    return Err(format!("fullscreen draw, but the vertex stage declares inputs {names:?}"));
                }
                (Vec::new(), Vec::new())
            }
        };
        let vi = vk::PipelineVertexInputStateCreateInfo::default().vertex_binding_descriptions(&bindings).vertex_attribute_descriptions(&attrs);
        let ia = vk::PipelineInputAssemblyStateCreateInfo::default().topology(self.topology);
        let tess = vk::PipelineTessellationStateCreateInfo::default().patch_control_points(self.patch_control_points.unwrap_or(3));
        let mut clip = vk::PipelineViewportDepthClipControlCreateInfoEXT::default().negative_one_to_one(true);
        let mut vp = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
        if key.neg_one_to_one {
            vp = vp.push_next(&mut clip);
        }
        let rs = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(key.cull)
            .front_face(vk::FrontFace::CLOCKWISE)
            .line_width(1.0);
        let ms = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let ds = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(key.depth_test && key.depth_format != vk::Format::UNDEFINED)
            .depth_write_enable(key.depth_write && key.depth_format != vk::Format::UNDEFINED)
            .depth_compare_op(key.depth_compare);
        let blends: Vec<vk::PipelineColorBlendAttachmentState> = key
            .attachments
            .iter()
            .zip(&key.color_formats)
            .map(|(a, f)| {
                let mut s = vk::PipelineColorBlendAttachmentState::default().color_write_mask(if a.write && *f != vk::Format::UNDEFINED { vk::ColorComponentFlags::RGBA } else { vk::ColorComponentFlags::empty() });
                if let Some(b) = a.blend
                    && a.write
                    && gpu.format_features(*f).contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT_BLEND)
                {
                    s = s
                        .blend_enable(true)
                        .src_color_blend_factor(blend_factor(b.src_color))
                        .dst_color_blend_factor(blend_factor(b.dst_color))
                        .color_blend_op(vk::BlendOp::ADD)
                        .src_alpha_blend_factor(blend_factor(b.src_alpha))
                        .dst_alpha_blend_factor(blend_factor(b.dst_alpha))
                        .alpha_blend_op(vk::BlendOp::ADD);
                }
                s
            })
            .collect();
        let cb = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blends);
        let dynamic = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dy = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic);
        let mut rendering = vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&key.color_formats).depth_attachment_format(key.depth_format);
        let mut info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vi)
            .input_assembly_state(&ia)
            .viewport_state(&vp)
            .rasterization_state(&rs)
            .multisample_state(&ms)
            .depth_stencil_state(&ds)
            .color_blend_state(&cb)
            .dynamic_state(&dy)
            .layout(self.layout)
            .push_next(&mut rendering);
        if self.patch_control_points.is_some() {
            info = info.tessellation_state(&tess);
        }
        let r = unsafe { gpu.device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None) };
        match r {
            Ok(p) => {
                let p = p.into_iter().next().ok_or("no pipeline returned")?;
                gpu.set_name(p, &self.name);
                Ok(p)
            }
            Err((_, e)) => Err(format!("vkCreateGraphicsPipelines failed: {e:?}")),
        }
    }

    /// The compute pipeline (built on first use).
    pub fn compute_pipeline(&mut self, gpu: &Gpu, arena: &mut Arena) -> Result<vk::Pipeline, RuntimeError> {
        if let Some(p) = self.compute_pipeline {
            return Ok(p);
        }
        let main = CString::new("main").unwrap_or_default();
        let Some((_, module)) = self.stages.first() else {
            return Err(RuntimeError::InvalidRequest(format!("{}: no compute stage", self.name)));
        };
        let stage = vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::COMPUTE).module(*module).name(&main);
        let info = vk::ComputePipelineCreateInfo::default().stage(stage).layout(self.layout);
        let p = unsafe { gpu.device.create_compute_pipelines(vk::PipelineCache::null(), &[info], None) }.map_err(|(_, e)| e).vk("vkCreateComputePipelines")?;
        let p = p.into_iter().next().ok_or(RuntimeError::Vulkan { call: "vkCreateComputePipelines", result: vk::Result::ERROR_UNKNOWN })?;
        gpu.set_name(p, &self.name);
        arena.pipelines.push(p);
        self.compute_pipeline = Some(p);
        self.pipelines_created += 1;
        Ok(p)
    }

    /// Every fragment output location (arrays and matrices expanded) with its base type
    /// (`float`, `int`, `uint`).
    pub fn output_classes(&self) -> HashMap<u32, String> {
        self.fragment_outputs
            .iter()
            .flat_map(|o| (o.location..o.location.saturating_add(o.location_count.max(1))).map(move |l| (l, o.base_type.clone())))
            .collect()
    }
}

/// The blend of logical output `i` of `program` (per-buffer override, else global).
pub(crate) fn output_blend(program: &Program, target: u32) -> Option<BlendMode> {
    match program.blend_per_buffer.get(&target) {
        Some(b) => *b,
        None => program.blend,
    }
}

/// Iris' dispatch size for a compute program: absolute `workGroups`, or
/// `ceil(ceil(width * x) / local_x), ceil(ceil(height * y) / local_y), 1` for
/// `workGroupsRender` (default `1, 1`). Clamped to `max` per axis.
pub(crate) fn dispatch_size(work: Option<&sb_core::model::WorkGroups>, local: [u32; 3], width: u32, height: u32, max: [u32; 3]) -> [u32; 3] {
    use sb_core::model::WorkGroups;
    let size = match work {
        Some(WorkGroups::Absolute { x, y, z }) => [*x, *y, *z],
        Some(WorkGroups::Relative { x, y }) => {
            let rel = |scale: f32, extent: u32, l: u32| -> u32 {
                let scale = if scale.is_finite() { scale.max(0.0) } else { 0.0 };
                let px = (f64::from(extent) * f64::from(scale)).ceil();
                (px / f64::from(l.max(1))).ceil().min(f64::from(u32::MAX)) as u32
            };
            [rel(*x, width, local[0]), rel(*y, height, local[1]), 1]
        }
        None => [width.div_ceil(local[0].max(1)), height.div_ceil(local[1].max(1)), 1],
    };
    [size[0].min(max[0]), size[1].min(max[1]), size[2].min(max[2])]
}

/// Numeric class of a reflected output/input base type (`int`, `uint`, else float).
pub(crate) fn output_class(base_type: &str) -> crate::texel::NumericClass {
    use crate::texel::NumericClass as C;
    match base_type {
        "int" => C::Int,
        "uint" => C::Uint,
        _ => C::Float,
    }
}

/// Whether a reflected sampler result type matches a format's class.
pub(crate) fn sample_type_matches(t: ScalarKind, class: crate::texel::NumericClass) -> bool {
    use crate::texel::NumericClass as C;
    matches!((t, class), (ScalarKind::Float, C::Float) | (ScalarKind::Int, C::Int) | (ScalarKind::Uint, C::Uint))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_core::model::WorkGroups;

    #[test]
    fn dispatch_formula_follows_iris() {
        let max = [65535; 3];
        // Relative: ceil(ceil(W * s) / local).
        assert_eq!(dispatch_size(Some(&WorkGroups::Relative { x: 0.5, y: 0.25 }), [16, 16, 1], 641, 360, max), [21, 6, 1]);
        assert_eq!(dispatch_size(Some(&WorkGroups::Relative { x: 1.0, y: 1.0 }), [8, 8, 1], 640, 360, max), [80, 45, 1]);
        // Absolute.
        assert_eq!(dispatch_size(Some(&WorkGroups::Absolute { x: 4, y: 2, z: 3 }), [64, 1, 1], 640, 360, max), [4, 2, 3]);
        // Default: one invocation per pixel.
        assert_eq!(dispatch_size(None, [16, 16, 1], 640, 360, max), [40, 23, 1]);
        // Clamped to device limits; nonsense scales do not overflow.
        assert_eq!(dispatch_size(Some(&WorkGroups::Absolute { x: 1 << 30, y: 1, z: 1 }), [1, 1, 1], 1, 1, max), [65535, 1, 1]);
        assert_eq!(dispatch_size(Some(&WorkGroups::Relative { x: f32::NAN, y: -1.0 }), [1, 1, 1], 10, 10, max), [0, 0, 1]);
        assert_eq!(dispatch_size(Some(&WorkGroups::Relative { x: 1e30, y: 1.0 }), [1, 0, 1], 10, 10, max), [65535, 10, 1]);
    }

    #[test]
    fn attribute_defaults_are_gl_current_attributes() {
        let b = attribute_defaults();
        let f: Vec<f32> = b[..16].chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        let i: Vec<i32> = b[16..].chunks(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();
        assert_eq!(f, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(i, [0, 0, 0, 1]);
    }

    #[test]
    fn host_samplers_and_blend_enums() {
        assert_eq!(host_sampler("Sampler0"), Some(ResourceRef::Atlas));
        assert_eq!(host_sampler("uBlockAtlas"), Some(ResourceRef::DhBlockAtlas));
        assert_eq!(host_sampler("colortex0"), None);
        assert_eq!(gl_blend_factor(BlendFactor::OneMinusSrcAlpha), 0x0303);
    }

    #[test]
    fn roles_from_reflection() {
        use sb_core::model::{BindingEntry, BlockLayout, UniformLayout};
        let src = "#version 450
layout(std140, set = 0, binding = 0) uniform sb_Frame { layout(offset = 0) float frameTimeCounter; };
layout(std140, set = 0, binding = 2) uniform Globals { ivec3 CameraBlockPos; } sb_hGlobals;
layout(std140, set = 0, binding = 3) uniform MyPackBlock { vec4 v; } pb;
layout(set = 1, binding = 0) uniform sampler2D Sampler0;
layout(set = 1, binding = 1) uniform sampler2D colortex4;
layout(set = 1, binding = 7) uniform sampler2D mystery;
layout(location = 0) out vec4 o;
void main() { o = texture(Sampler0, vec2(0)) + texture(colortex4, vec2(0)) + texture(mystery, vec2(0)) + vec4(frameTimeCounter) + vec4(sb_hGlobals.CameraBlockPos.x) + pb.v; }
";
        let spirv = sb_compile::compile_glsl(src, ShaderStage::Fragment, "t.fsh", &Default::default(), None).unwrap();
        let refl = sb_compile::reflect(&spirv).unwrap();
        let mut dim_json = serde_json_dim();
        dim_json.uniforms = UniformLayout { frame: BlockLayout { name: "sb_Frame".into(), set: 0, binding: 0, size: 16, members: vec![] }, draw: BlockLayout::default() };
        dim_json.bindings.entries = vec![
            BindingEntry { name: "gtexture".into(), set: 1, binding: 0, kind: ResourceKind::Sampler { dim: "2d".into(), shadow: false, sample_type: "float".into() }, resource: ResourceRef::Atlas },
            BindingEntry { name: "colortex4".into(), set: 1, binding: 1, kind: ResourceKind::Sampler { dim: "2d".into(), shadow: false, sample_type: "float".into() }, resource: ResourceRef::ColorTex(4) },
        ];
        let role = |n: &str| resolve_role(refl.descriptor_by_name(n).unwrap(), &dim_json);
        assert_eq!(role("sb_Frame"), Role::Frame);
        assert_eq!(role("sb_hGlobals"), Role::HostBlock("Globals".into()));
        assert_eq!(role("pb"), Role::PackUbo);
        assert_eq!(role("Sampler0"), Role::Resource(ResourceRef::Atlas));
        assert_eq!(role("colortex4"), Role::Resource(ResourceRef::ColorTex(4)));
        assert_eq!(role("mystery"), Role::Resource(ResourceRef::Unknown("mystery".into())));
    }

    fn serde_json_dim() -> DimensionPipeline {
        crate::testutil::empty_dimension()
    }
}
