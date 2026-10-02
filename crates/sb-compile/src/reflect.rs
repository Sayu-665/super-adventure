//! SPIR-V reflection: descriptors, push constants, stage interface, compute size.
//!
//! Descriptor classification and block layouts come from
//! [spirq](https://docs.rs/spirq). The stage interface (locations, types,
//! interpolation qualifiers, interface-block members), storage-image sample
//! types, built-ins, capabilities and `WorkgroupSize` constants, which spirq does
//! not expose (or drops), come from a word-level scan of the module
//! (`crate::module`).

use crate::module::{self, ModuleInfo, TypeDef};
use sb_core::{GlslType, ScalarKind, ShaderStage};
use serde::{Deserialize, Serialize};
use spirq::spirv::{self, Decoration, Dim, ExecutionModel, ImageFormat};
use spirq::ty::{AccessType, MatrixAxisOrder, ScalarType, Type};
use spirq::var::Variable;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Reflection data of one shader module (its `main`-style entry point).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reflection {
    /// Stage of the entry point.
    pub stage: ShaderStage,
    /// Entry point name (`main` for everything ShaderBridge compiles).
    pub entry_point: String,
    /// SPIR-V version of the module, `(major, minor)`.
    pub spirv_version: (u8, u8),
    /// Every descriptor declared by the module (used or not), sorted by
    /// `(set, binding, name)`.
    pub descriptors: Vec<Descriptor>,
    /// Size in bytes of the push-constant block (0 if none).
    pub push_constant_size: u32,
    /// The push-constant block, if any.
    pub push_constants: Option<PushConstantBlock>,
    /// User (non-built-in) inputs, sorted by `(location, component, index)`.
    /// Interface blocks and struct variables are expanded into one entry per
    /// member (see [`InterfaceVar::name`]).
    pub inputs: Vec<InterfaceVar>,
    /// User (non-built-in) outputs, sorted like [`Reflection::inputs`].
    pub outputs: Vec<InterfaceVar>,
    /// Built-in inputs in the entry point interface (`FragCoord`, `VertexIndex`, ...).
    pub builtin_inputs: Vec<String>,
    /// Built-in outputs in the entry point interface (`Position`, `FragDepth`, ...).
    /// Members of `gl_PerVertex` are listed even when they are not written.
    pub builtin_outputs: Vec<String>,
    /// Compute work-group size (`local_size_x/y/z`), for compute shaders.
    pub local_size: Option<[u32; 3]>,
    /// Execution modes of the entry point (`OriginUpperLeft`, `DepthReplacing`,
    /// `Triangles`, `OutputVertices`, ...).
    pub execution_modes: Vec<ExecutionModeInfo>,
    /// Declared capabilities (`Shader`, `Geometry`, `StorageImageReadWithoutFormat`, ...).
    pub capabilities: Vec<String>,
    /// Declared SPIR-V extensions.
    pub extensions: Vec<String>,
}

impl Reflection {
    /// The descriptor at `(set, binding)` (the first, if several alias it).
    pub fn descriptor(&self, set: u32, binding: u32) -> Option<&Descriptor> {
        self.descriptors.iter().find(|d| d.set == set && d.binding == binding)
    }

    /// The descriptor whose variable name or block type name is `name`.
    pub fn descriptor_by_name(&self, name: &str) -> Option<&Descriptor> {
        self.descriptors
            .iter()
            .find(|d| d.name == name)
            .or_else(|| self.descriptors.iter().find(|d| d.block_type_name.as_deref() == Some(name)))
    }

    /// The input at `location` (component 0 first).
    pub fn input(&self, location: u32) -> Option<&InterfaceVar> {
        self.inputs.iter().find(|v| v.location == location)
    }

    /// The input named `name`.
    pub fn input_by_name(&self, name: &str) -> Option<&InterfaceVar> {
        self.inputs.iter().find(|v| v.name == name)
    }

    /// The output at `location` (component 0 first).
    pub fn output(&self, location: u32) -> Option<&InterfaceVar> {
        self.outputs.iter().find(|v| v.location == location)
    }

    /// The output named `name`.
    pub fn output_by_name(&self, name: &str) -> Option<&InterfaceVar> {
        self.outputs.iter().find(|v| v.name == name)
    }

    /// Whether the entry point has execution mode `mode` (SPIR-V spelling, e.g. `DepthReplacing`).
    pub fn has_execution_mode(&self, mode: &str) -> bool {
        self.execution_modes.iter().any(|m| m.mode == mode)
    }

    /// Whether the module declares capability `cap` (SPIR-V spelling, e.g. `Geometry`).
    pub fn has_capability(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }
}

/// One descriptor binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Descriptor {
    /// `DescriptorSet` decoration (0 when absent).
    pub set: u32,
    /// `Binding` decoration (0 when absent).
    pub binding: u32,
    /// The variable name. For anonymous blocks (whose variable has no name) this is
    /// the block type name. Empty if the module carries no debug names.
    pub name: String,
    /// Block type name (`uniform sb_Frame { ... }` -> `sb_Frame`) for uniform and
    /// storage buffers.
    pub block_type_name: Option<String>,
    /// Resource type and layout.
    pub kind: DescriptorKind,
    /// Descriptor array size (`uniform sampler2D tex[4]` -> 4); 1 if not an
    /// array; 0 for runtime-sized arrays.
    pub count: u32,
}

