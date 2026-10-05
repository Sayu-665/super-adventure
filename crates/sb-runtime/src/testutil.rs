//! Helpers for unit tests.

use indexmap::IndexMap;
use sb_core::model::*;
use sb_core::program::GeometryProgram;
use sb_core::{GlslType, PassGroup, ShaderStage, TextureFormat};

/// A dimension without programs.
pub(crate) fn empty_dimension() -> DimensionPipeline {
    DimensionPipeline {
        folder: String::new(),
        dimension_ids: Vec::new(),
        targets: RenderTargets {
            colortex: Vec::new(),
            shadowcolor: Vec::new(),
            shadow: ShadowSettings::default(),
            uses_depthtex1: false,
            uses_depthtex2: false,
            noise_texture_resolution: 256,
            noise_texture: None,
            custom_textures: Vec::new(),
            images: Vec::new(),
            buffers: Vec::new(),
        },
        settings: PackSettings::default(),
        uniforms: UniformLayout::default(),
        custom_uniforms: Vec::new(),
        bindings: BindingTable::default(),
        programs: Vec::new(),
        geometry: Default::default(),
        passes: Vec::new(),
        gbuffer_attachments: Vec::new(),
        shadow_attachments: Vec::new(),
        end_of_frame_copies: Vec::new(),
        distant_horizons: DhPipeline::default(),
    }
}

fn compile(src: &str, stage: ShaderStage) -> Vec<u32> {
    sb_compile::compile_glsl(src, stage, "unit", &sb_compile::CompileOptions::default(), None).unwrap_or_else(|e| panic!("{e}\n{}\n{src}", e.log))
}

fn program(blobs: &mut BlobTable, name: &str, kind: ProgramKind, profile: Option<&str>, stages: &[(ShaderStage, &str)], draw_buffers: Vec<u32>, compute: Option<ComputeInfo>) -> Program {
    let stages = stages
        .iter()
        .map(|(stage, src)| StageModule {
            stage: *stage,
            entry_point: "main".into(),
            spirv: Some(blobs.push_spirv(&compile(src, *stage))),
            glsl_vulkan: None,
            glsl_renderpearl: None,
            source_file: format!("{name}.{}", stage.pack_extension()),
        })
        .collect();
    Program {
        name: name.into(),
        kind,
        draw_profile: profile.map(str::to_string),
        requires_raw_vulkan: compute.is_some(),
        stages,
        output_slots: (0..draw_buffers.len() as u32).collect(),
        output_types: draw_buffers.iter().map(|_| "float".to_string()).collect(),
        draw_buffers,
        blend: None,
        blend_per_buffer: IndexMap::new(),
        alpha_test: None,
        viewport: ViewportScale::default(),
        mipmap_targets: Vec::new(),
        bindings_used: Vec::new(),
        vertex_inputs: Vec::new(),
        push_constant_size: 0,
        compute,
        cull: None,
        synthesized_from: None,
    }
}

const FRAME: &str = "layout(std140, set = 0, binding = 0) uniform sb_Frame { layout(offset = 0) mat4 gbufferModelView; layout(offset = 64) mat4 gbufferProjection; };\n";