/// What kind of resource a descriptor is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum DescriptorKind {
    /// `sampler*` (`VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER`).
    CombinedImageSampler {
        /// Image dimensionality.
        dim: ImageDim,
        /// `*Array` sampler.
        arrayed: bool,
        /// `*Shadow` (depth-comparison) sampler; the host binds a compare sampler.
        shadow: bool,
        /// `sampler2DMS*`.
        multisampled: bool,
        /// Result type: `Float` (`sampler*`), `Int` (`isampler*`), `Uint` (`usampler*`).
        sample_type: ScalarKind,
    },
    /// `texture*` (`VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE`).
    SampledImage {
        /// Image dimensionality.
        dim: ImageDim,
        /// `*Array` texture.
        arrayed: bool,
        /// `texture2DMS*`.
        multisampled: bool,
        /// Result type (`Float`, `Int`, `Uint`).
        sample_type: ScalarKind,
    },
    /// `sampler` / `samplerShadow` (`VK_DESCRIPTOR_TYPE_SAMPLER`).
    Sampler,
    /// `image*` (`VK_DESCRIPTOR_TYPE_STORAGE_IMAGE`).
    StorageImage {
        /// Image dimensionality.
        dim: ImageDim,
        /// `*Array` image.
        arrayed: bool,
        /// `image2DMS*`.
        multisampled: bool,
        /// GLSL format qualifier (`rgba16f`), `None` when declared without one
        /// (requires `shaderStorageImageReadWithoutFormat` to be read).
        format: Option<String>,
        /// `readonly` / `writeonly` qualifiers.
        access: Access,
        /// Texel type: `Float` (`image*`), `Int` (`iimage*`), `Uint` (`uimage*`).
        sample_type: ScalarKind,
    },
    /// Uniform block (`VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER`).
    UniformBuffer {
        /// Minimal byte size: the end of the last member.
        size: u32,
        /// Members in declaration order, with their explicit offsets.
        members: Vec<BufferMember>,
    },
    /// Shader storage block (`VK_DESCRIPTOR_TYPE_STORAGE_BUFFER`).
    StorageBuffer {
        /// Byte size of the fixed part (a trailing runtime-sized array counts 0).
        size: u32,
        /// Members in declaration order.
        members: Vec<BufferMember>,
        /// `readonly` / `writeonly` qualifiers.
        access: Access,
    },
    /// `samplerBuffer` (`VK_DESCRIPTOR_TYPE_UNIFORM_TEXEL_BUFFER`).
    UniformTexelBuffer {
        /// Result type (`Float`, `Int`, `Uint`).
        sample_type: ScalarKind,
    },
    /// `imageBuffer` (`VK_DESCRIPTOR_TYPE_STORAGE_TEXEL_BUFFER`).
    StorageTexelBuffer {
        /// GLSL format qualifier, if declared.
        format: Option<String>,
        /// `readonly` / `writeonly` qualifiers.
        access: Access,
        /// Texel type: `Float` (`imageBuffer`), `Int` (`iimageBuffer`), `Uint` (`uimageBuffer`).
        sample_type: ScalarKind,
    },
    /// `subpassInput` (`VK_DESCRIPTOR_TYPE_INPUT_ATTACHMENT`).
    InputAttachment {
        /// `input_attachment_index`.
        index: u32,
    },
    /// Ray-tracing acceleration structure.
    AccelerationStructure,
}

impl DescriptorKind {
    /// The matching `VkDescriptorType` value.
    pub fn vk_descriptor_type(&self) -> u32 {
        match self {
            Self::Sampler => 0,
            Self::CombinedImageSampler { .. } => 1,
            Self::SampledImage { .. } => 2,
            Self::StorageImage { .. } => 3,
            Self::UniformTexelBuffer { .. } => 4,
            Self::StorageTexelBuffer { .. } => 5,
            Self::UniformBuffer { .. } => 6,
            Self::StorageBuffer { .. } => 7,
            Self::InputAttachment { .. } => 10,
            Self::AccelerationStructure => 1_000_150_000,
        }
    }

    /// The image dimensionality in `sb_core::model::ResourceKind` spelling
    /// (`1d`, `2d`, `3d`, `cube`, `2d_array`, `cube_array`, `2d_rect`, `buffer`,
    /// `2d_ms`, `2d_ms_array`), or `None` for non-image descriptors.
    pub fn core_dim(&self) -> Option<String> {
        let (dim, arrayed, ms) = match self {
            Self::CombinedImageSampler { dim, arrayed, multisampled, .. }
            | Self::SampledImage { dim, arrayed, multisampled, .. }
            | Self::StorageImage { dim, arrayed, multisampled, .. } => (*dim, *arrayed, *multisampled),
            Self::UniformTexelBuffer { .. } | Self::StorageTexelBuffer { .. } => (ImageDim::Buffer, false, false),
            _ => return None,
        };
        let mut s = String::from(dim.as_str());
        if ms {
            s.push_str("_ms");
        }
        if arrayed {
            s.push_str("_array");
        }
        Some(s)
    }

    /// Members of a uniform or storage buffer.
    pub fn members(&self) -> Option<&[BufferMember]> {
        match self {
            Self::UniformBuffer { members, .. } | Self::StorageBuffer { members, .. } => Some(members),
            _ => None,
        }
    }
}

/// Image dimensionality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImageDim {
    /// `*1D`.
    #[serde(rename = "1d")]
    D1,
    /// `*2D`.
    #[serde(rename = "2d")]
    D2,
    /// `*3D`.
    #[serde(rename = "3d")]
    D3,
    /// `*Cube`.
    #[serde(rename = "cube")]
    Cube,
    /// `*2DRect`.
    #[serde(rename = "2d_rect")]
    Rect,
    /// `*Buffer`.
    #[serde(rename = "buffer")]
    Buffer,
    /// `subpassInput*`.
    #[serde(rename = "subpass")]
    SubpassData,
}

impl ImageDim {
    /// `1d`, `2d`, `3d`, `cube`, `2d_rect`, `buffer`, `subpass`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::D1 => "1d",
            Self::D2 => "2d",
            Self::D3 => "3d",
            Self::Cube => "cube",
            Self::Rect => "2d_rect",
            Self::Buffer => "buffer",
            Self::SubpassData => "subpass",
        }
    }

    fn from_spirv(dim: Dim) -> Self {
        match dim {
            Dim::Dim1D => Self::D1,
            Dim::Dim3D => Self::D3,
            Dim::DimCube => Self::Cube,
            Dim::DimRect => Self::Rect,
            Dim::DimBuffer => Self::Buffer,
            Dim::DimSubpassData => Self::SubpassData,
            _ => Self::D2,
        }
    }
}

/// Shader access to a storage resource (from `readonly` / `writeonly`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// `readonly` (`NonWritable`).
    ReadOnly,
    /// `writeonly` (`NonReadable`).
    WriteOnly,
    /// Neither qualifier.
    ReadWrite,
}

impl From<AccessType> for Access {
    fn from(a: AccessType) -> Self {
        match a {
            AccessType::ReadOnly => Self::ReadOnly,
            AccessType::WriteOnly => Self::WriteOnly,
            AccessType::ReadWrite => Self::ReadWrite,
        }
    }
}

/// A member of a uniform/storage/push-constant block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BufferMember {
    /// Member name (`_m<index>` if the module has no debug names).
    pub name: String,
    /// Byte offset from the start of the block.
    pub offset: u32,
    /// Byte size (array stride x length for arrays; 0 for runtime-sized arrays;
    /// column/row stride x count for matrices).
    pub size: u32,
    /// GLSL spelling of the type (`float`, `vec3`, `mat4`, `float[4]`, `uint[]`, `Light`).
    pub type_name: String,
    /// The type as a [`GlslType`] when it is a scalar, vector or matrix, or a
    /// one-dimensional sized array of one.
    pub glsl_type: Option<GlslType>,
    /// Array stride for array members.
    pub array_stride: Option<u32>,
    /// Matrix stride for matrix (or matrix array) members.
    pub matrix_stride: Option<u32>,
    /// `row_major` matrix layout.
    pub row_major: bool,
}

/// The push-constant block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushConstantBlock {
    /// Variable name (or the block type name if the variable is unnamed).
    pub name: String,
    /// Block type name, e.g. `PushConstants`.
    pub block_type_name: Option<String>,
    /// End of the last member, in bytes.
    pub size: u32,
    /// Members in declaration order.
    pub members: Vec<BufferMember>,
}

/// A user stage input or output.
///
/// Interface blocks (`out VertexData { vec2 uv; flat int id; } vd;`) and
/// struct-typed variables are reported member by member, each with its own
/// location (explicit member `location` or consecutive after the block's),
/// type and interpolation qualifiers. Only arrays of blocks/structs (other
/// than the implicit per-vertex array) are reported as one `struct` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceVar {
    /// `Location` decoration.
    pub location: u32,
    /// `Component` decoration (0 when absent).
    pub component: u32,
    /// `Index` decoration of fragment outputs (dual-source blending); 0 when absent.
    #[serde(default)]
    pub index: u32,
    /// Variable name; `<instance>.<member>` for members of interface blocks and
    /// struct variables (`<BlockType>.<member>` for anonymous block instances).
    /// Empty without debug names.
    pub name: String,
    /// `float`, `int`, `uint`, `double`, `bool`, `float16_t`, `int64_t`, `uint64_t`,
    /// ... or `struct` for arrays of structs/blocks.
    pub base_type: String,
    /// Vector size (rows for matrices); 1 for scalars.
    pub vec_size: u32,
    /// Matrix columns; 1 for scalars and vectors.
    pub columns: u32,
    /// Length of a user-declared array (the product of all dimensions for
    /// multi-dimensional arrays; `Some(0)` if unsized). The implicit per-vertex
    /// array of geometry/tessellation interfaces is not included, see
    /// [`InterfaceVar::per_vertex`].
    pub array_len: Option<u32>,
    /// The variable is implicitly arrayed per vertex (geometry inputs,
    /// tessellation control inputs/outputs and tessellation evaluation inputs,
    /// except `patch` variables).
    pub per_vertex: bool,
    /// Number of consecutive locations the variable occupies (per vertex).
    pub location_count: u32,
    /// `flat` interpolation.
    pub flat: bool,
    /// `noperspective` interpolation.
    pub noperspective: bool,
    /// `centroid` auxiliary qualifier.
    pub centroid: bool,
    /// `sample` auxiliary qualifier.
    pub sample: bool,
    /// `patch` (per-patch tessellation variable).
    pub patch: bool,
}

impl InterfaceVar {
    /// GLSL spelling of the variable's type, without the per-vertex array
    /// (`vec4`, `uvec2`, `mat3`, `float[4]`, `struct`).
    pub fn glsl_type_name(&self) -> String {
        let base = if self.base_type == "struct" {
            "struct".to_string()
        } else if self.columns > 1 {
            let p = if self.base_type == "double" { "dmat" } else { "mat" };
            if self.columns == self.vec_size {
                format!("{p}{}", self.columns)
            } else {
                format!("{p}{}x{}", self.columns, self.vec_size)
            }
        } else if self.vec_size > 1 {
            format!("{}{}", vector_prefix(&self.base_type), self.vec_size)
        } else {
            self.base_type.clone()
        };
        match self.array_len {
            Some(0) => format!("{base}[]"),
            Some(n) => format!("{base}[{n}]"),
            None => base,
        }
    }
}

/// An execution mode with its literal operands (id operands are resolved to
/// constant values where possible).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionModeInfo {
    /// SPIR-V spelling, e.g. `LocalSize`, `OriginUpperLeft`, `OutputVertices`.
    pub mode: String,
    /// Literal operands (e.g. `[8, 8, 1]` for `LocalSize`).
    pub operands: Vec<u32>,
}

/// Deepest type nesting accepted by [`reflect`] (struct in array in struct ...).
const MAX_TYPE_NESTING: u32 = 64;

/// Largest type accepted by [`reflect`], in nodes when expanded as a tree.
const MAX_TYPE_TREE_NODES: u64 = 200_000;

/// Reflect a SPIR-V module.
///
/// Errors (never panics) on malformed modules, on modules without an entry
/// point, on execution models other than the six graphics/compute stages, on
/// types nested more than 64 levels deep or expanding to more than 200,000
/// nodes, and on descriptors Vulkan does not allow (arrays of arrays).
/// When the module has several entry points, `main` is preferred, else the
/// first one.
pub fn reflect(spirv: &[u32]) -> Result<Reflection, String> {
    let (header, info) = module::scan(spirv)?;
    if let Some(op) = info.unknown_opcodes.first() {
        return Err(format!("unsupported SPIR-V opcode {op} (newer than the reflection library)"));
    }
    if info.entry_points.is_empty() {
        return Err("SPIR-V module has no entry point".into());
    }
    // spirq expands every type into an owned tree (shared struct types are
    // copied per use, deep nesting is cloned recursively): bound both so that a
    // crafted module cannot exhaust memory, time or the stack.
    match info.largest_type_tree(MAX_TYPE_NESTING) {
        None => return Err(format!("SPIR-V types nest deeper than {MAX_TYPE_NESTING} levels")),
        Some(nodes) if nodes > MAX_TYPE_TREE_NODES => {
            return Err(format!("SPIR-V types too large to reflect ({nodes} nodes when expanded)"));
        }
        Some(_) => {}
    }

    // spirq can panic on some malformed modules; contain that.
    let entry_points = catch_unwind(AssertUnwindSafe(|| {
        spirq::ReflectConfig::new()
            .spv(spirv)
            .ref_all_rscs(true)
            .combine_img_samplers(false)
            .gen_unique_names(false)
            .reflect()
    }))
    .map_err(|_| "SPIR-V reflection failed (malformed module)".to_string())?
    .map_err(|e| format!("SPIR-V reflection failed: {e}"))?;

    let ep = entry_points
        .iter()
        .find(|e| e.name == "main")
        .or_else(|| entry_points.first())
        .ok_or_else(|| "SPIR-V module has no entry point".to_string())?;
    let stage = match ep.exec_model {
        ExecutionModel::Vertex => ShaderStage::Vertex,
        ExecutionModel::TessellationControl => ShaderStage::TessControl,
        ExecutionModel::TessellationEvaluation => ShaderStage::TessEval,
        ExecutionModel::Geometry => ShaderStage::Geometry,
        ExecutionModel::Fragment => ShaderStage::Fragment,
        ExecutionModel::GLCompute => ShaderStage::Compute,
        other => return Err(format!("unsupported execution model {other:?}")),
    };
    let (ep_func, ep_interface) = info
        .entry_points
        .iter()
        .find(|(model, _, name, _)| *model == ep.exec_model as u32 && *name == ep.name)
        .map(|(_, func, _, iface)| (*func, iface.as_slice()))
        .unwrap_or((u32::MAX, &[]));

    let mut descriptors = Vec::new();
    let mut push_constants = None;
    for var in &ep.vars {
        match var {
            Variable::Descriptor { name, desc_bind, desc_ty, ty, nbind } => {
                let (set, binding) = (desc_bind.set(), desc_bind.bind());
                let texel_type = find_descriptor_var(&info, set, binding, name.as_deref())
                    .map_or(ScalarKind::Float, |id| image_texel_kind(&info, id));
                if let Some(d) = make_descriptor(name.as_deref(), set, binding, desc_ty, ty, *nbind, texel_type) {
                    descriptors.push(d);
                }
            }
            Variable::PushConstant { name, ty } => {
                if let Type::Struct(st) = ty {
                    let block_type_name = st.name.clone().filter(|n| !n.is_empty());
                    let members = buffer_members(st);
                    push_constants = Some(PushConstantBlock {
                        name: var_name(name.as_deref(), block_type_name.as_deref()),
                        block_type_name,
                        size: struct_size(st),
                        members,
                    });
                }
            }
            // The stage interface comes from the module scan (`interface_vars`):
            // spirq drops blocks whose members carry the locations.
            Variable::Input { .. } | Variable::Output { .. } | Variable::SpecConstant { .. } => {}
        }
    }
    descriptors.sort_by(|a, b| (a.set, a.binding, &a.name).cmp(&(b.set, b.binding, &b.name)));
    check_all_descriptors_reported(&info, &descriptors)?;
    let inputs = interface_vars(&info, stage, ep_interface, true);
    let outputs = interface_vars(&info, stage, ep_interface, false);

    let (builtin_inputs, builtin_outputs) = builtins(&info, ep_interface);
    let execution_modes = execution_modes(&info, ep_func);
    let local_size = (stage == ShaderStage::Compute).then(|| local_size(&info, ep_func)).flatten();

    Ok(Reflection {
        stage,
        entry_point: ep.name.clone(),
        spirv_version: header.version_pair(),
        push_constant_size: push_constants.as_ref().map_or(0, |p| p.size),
        push_constants,
        descriptors,
        inputs,
        outputs,
        builtin_inputs,
        builtin_outputs,
        local_size,
        execution_modes,
        capabilities: info.capabilities.iter().map(|&c| capability_name(c)).collect(),
        extensions: info.extensions.clone(),
    })
}