/// A small pack exercising every kind of Vulkan object the runtime creates: a terrain
/// program (also the shadow program), a compute pass writing an SSBO and a `final`
/// pass sampling colortex0, with shadows and a uniform block.
pub(crate) fn mini_pack() -> (CompiledPack, BlobTable) {
    let mut blobs = BlobTable::default();
    let terrain_vsh = format!(
        "#version 460\n{FRAME}{}",
        "layout(std140, set = 0, binding = 2) uniform Globals { ivec3 CameraBlockPos; float GlintAlpha; vec3 CameraOffset; } g;\n\
         layout(location = 0) in vec3 Position;\nlayout(location = 2) in vec2 UV0;\nlayout(location = 4) in ivec3 ChunkPosition;\n\
         layout(location = 0) out vec2 uv;\n\
         void main() { vec3 p = Position + vec3(ChunkPosition - g.CameraBlockPos) + g.CameraOffset; uv = UV0;\n\
         gl_Position = gbufferProjection * (gbufferModelView * vec4(p, 1.0)); gl_Position.z = 0.5 * (gl_Position.z + gl_Position.w); }\n"
    );
    let terrain_fsh = "#version 460\nlayout(set = 1, binding = 0) uniform sampler2D gtexture;\nlayout(location = 0) in vec2 uv;\nlayout(location = 0) out vec4 o;\nvoid main() { o = texture(gtexture, uv); }\n";
    let fullscreen_vsh = "#version 460\nlayout(location = 0) out vec2 uv;\nvoid main() { int i = gl_VertexIndex % 6; uv = vec2(float(i == 1 || i == 2 || i == 4), float(i == 2 || i == 4 || i == 5)); gl_Position = vec4(uv * 2.0 - 1.0, 0.5, 1.0); }\n";
    let final_fsh = "#version 460\nlayout(set = 1, binding = 1) uniform sampler2D colortex0;\nlayout(std430, set = 2, binding = 0) readonly buffer B { vec4 v[]; } b;\nlayout(location = 0) in vec2 uv;\nlayout(location = 0) out vec4 o;\nvoid main() { o = texture(colortex0, uv) * b.v[0]; }\n";
    let csh = "#version 460\nlayout(local_size_x = 1) in;\nlayout(std430, set = 2, binding = 0) buffer B { vec4 v[]; } b;\nvoid main() { b.v[0] = vec4(1.0); }\n";
    let geo = |p| ProgramKind::Geometry { program: p };
    let programs = vec![
        program(&mut blobs, "gbuffers_terrain", geo(GeometryProgram::Terrain), Some("vanilla_terrain"), &[(ShaderStage::Vertex, &terrain_vsh), (ShaderStage::Fragment, terrain_fsh)], vec![0], None),
        program(&mut blobs, "composite_a", ProgramKind::Compute { group: PassGroup::Composite, index: 0, letter: Some('a') }, None, &[(ShaderStage::Compute, csh)], vec![], Some(ComputeInfo { local_size: [1, 1, 1], work_groups: WorkGroups::Absolute { x: 1, y: 1, z: 1 }, indirect: None })),
        program(&mut blobs, "final", ProgramKind::Composite { group: PassGroup::Final, index: 0 }, Some("fullscreen"), &[(ShaderStage::Vertex, fullscreen_vsh), (ShaderStage::Fragment, final_fsh)], vec![0], None),
    ];
    let mut dim = empty_dimension();
    dim.folder = "world0".into();
    dim.programs = programs;
    for g in [GeometryProgram::Terrain, GeometryProgram::TerrainSolid, GeometryProgram::TerrainCutout, GeometryProgram::Water, GeometryProgram::Shadow, GeometryProgram::ShadowSolid, GeometryProgram::ShadowCutout] {
        dim.geometry.insert(g, GeometrySlot { program: 0, resolved_from: GeometryProgram::Terrain, variants: Default::default() });
    }
    let sampler = ResourceKind::Sampler { dim: "2d".into(), shadow: false, sample_type: "float".into() };
    dim.bindings.entries = vec![
        BindingEntry { name: "gtexture".into(), set: 1, binding: 0, kind: sampler.clone(), resource: ResourceRef::Atlas },
        BindingEntry { name: "colortex0".into(), set: 1, binding: 1, kind: sampler, resource: ResourceRef::ColorTex(0) },
        BindingEntry { name: "B".into(), set: 2, binding: 0, kind: ResourceKind::StorageBuffer, resource: ResourceRef::Ssbo(0) },
        BindingEntry { name: "Globals".into(), set: 0, binding: 2, kind: ResourceKind::UniformBuffer, resource: ResourceRef::UniformBlock("Globals".into()) },
    ];
    let member = |name: &str, offset| BlockMember { name: name.into(), ty: GlslType::MAT4, offset, source: UniformSource::Builtin(name.into()), default: None };
    dim.uniforms.frame = BlockLayout { name: "sb_Frame".into(), set: 0, binding: 0, size: 128, members: vec![member("gbufferModelView", 0), member("gbufferProjection", 64)] };
    dim.targets.colortex = vec![ColorTarget { index: 0, format: TextureFormat::RGBA16F, clear: true, clear_color: None, mipmap_programs: Vec::new(), size: TargetSize::default(), used: true }];
    dim.targets.shadow = ShadowSettings { enabled: true, resolution: 64, ..Default::default() };
    dim.targets.buffers = vec![StorageBuffer { index: 0, size: 64, relative: None, file: None }];
    dim.gbuffer_attachments = vec![0];
    let pass = |group, program, computes| Pass { group, index: 0, computes, program, flips_after: Vec::new(), flip_state: Vec::new() };
    dim.passes = vec![
        pass(PassGroup::Shadow, None, vec![]),
        pass(PassGroup::GbuffersOpaque, None, vec![]),
        pass(PassGroup::Composite, None, vec![1]),
        pass(PassGroup::Final, Some(2), vec![]),
    ];
    let pack = CompiledPack {
        format_version: sb_core::MODEL_FORMAT_VERSION,
        info: PackInfo {
            name: "unit".into(),
            source_hash: String::new(),
            shaderbridge_version: sb_core::SHADERBRIDGE_VERSION.into(),
            features_enabled: Vec::new(),
            features_unsupported: Vec::new(),
            environment: CompileEnvironment::default(),
        },
        options: OptionsModel::default(),
        id_maps: IdMaps::default(),
        dimensions: vec![dim],
        diagnostics: Default::default(),
        blobs: Vec::new(),
    };
    (pack, blobs)
}