fn var_name(name: Option<&str>, block_type_name: Option<&str>) -> String {
    match name {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => block_type_name.unwrap_or_default().to_string(),
    }
}

/// `count` is spirq's descriptor array size; `texel_type` is the texel type of
/// storage images and storage texel buffers (from the module scan).
fn make_descriptor(
    name: Option<&str>,
    set: u32,
    binding: u32,
    desc_ty: &spirq::ty::DescriptorType,
    ty: &Type,
    count: u32,
    texel_type: ScalarKind,
) -> Option<Descriptor> {
    use spirq::ty::DescriptorType as D;
    let mut block_type_name = None;
    let kind = match (desc_ty, ty) {
        (D::CombinedImageSampler(), Type::CombinedImageSampler(t)) => {
            let s = &t.sampled_image_ty;
            DescriptorKind::CombinedImageSampler {
                dim: ImageDim::from_spirv(s.dim),
                arrayed: s.is_array,
                shadow: s.is_depth == Some(true),
                multisampled: s.is_multisampled,
                sample_type: scalar_kind(&s.scalar_ty),
            }
        }
        (D::SampledImage(), Type::SampledImage(s)) => DescriptorKind::SampledImage {
            dim: ImageDim::from_spirv(s.dim),
            arrayed: s.is_array,
            multisampled: s.is_multisampled,
            sample_type: scalar_kind(&s.scalar_ty),
        },
        (D::UniformTexelBuffer(), Type::CombinedImageSampler(t)) => {
            DescriptorKind::UniformTexelBuffer { sample_type: scalar_kind(&t.sampled_image_ty.scalar_ty) }
        }
        (D::UniformTexelBuffer(), Type::SampledImage(s)) => {
            DescriptorKind::UniformTexelBuffer { sample_type: scalar_kind(&s.scalar_ty) }
        }
        (D::Sampler(), _) => DescriptorKind::Sampler,
        (D::StorageImage(access), Type::StorageImage(s)) => DescriptorKind::StorageImage {
            dim: ImageDim::from_spirv(s.dim),
            arrayed: s.is_array,
            multisampled: s.is_multisampled,
            format: image_format_qualifier(s.fmt).map(str::to_string),
            access: (*access).into(),
            sample_type: texel_type,
        },
        (D::StorageTexelBuffer(access), Type::StorageImage(s)) => DescriptorKind::StorageTexelBuffer {
            format: image_format_qualifier(s.fmt).map(str::to_string),
            access: (*access).into(),
            sample_type: texel_type,
        },
        (D::UniformBuffer(), Type::Struct(st)) => {
            block_type_name = st.name.clone().filter(|n| !n.is_empty());
            DescriptorKind::UniformBuffer { size: struct_size(st), members: buffer_members(st) }
        }
        (D::StorageBuffer(access), Type::Struct(st)) => {
            block_type_name = st.name.clone().filter(|n| !n.is_empty());
            DescriptorKind::StorageBuffer { size: struct_size(st), members: buffer_members(st), access: (*access).into() }
        }
        (D::InputAttachment(index), _) => DescriptorKind::InputAttachment { index: *index },
        (D::AccelStruct(), _) => DescriptorKind::AccelerationStructure,
        _ => return None,
    };
    Some(Descriptor { set, binding, name: var_name(name, block_type_name.as_deref()), block_type_name, kind, count })
}

fn scalar_kind(s: &ScalarType) -> ScalarKind {
    match s {
        ScalarType::Integer { is_signed: true, .. } => ScalarKind::Int,
        ScalarType::Integer { is_signed: false, .. } => ScalarKind::Uint,
        ScalarType::Float { bits: 64 } => ScalarKind::Double,
        ScalarType::Boolean => ScalarKind::Bool,
        _ => ScalarKind::Float,
    }
}

/// GLSL image format qualifier for a SPIR-V image format.
pub(crate) fn image_format_qualifier(f: ImageFormat) -> Option<&'static str> {
    use ImageFormat as F;
    Some(match f {
        F::Unknown => return None,
        F::Rgba32f => "rgba32f",
        F::Rgba16f => "rgba16f",
        F::R32f => "r32f",
        F::Rgba8 => "rgba8",
        F::Rgba8Snorm => "rgba8_snorm",
        F::Rg32f => "rg32f",
        F::Rg16f => "rg16f",
        F::R11fG11fB10f => "r11f_g11f_b10f",
        F::R16f => "r16f",
        F::Rgba16 => "rgba16",
        F::Rgb10A2 => "rgb10_a2",
        F::Rg16 => "rg16",
        F::Rg8 => "rg8",
        F::R16 => "r16",
        F::R8 => "r8",
        F::Rgba16Snorm => "rgba16_snorm",
        F::Rg16Snorm => "rg16_snorm",
        F::Rg8Snorm => "rg8_snorm",
        F::R16Snorm => "r16_snorm",
        F::R8Snorm => "r8_snorm",
        F::Rgba32i => "rgba32i",
        F::Rgba16i => "rgba16i",
        F::Rgba8i => "rgba8i",
        F::R32i => "r32i",
        F::Rg32i => "rg32i",
        F::Rg16i => "rg16i",
        F::Rg8i => "rg8i",
        F::R16i => "r16i",
        F::R8i => "r8i",
        F::Rgba32ui => "rgba32ui",
        F::Rgba16ui => "rgba16ui",
        F::Rgba8ui => "rgba8ui",
        F::R32ui => "r32ui",
        F::Rgb10a2ui => "rgb10_a2ui",
        F::Rg32ui => "rg32ui",
        F::Rg16ui => "rg16ui",
        F::Rg8ui => "rg8ui",
        F::R16ui => "r16ui",
        F::R8ui => "r8ui",
        F::R64ui => "r64ui",
        F::R64i => "r64i",
    })
}

fn clamp_u32(v: u64) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

fn scalar_bytes(s: &ScalarType) -> Option<u64> {
    match s {
        ScalarType::Integer { bits, .. } | ScalarType::Float { bits } => Some(u64::from(*bits) / 8),
        // Booleans have no defined size; glslang never emits them in blocks.
        ScalarType::Boolean => Some(4),
        ScalarType::Void => None,
    }
}

/// Byte size of a type inside an explicitly laid-out block.
fn type_size(ty: &Type) -> Option<u64> {
    Some(match ty {
        Type::Scalar(s) => scalar_bytes(s)?,
        Type::Vector(v) => scalar_bytes(&v.scalar_ty)? * u64::from(v.nscalar),
        Type::Matrix(m) => {
            let stride = m.stride? as u64;
            match m.axis_order {
                Some(MatrixAxisOrder::RowMajor) => stride * u64::from(m.vector_ty.nscalar),
                _ => stride * u64::from(m.nvector),
            }
        }
        Type::Array(a) => match a.nelement {
            None => 0,
            Some(n) => match a.stride {
                Some(s) => s as u64 * u64::from(n),
                None => type_size(&a.element_ty)? * u64::from(n),
            },
        },
        Type::Struct(st) => st
            .members
            .iter()
            .filter_map(|m| Some(m.offset? as u64 + type_size(&m.ty)?))
            .max()
            .unwrap_or(0),
        Type::DevicePointer(_) | Type::DeviceAddress(_) => 8,
        _ => return None,
    })
}

fn struct_size(st: &spirq::ty::StructType) -> u32 {
    clamp_u32(type_size(&Type::Struct(st.clone())).unwrap_or(0))
}

fn buffer_members(st: &spirq::ty::StructType) -> Vec<BufferMember> {
    st.members
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut elem = &m.ty;
            let mut array_stride = None;
            while let Type::Array(a) = elem {
                if array_stride.is_none() {
                    array_stride = a.stride.map(|s| clamp_u32(s as u64));
                }
                elem = &a.element_ty;
            }
            let (matrix_stride, row_major) = match elem {
                Type::Matrix(mt) => (mt.stride.map(|s| clamp_u32(s as u64)), mt.axis_order == Some(MatrixAxisOrder::RowMajor)),
                _ => (None, false),
            };
            BufferMember {
                name: m.name.clone().unwrap_or_else(|| format!("_m{i}")),
                offset: clamp_u32(m.offset.unwrap_or(0) as u64),
                size: clamp_u32(type_size(&m.ty).unwrap_or(0)),
                type_name: type_name(&m.ty),
                glsl_type: glsl_type(&m.ty),
                array_stride,
                matrix_stride,
                row_major,
            }
        })
        .collect()
}

fn base_type_name(s: &ScalarType) -> &'static str {
    match s {
        ScalarType::Void => "void",
        ScalarType::Boolean => "bool",
        ScalarType::Float { bits: 16 } => "float16_t",
        ScalarType::Float { bits: 64 } => "double",
        ScalarType::Float { .. } => "float",
        ScalarType::Integer { bits: 8, is_signed: true } => "int8_t",
        ScalarType::Integer { bits: 8, is_signed: false } => "uint8_t",
        ScalarType::Integer { bits: 16, is_signed: true } => "int16_t",
        ScalarType::Integer { bits: 16, is_signed: false } => "uint16_t",
        ScalarType::Integer { bits: 64, is_signed: true } => "int64_t",
        ScalarType::Integer { bits: 64, is_signed: false } => "uint64_t",
        ScalarType::Integer { is_signed: true, .. } => "int",
        ScalarType::Integer { is_signed: false, .. } => "uint",
    }
}

fn vector_prefix(base: &str) -> String {
    match base {
        "float" => "vec".into(),
        "double" => "dvec".into(),
        "int" => "ivec".into(),
        "uint" => "uvec".into(),
        "bool" => "bvec".into(),
        "float16_t" => "f16vec".into(),
        "int64_t" => "i64vec".into(),
        "uint64_t" => "u64vec".into(),
        "int16_t" => "i16vec".into(),
        "uint16_t" => "u16vec".into(),
        "int8_t" => "i8vec".into(),
        "uint8_t" => "u8vec".into(),
        other => format!("{other}vec"),
    }
}

/// GLSL spelling of a block member type.
fn type_name(ty: &Type) -> String {
    let mut dims = String::new();
    let mut elem = ty;
    while let Type::Array(a) = elem {
        match a.nelement {
            Some(n) => dims.push_str(&format!("[{n}]")),
            None => dims.push_str("[]"),
        }
        elem = &a.element_ty;
    }
    let base = match elem {
        Type::Scalar(s) => base_type_name(s).to_string(),
        Type::Vector(v) => format!("{}{}", vector_prefix(base_type_name(&v.scalar_ty)), v.nscalar),
        Type::Matrix(m) => {
            let p = if matches!(m.vector_ty.scalar_ty, ScalarType::Float { bits: 64 }) { "dmat" } else { "mat" };
            let (cols, rows) = (m.nvector, m.vector_ty.nscalar);
            if cols == rows { format!("{p}{cols}") } else { format!("{p}{cols}x{rows}") }
        }
        Type::Struct(st) => st.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| "struct".into()),
        other => other.to_string(),
    };
    format!("{base}{dims}")
}

fn glsl_scalar(s: &ScalarType) -> Option<ScalarKind> {
    Some(match s {
        ScalarType::Float { bits: 32 } => ScalarKind::Float,
        ScalarType::Float { bits: 64 } => ScalarKind::Double,
        ScalarType::Integer { bits: 32, is_signed: true } => ScalarKind::Int,
        ScalarType::Integer { bits: 32, is_signed: false } => ScalarKind::Uint,
        ScalarType::Boolean => ScalarKind::Bool,
        _ => return None,
    })
}

fn glsl_type(ty: &Type) -> Option<GlslType> {
    let small = |n: u32| (2..=4).contains(&n).then_some(n as u8);
    match ty {
        Type::Scalar(s) => Some(GlslType::scalar(glsl_scalar(s)?)),
        Type::Vector(v) => Some(GlslType::vector(glsl_scalar(&v.scalar_ty)?, small(v.nscalar)?)),
        Type::Matrix(m) => {
            let scalar = glsl_scalar(&m.vector_ty.scalar_ty)?;
            if !matches!(scalar, ScalarKind::Float | ScalarKind::Double) {
                return None;
            }
            Some(GlslType { scalar, rows: small(m.vector_ty.nscalar)?, cols: small(m.nvector)?, array: None })
        }
        Type::Array(a) => {
            let n = a.nelement?;
            let inner = glsl_type(&a.element_ty)?;
            (inner.array.is_none()).then(|| inner.with_array(n))
        }
        _ => None,
    }
}

/// Bound for walking type chains: malformed modules can contain cycles.
const MAX_TYPE_DEPTH: u32 = 32;

/// First literal operand of a decoration.
fn deco_u32(info: &ModuleInfo, id: u32, deco: Decoration) -> Option<u32> {
    info.decoration(id, deco).and_then(|ops| ops.first().copied())
}

/// First literal operand of a member decoration.
fn member_deco_u32(info: &ModuleInfo, ty: u32, member: u32, deco: Decoration) -> Option<u32> {
    info.member_decoration(ty, member, deco).and_then(|ops| ops.first().copied())
}

/// Storage classes whose variables are descriptors in Vulkan.
fn is_descriptor_storage(storage: u32) -> bool {
    storage == spirv::StorageClass::UniformConstant as u32
        || storage == spirv::StorageClass::Uniform as u32
        || storage == spirv::StorageClass::StorageBuffer as u32
}

/// The descriptor variable at `(set, binding)`, preferring the one named `name`.
fn find_descriptor_var(info: &ModuleInfo, set: u32, binding: u32, name: Option<&str>) -> Option<u32> {
    let mut ids: Vec<u32> = info
        .variables
        .iter()
        .filter(|(id, (_, storage))| {
            is_descriptor_storage(*storage)
                && deco_u32(info, **id, Decoration::DescriptorSet).unwrap_or(0) == set
                && deco_u32(info, **id, Decoration::Binding).unwrap_or(0) == binding
        })
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable();
    ids.iter()
        .copied()
        .find(|id| name.is_some_and(|n| info.names.get(id).map(String::as_str) == Some(n)))
        .or_else(|| ids.first().copied())
}

/// Texel type of an image-typed descriptor variable (`image2D` -> `Float`,
/// `iimage2D` -> `Int`, `uimageBuffer` -> `Uint`); `Float` if it cannot be determined.
fn image_texel_kind(info: &ModuleInfo, var: u32) -> ScalarKind {
    let mut ty = info.variables.get(&var).and_then(|(ptr, _)| info.pointers.get(ptr)).map(|(_, pointee)| *pointee);
    for _ in 0..MAX_TYPE_DEPTH {
        let Some(t) = ty else { break };
        ty = match info.types.get(&t) {
            Some(TypeDef::Array { element, .. } | TypeDef::RuntimeArray { element }) => Some(*element),
            Some(TypeDef::SampledImage { image }) => Some(*image),
            Some(TypeDef::Image { sampled_type }) => {
                return match info.types.get(sampled_type) {
                    Some(TypeDef::Int { signed: true, .. }) => ScalarKind::Int,
                    Some(TypeDef::Int { signed: false, .. }) => ScalarKind::Uint,
                    Some(TypeDef::Float { width: 64 }) => ScalarKind::Double,
                    _ => ScalarKind::Float,
                };
            }
            _ => None,
        };
    }
    ScalarKind::Float
}

/// spirq silently skips descriptor variables whose type it cannot classify,
/// e.g. arrays of arrays of resources (which glslang emits for
/// `uniform sampler2D t[2][3]` although Vulkan forbids them). Missing a
/// resource would produce a wrong pipeline layout, so report it as an error.
fn check_all_descriptors_reported(info: &ModuleInfo, descriptors: &[Descriptor]) -> Result<(), String> {
    let mut ids: Vec<u32> =
        info.variables.iter().filter(|(_, (_, storage))| is_descriptor_storage(*storage)).map(|(id, _)| *id).collect();
    ids.sort_unstable();
    for id in ids {
        let set = deco_u32(info, id, Decoration::DescriptorSet).unwrap_or(0);
        let binding = deco_u32(info, id, Decoration::Binding).unwrap_or(0);
        let name = info.names.get(&id).map(String::as_str).unwrap_or_default();
        let reported = descriptors
            .iter()
            .any(|d| d.set == set && d.binding == binding && (name.is_empty() || d.name == name));
        if !reported {
            let label = if name.is_empty() { format!("%{id}") } else { format!("`{name}`") };
            return Err(format!(
                "descriptor {label} (set {set}, binding {binding}) has a type that cannot be reflected \
                 (Vulkan allows only one array level of resources)"
            ));
        }
    }
    Ok(())
}

/// GLSL spelling of a scalar type id.
fn scalar_type_name(info: &ModuleInfo, id: u32) -> &'static str {
    match info.types.get(&id) {
        Some(TypeDef::Bool) => "bool",
        Some(TypeDef::Float { width: 16 }) => "float16_t",
        Some(TypeDef::Float { width: 64 }) => "double",
        Some(TypeDef::Float { .. }) => "float",
        Some(TypeDef::Int { width: 8, signed: true }) => "int8_t",
        Some(TypeDef::Int { width: 8, signed: false }) => "uint8_t",
        Some(TypeDef::Int { width: 16, signed: true }) => "int16_t",
        Some(TypeDef::Int { width: 16, signed: false }) => "uint16_t",
        Some(TypeDef::Int { width: 64, signed: true }) => "int64_t",
        Some(TypeDef::Int { width: 64, signed: false }) => "uint64_t",
        Some(TypeDef::Int { signed: true, .. }) => "int",
        Some(TypeDef::Int { signed: false, .. }) => "uint",
        Some(TypeDef::Struct { .. }) => "struct",
        _ => "unknown",
    }
}

/// Number of locations a type occupies in a stage interface (64-bit 3- and
/// 4-component vectors take two).
fn type_locations(info: &ModuleInfo, id: u32, depth: u32) -> u64 {
    if depth > MAX_TYPE_DEPTH {
        return 1;
    }
    match info.types.get(&id) {
        Some(TypeDef::Vector { component, count }) => {
            let wide = matches!(
                info.types.get(component),
                Some(TypeDef::Float { width: 64 } | TypeDef::Int { width: 64, .. })
            );
            if wide && *count > 2 { 2 } else { 1 }
        }
        Some(TypeDef::Matrix { column, count }) => u64::from(*count).saturating_mul(type_locations(info, *column, depth + 1)),
        Some(TypeDef::Array { element, length }) => u64::from(info.const_u32(*length).unwrap_or(1))
            .saturating_mul(type_locations(info, *element, depth + 1)),
        Some(TypeDef::RuntimeArray { element }) => type_locations(info, *element, depth + 1),
        Some(TypeDef::Struct { members }) => {
            members.iter().fold(0u64, |acc, m| acc.saturating_add(type_locations(info, *m, depth + 1)))
        }
        _ => 1,
    }
}

/// Interpolation and auxiliary qualifiers of an interface variable or member.
#[derive(Debug, Clone, Copy, Default)]
struct Qualifiers {
    flat: bool,
    noperspective: bool,
    centroid: bool,
    sample: bool,
    patch: bool,
}

impl Qualifiers {
    fn of(has: impl Fn(Decoration) -> bool) -> Self {
        Self {
            flat: has(Decoration::Flat),
            noperspective: has(Decoration::NoPerspective),
            centroid: has(Decoration::Centroid),
            sample: has(Decoration::Sample),
            patch: has(Decoration::Patch),
        }
    }

    fn union(self, o: Self) -> Self {
        Self {
            flat: self.flat || o.flat,
            noperspective: self.noperspective || o.noperspective,
            centroid: self.centroid || o.centroid,
            sample: self.sample || o.sample,
            patch: self.patch || o.patch,
        }
    }
}

/// Where an interface entry sits and how it is qualified.
struct Slot {
    name: String,
    location: u32,
    component: u32,
    index: u32,
    per_vertex: bool,
    qualifiers: Qualifiers,
}

/// Describe a value of type `ty` (per-vertex array already removed).
fn make_interface_var(info: &ModuleInfo, ty: u32, slot: Slot) -> InterfaceVar {
    let location_count = clamp_u32(type_locations(info, ty, 0));
    let mut array_len: Option<u32> = None;
    let mut elem = ty;
    for _ in 0..MAX_TYPE_DEPTH {
        let n = match info.types.get(&elem) {
            Some(TypeDef::Array { element, length }) => {
                elem = *element;
                info.const_u32(*length).unwrap_or(0)
            }
            Some(TypeDef::RuntimeArray { element }) => {
                elem = *element;
                0
            }
            _ => break,
        };
        array_len = Some(array_len.map_or(n, |len| len.saturating_mul(n)));
    }
    let (base_type, vec_size, columns) = match info.types.get(&elem) {
        Some(TypeDef::Vector { component, count }) => (scalar_type_name(info, *component), *count, 1),
        Some(TypeDef::Matrix { column, count }) => match info.types.get(column) {
            Some(TypeDef::Vector { component, count: rows }) => (scalar_type_name(info, *component), *rows, *count),
            _ => ("unknown", 1, *count),
        },
        _ => (scalar_type_name(info, elem), 1, 1),
    };
    let q = slot.qualifiers;
    InterfaceVar {
        location: slot.location,
        component: slot.component,
        index: slot.index,
        name: slot.name,
        base_type: base_type.to_string(),
        vec_size,
        columns,
        array_len,
        per_vertex: slot.per_vertex,
        location_count,
        flat: q.flat,
        noperspective: q.noperspective,
        centroid: q.centroid,
        sample: q.sample,
        patch: q.patch,
    }
}

/// User inputs (`is_input`) or outputs of the entry point, from the module scan.
///
/// Every declared variable of the storage class is reported (used or not), like
/// descriptors; when the module has several entry points, only those listed in
/// this entry point's interface. Built-ins (variables and `gl_PerVertex`-style
/// blocks) are skipped. Blocks and struct variables are expanded per member.
fn interface_vars(info: &ModuleInfo, stage: ShaderStage, interface: &[u32], is_input: bool) -> Vec<InterfaceVar> {
    let storage = if is_input { spirv::StorageClass::Input } else { spirv::StorageClass::Output } as u32;
    let mut ids: Vec<u32> = if info.entry_points.len() <= 1 {
        info.variables.iter().filter(|(_, (_, sc))| *sc == storage).map(|(id, _)| *id).collect()
    } else {
        interface.iter().copied().filter(|id| info.variables.get(id).is_some_and(|(_, sc)| *sc == storage)).collect()
    };
    ids.sort_unstable();
    ids.dedup();

    let mut out = Vec::new();
    for id in ids {
        if info.has_decoration(id, Decoration::BuiltIn) {
            continue;
        }
        let Some(mut ty) = info.variables.get(&id).and_then(|(ptr, _)| info.pointers.get(ptr)).map(|(_, p)| *p) else {
            continue;
        };
        let var_q = Qualifiers::of(|d| info.has_decoration(id, d));
        let per_vertex = !var_q.patch
            && match stage {
                ShaderStage::Geometry | ShaderStage::TessEval => is_input,
                ShaderStage::TessControl => true,
                _ => false,
            };
        if per_vertex
            && let Some(TypeDef::Array { element, .. } | TypeDef::RuntimeArray { element }) = info.types.get(&ty)
        {
            ty = *element;
        }
        let name = info.names.get(&id).cloned().unwrap_or_default();
        let location = deco_u32(info, id, Decoration::Location);

        if let Some(TypeDef::Struct { members }) = info.types.get(&ty) {
            let count = u32::try_from(members.len()).unwrap_or(u32::MAX);
            if (0..count).any(|m| info.member_decoration(ty, m, Decoration::BuiltIn).is_some()) {
                continue; // gl_PerVertex / gl_in[] / gl_out[]
            }
            let prefix = if name.is_empty() { info.names.get(&ty).cloned().unwrap_or_default() } else { name };
            let mut cursor = location;
            for (m, &member_ty) in (0..count).zip(members) {
                let Some(loc) = member_deco_u32(info, ty, m, Decoration::Location).or(cursor) else { continue };
                let member_name = match info.member_names.get(&(ty, m)) {
                    Some(n) if !n.is_empty() && !prefix.is_empty() => format!("{prefix}.{n}"),
                    Some(n) => n.clone(),
                    None => String::new(),
                };
                let qualifiers = var_q.union(Qualifiers::of(|d| info.member_decoration(ty, m, d).is_some()));
                let v = make_interface_var(
                    info,
                    member_ty,
                    Slot {
                        name: member_name,
                        location: loc,
                        component: member_deco_u32(info, ty, m, Decoration::Component).unwrap_or(0),
                        index: 0,
                        per_vertex,
                        qualifiers,
                    },
                );
                cursor = Some(loc.saturating_add(v.location_count));
                out.push(v);
            }
            continue;
        }

        let Some(location) = location else { continue };
        out.push(make_interface_var(
            info,
            ty,
            Slot {
                name,
                location,
                component: deco_u32(info, id, Decoration::Component).unwrap_or(0),
                index: deco_u32(info, id, Decoration::Index).unwrap_or(0),
                per_vertex,
                qualifiers: var_q,
            },
        ));
    }
    out.sort_by_key(|v| (v.location, v.component, v.index));
    out
}

fn builtin_name(v: u32) -> String {
    spirv::BuiltIn::from_u32(v).map_or_else(|| format!("BuiltIn({v})"), |b| format!("{b:?}"))
}

fn capability_name(v: u32) -> String {
    spirv::Capability::from_u32(v).map_or_else(|| format!("Capability({v})"), |c| format!("{c:?}"))
}

/// Built-in inputs and outputs of the entry point interface.
fn builtins(info: &ModuleInfo, interface: &[u32]) -> (Vec<String>, Vec<String>) {
    let mut ins = Vec::new();
    let mut outs = Vec::new();
    for id in interface {
        let Some(&(ptr_ty, sc)) = info.variables.get(id) else { continue };
        let list = if sc == spirv::StorageClass::Input as u32 {
            &mut ins
        } else if sc == spirv::StorageClass::Output as u32 {
            &mut outs
        } else {
            continue;
        };
        if let Some(ops) = info.decoration(*id, Decoration::BuiltIn) {
            if let Some(&b) = ops.first() {
                list.push(builtin_name(b));
            }
            continue;
        }
        // Built-in blocks (gl_PerVertex, gl_in[]): member decorations on the struct.
        let mut ty = info.pointers.get(&ptr_ty).map(|(_, pointee)| *pointee);
        while let Some(t) = ty {
            if let Some(&elem) = info.arrays.get(&t) {
                ty = Some(elem);
                continue;
            }
            if let Some(members) = info.structs.get(&t) {
                for m in 0..members.len() as u32 {
                    if let Some(&b) = info.member_decoration(t, m, Decoration::BuiltIn).and_then(|o| o.first()) {
                        list.push(builtin_name(b));
                    }
                }
            }
            break;
        }
    }
    ins.sort();
    ins.dedup();
    outs.sort();
    outs.dedup();
    (ins, outs)
}

fn execution_modes(info: &ModuleInfo, func: u32) -> Vec<ExecutionModeInfo> {
    info.exec_modes
        .iter()
        .filter(|(f, ..)| *f == func)
        .map(|(_, mode, operands, ids)| ExecutionModeInfo {
            mode: spirv::ExecutionMode::from_u32(*mode).map_or_else(|| format!("ExecutionMode({mode})"), |m| format!("{m:?}")),
            operands: if *ids {
                operands.iter().map(|id| info.const_u32(*id).unwrap_or(*id)).collect()
            } else {
                operands.clone()
            },
        })
        .collect()
}

/// Compute work-group size: a `WorkgroupSize` built-in constant overrides the
/// `LocalSize` / `LocalSizeId` execution modes (SPIR-V spec, BuiltIn WorkgroupSize).
fn local_size(info: &ModuleInfo, func: u32) -> Option<[u32; 3]> {
    let three = |v: &[u32]| -> Option<[u32; 3]> { Some([*v.first()?, *v.get(1)?, *v.get(2)?]) };
    let builtin = info.composites.iter().find_map(|(id, parts)| {
        let b = info.decoration(*id, Decoration::BuiltIn)?.first().copied()?;
        if b != spirv::BuiltIn::WorkgroupSize as u32 {
            return None;
        }
        let values: Option<Vec<u32>> = parts.iter().map(|p| info.const_u32(*p)).collect();
        three(&values?)
    });
    if builtin.is_some() {
        return builtin;
    }
    info.exec_modes.iter().filter(|(f, ..)| *f == func).find_map(|(_, mode, ops, ids)| {
        if *mode == spirv::ExecutionMode::LocalSize as u32 && !ids {
            three(ops)
        } else if *mode == spirv::ExecutionMode::LocalSizeId as u32 {
            let values: Option<Vec<u32>> = ops.iter().map(|id| info.const_u32(*id)).collect();
            three(&values?)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage() {
        assert!(reflect(&[]).is_err());
        assert!(reflect(&[0; 5]).is_err());
        assert!(reflect(&[module::MAGIC, 0x0001_0500, 0, 1, 0]).unwrap_err().contains("entry point"));
        // Truncated instruction.
        assert!(reflect(&[module::MAGIC, 0x0001_0500, 0, 1, 0, (4 << 16) | 17, 1]).is_err());
        // Unknown opcode.
        assert!(reflect(&[module::MAGIC, 0x0001_0500, 0, 1, 0, (1 << 16) | 0xfff0]).unwrap_err().contains("opcode"));
    }

    #[test]
    fn core_dim_spelling() {
        let k = DescriptorKind::CombinedImageSampler {
            dim: ImageDim::D2,
            arrayed: true,
            shadow: false,
            multisampled: false,
            sample_type: ScalarKind::Float,
        };
        assert_eq!(k.core_dim().as_deref(), Some("2d_array"));
        assert_eq!(k.vk_descriptor_type(), 1);
        let k = DescriptorKind::UniformTexelBuffer { sample_type: ScalarKind::Float };
        assert_eq!(k.core_dim().as_deref(), Some("buffer"));
        let k = DescriptorKind::UniformBuffer { size: 16, members: vec![] };
        assert_eq!(k.core_dim(), None);
        assert_eq!(k.vk_descriptor_type(), 6);
    }

    #[test]
    fn image_format_names_cover_core_formats() {
        // Every format sb-core can emit as an image qualifier maps back.
        for f in sb_core::TextureFormat::ALL {
            let Some(q) = f.glsl_image_format() else { continue };
            let found = (0..=41).filter_map(ImageFormat::from_u32).any(|i| image_format_qualifier(i) == Some(q));
            assert!(found, "{q}");
        }
    }

    #[test]
    fn interface_type_names() {
        let mut v = InterfaceVar {
            location: 0,
            component: 0,
            index: 0,
            name: "x".into(),
            base_type: "uint".into(),
            vec_size: 3,
            columns: 1,
            array_len: None,
            per_vertex: false,
            location_count: 1,
            flat: true,
            noperspective: false,
            centroid: false,
            sample: false,
            patch: false,
        };
        assert_eq!(v.glsl_type_name(), "uvec3");
        v.base_type = "float".into();
        v.vec_size = 4;
        v.columns = 4;
        assert_eq!(v.glsl_type_name(), "mat4");
        v.columns = 3;
        v.array_len = Some(2);
        assert_eq!(v.glsl_type_name(), "mat3x4[2]");
    }
}
