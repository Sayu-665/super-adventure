//! Shared helpers of the integration tests: a hand-assembled `CompiledPack` following the
//! model contract exactly (bindings in sets 0/1/2, `sb_Frame`/`sb_Draw`, profile host
//! blocks, shared gbuffer attachments, static flip schedule), compiled with sb-compile.
#![allow(dead_code)]

pub mod shaders;

use indexmap::IndexMap;
use sb_compile::{CompileOptions, compile_glsl};
use sb_core::model::*;
use sb_core::program::{AlphaFunc, BlendMode, GeometryProgram};
use sb_core::{GlslType, PassGroup, ShaderStage, TextureFormat};
use sb_runtime::{NoTextures, RenderOutput, RenderRequest, Runtime, RuntimeError, RuntimeOptions, SceneParams};
use std::sync::Mutex;

/// Renders are serialized: lavapipe already uses every core.
pub static GPU_LOCK: Mutex<()> = Mutex::new(());

/// Variants of the test pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variant {
    pub depth_mode: DepthMode,
    /// DH strategy: Native (separate DH depth, the pack reads dhDepthTex0) or
    /// Synthesized (unified projection, shared depth).
    pub native_dh: bool,
    /// Leave the DH programs out of the geometry map.
    pub without_dh_programs: bool,
}

impl Default for Variant {
    fn default() -> Self {
        Self { depth_mode: DepthMode::ForwardZeroToOne, native_dh: false, without_dh_programs: false }
    }
}

fn compile(src: &str, stage: ShaderStage, defines: &[&str]) -> Vec<u32> {
    // Insert the defines right after `#version`.
    let mut lines = src.lines();
    let version = lines.next().unwrap_or("#version 460");
    let rest: Vec<&str> = lines.collect();
    let mut full = String::from(version);
    full.push('\n');
    for d in defines {
        full.push_str(&format!("#define {d}\n"));
    }
    full.push_str(&rest.join("\n"));
    compile_glsl(&full, stage, "test", &CompileOptions::default(), None).unwrap_or_else(|e| panic!("{e}\n{}\n{full}", e.log))
}

fn member(name: &str, ty: GlslType, offset: u32, source: UniformSource) -> BlockMember {
    BlockMember { name: name.into(), ty, offset, source, default: None }
}

fn builtin(name: &str) -> UniformSource {
    UniformSource::Builtin(name.into())
}

pub fn frame_layout() -> BlockLayout {
    let mut members = vec![
        member("gbufferModelView", GlslType::MAT4, 0, builtin("gbufferModelView")),
        member("gbufferProjection", GlslType::MAT4, 64, builtin("gbufferProjection")),
        member("gbufferProjectionInverse", GlslType::MAT4, 128, builtin("gbufferProjectionInverse")),
        member("gbufferModelViewInverse", GlslType::MAT4, 192, builtin("gbufferModelViewInverse")),
        member("shadowModelView", GlslType::MAT4, 256, builtin("shadowModelView")),
        member("shadowProjection", GlslType::MAT4, 320, builtin("shadowProjection")),
        member("dhProjection", GlslType::MAT4, 384, builtin("dhProjection")),
        member("dhProjectionInverse", GlslType::MAT4, 448, builtin("dhProjectionInverse")),
        member("sunPosition", GlslType::VEC3, 512, builtin("sunPosition")),
        member("frameTimeCounter", GlslType::FLOAT, 524, builtin("frameTimeCounter")),
        member("fogColor", GlslType::VEC3, 528, builtin("fogColor")),
        member("rainStrength", GlslType::FLOAT, 540, builtin("rainStrength")),
        member("skyColor", GlslType::VEC3, 544, builtin("skyColor")),
        member("viewWidth", GlslType::FLOAT, 556, builtin("viewWidth")),
        member("viewHeight", GlslType::FLOAT, 560, builtin("viewHeight")),
        member("myCustom", GlslType::FLOAT, 564, UniformSource::Custom("myCustom".into())),
        member("testDefault", GlslType::FLOAT, 568, UniformSource::Unset),
        member("near", GlslType::FLOAT, 572, builtin("near")),
        member("far", GlslType::FLOAT, 576, builtin("far")),
        member("dhFarPlane", GlslType::FLOAT, 580, builtin("dhFarPlane")),
    ];
    members[16].default = Some(vec![0.75]);
    BlockLayout { name: "sb_Frame".into(), set: 0, binding: 0, size: 592, members }
}

pub fn draw_layout() -> BlockLayout {
    BlockLayout {
        name: "sb_Draw".into(),
        set: 0,
        binding: 1,
        size: 80,
        members: vec![
            member("alphaTestRef", GlslType::FLOAT, 0, builtin("alphaTestRef")),
            member("entityId", GlslType::INT, 4, builtin("entityId")),
            member("projectionMatrix", GlslType::MAT4, 16, builtin("projectionMatrix")),
        ],
    }
}

fn sampler(dim: &str, shadow: bool) -> ResourceKind {
    ResourceKind::Sampler { dim: dim.into(), shadow, sample_type: "float".into() }
}

pub fn binding_table() -> BindingTable {
    let s = |name: &str, binding: u32, resource: ResourceRef| BindingEntry { name: name.into(), set: 1, binding, kind: sampler("2d", false), resource };
    let ubo = |name: &str, binding: u32| BindingEntry { name: name.into(), set: 0, binding, kind: ResourceKind::UniformBuffer, resource: ResourceRef::UniformBlock(name.into()) };
    BindingTable {
        entries: vec![
            ubo("Globals", 2),
            ubo("TerrainUniform", 3),
            ubo("DynamicTransforms", 4),
            ubo("vertUniqueUniformBlock", 5),
            ubo("vertSharedUniformBlock", 6),
            s("gtexture", 0, ResourceRef::Atlas),
            s("lightmap", 1, ResourceRef::Lightmap),
            s("colortex0", 2, ResourceRef::ColorTex(0)),
            s("colortex1", 3, ResourceRef::ColorTex(1)),
            s("colortex2", 4, ResourceRef::ColorTex(2)),
            s("depthtex0", 5, ResourceRef::DepthTex(0)),
            s("shadowtex0", 6, ResourceRef::ShadowTex(0)),
            s("noisetex", 7, ResourceRef::Noise),
            s("depthtex1", 8, ResourceRef::DepthTex(1)),
            s("dhDepthTex0", 9, ResourceRef::DhDepthTex(0)),
            s("dhBlockAtlas", 10, ResourceRef::DhBlockAtlas),
            BindingEntry { name: "shadowtex1".into(), set: 1, binding: 11, kind: sampler("2d", true), resource: ResourceRef::ShadowTex(1) },
            s("shadowcolor0", 12, ResourceRef::ShadowColor(0)),
            s("iris_overlay", 13, ResourceRef::Overlay),
            BindingEntry { name: "SbBuf".into(), set: 2, binding: 0, kind: ResourceKind::StorageBuffer, resource: ResourceRef::Ssbo(0) },
        ],
    }
}

fn color_target(index: u32, format: TextureFormat) -> ColorTarget {
    ColorTarget { index, format, clear: true, clear_color: None, mipmap_programs: Vec::new(), size: TargetSize::default(), used: true }
}

struct ProgramSpec {
    name: &'static str,
    kind: ProgramKind,
    profile: Option<&'static str>,
    stages: Vec<(ShaderStage, String)>,
    draw_buffers: Vec<u32>,
    blend: Option<BlendMode>,
    alpha_test: Option<AlphaTest>,
    uses: Vec<(&'static str, bool)>,
    compute: Option<ComputeInfo>,
}

fn program(blobs: &mut BlobTable, spec: ProgramSpec, defines: &[&str], table: &BindingTable) -> Program {
    let stages = spec
        .stages
        .iter()
        .map(|(stage, src)| {
            let words = compile(src, *stage, defines);
            StageModule {
                stage: *stage,
                entry_point: "main".into(),
                spirv: Some(blobs.push_spirv(&words)),
                glsl_vulkan: Some(blobs.push_glsl(src.clone())),
                glsl_renderpearl: None,
                source_file: format!("{}.{}", spec.name, stage.pack_extension()),
            }
        })
        .collect();
    let bindings_used = spec
        .uses
        .iter()
        .filter_map(|(n, alt)| table.get(n).map(|e| BindingUse { name: e.name.clone(), set: e.set, binding: e.binding, use_alt: *alt, stages: vec![ShaderStage::Fragment], shadow_emulated: false }))
        .collect();
    Program {
        name: spec.name.into(),
        kind: spec.kind,
        draw_profile: spec.profile.map(str::to_string),
        requires_raw_vulkan: spec.compute.is_some(),
        stages,
        output_slots: (0..spec.draw_buffers.len() as u32).collect(),
        output_types: spec.draw_buffers.iter().map(|_| "float".to_string()).collect(),
        draw_buffers: spec.draw_buffers,
        blend: spec.blend,
        blend_per_buffer: IndexMap::new(),
        alpha_test: spec.alpha_test,
        viewport: ViewportScale::default(),
        mipmap_targets: Vec::new(),
        bindings_used,
        vertex_inputs: Vec::new(),
        push_constant_size: 0,
        compute: spec.compute,
        cull: None,
        synthesized_from: None,
        inherit_blend: false,
    }
}

/// Build the test pack.
pub fn test_pack(v: Variant) -> (CompiledPack, BlobTable) {
    use shaders::*;
    let mut defines = Vec::new();
    if v.depth_mode == DepthMode::ReversedZeroToOne {
        defines.push("SB_REVERSED");
    }
    if v.native_dh {
        defines.push("SB_NATIVE_DH");
    }
    let table = binding_table();
    let mut blobs = BlobTable::default();
    let geo = |p: GeometryProgram| ProgramKind::Geometry { program: p };
    let gb = vec![0, 1, 2];
    let alpha = Some(AlphaTest { func: AlphaFunc::Greater, reference: 0.1 });
    let specs = vec![
        ProgramSpec { name: "gbuffers_terrain", kind: geo(GeometryProgram::Terrain), profile: Some("vanilla_terrain"), stages: vec![(ShaderStage::Vertex, terrain_vsh()), (ShaderStage::Fragment, terrain_fsh())], draw_buffers: gb.clone(), blend: None, alpha_test: alpha, uses: vec![("gtexture", false), ("lightmap", false)], compute: None },
        ProgramSpec { name: "gbuffers_entities", kind: geo(GeometryProgram::Entities), profile: Some("vanilla_entity"), stages: vec![(ShaderStage::Vertex, entity_vsh()), (ShaderStage::Fragment, terrain_fsh())], draw_buffers: gb.clone(), blend: None, alpha_test: alpha, uses: vec![("gtexture", false)], compute: None },
        ProgramSpec { name: "gbuffers_skybasic", kind: geo(GeometryProgram::SkyBasic), profile: Some("vanilla_position"), stages: vec![(ShaderStage::Vertex, sky_vsh()), (ShaderStage::Fragment, sky_fsh())], draw_buffers: vec![0, 1], blend: None, alpha_test: None, uses: vec![], compute: None },
        ProgramSpec { name: "dh_terrain", kind: geo(GeometryProgram::DhTerrain), profile: Some("dh_terrain"), stages: vec![(ShaderStage::Vertex, dh_vsh()), (ShaderStage::Fragment, dh_fsh())], draw_buffers: gb.clone(), blend: None, alpha_test: None, uses: vec![], compute: None },
        ProgramSpec { name: "gbuffers_water", kind: geo(GeometryProgram::Water), profile: Some("vanilla_terrain"), stages: vec![(ShaderStage::Vertex, terrain_vsh()), (ShaderStage::Fragment, water_fsh())], draw_buffers: gb.clone(), blend: Some(BlendMode::TRANSLUCENT), alpha_test: None, uses: vec![], compute: None },
        ProgramSpec { name: "shadow", kind: geo(GeometryProgram::Shadow), profile: Some("vanilla_terrain"), stages: vec![(ShaderStage::Vertex, shadow_vsh()), (ShaderStage::Fragment, shadow_fsh())], draw_buffers: vec![0], blend: None, alpha_test: alpha, uses: vec![], compute: None },
        ProgramSpec {
            name: "composite",
            kind: ProgramKind::Composite { group: PassGroup::Composite, index: 0 },
            profile: Some("fullscreen"),
            stages: vec![(ShaderStage::Vertex, fullscreen_vsh()), (ShaderStage::Fragment, composite_fsh())],
            draw_buffers: vec![0],
            blend: None,
            alpha_test: None,
            uses: vec![("colortex0", false), ("colortex1", false), ("colortex2", false)],
            compute: None,
        },
        ProgramSpec {
            name: "composite_a",
            kind: ProgramKind::Compute { group: PassGroup::Composite, index: 0, letter: Some('a') },
            profile: None,
            stages: vec![(ShaderStage::Compute, compute_csh())],
            draw_buffers: vec![],
            blend: None,
            alpha_test: None,
            uses: vec![],
            compute: Some(ComputeInfo { local_size: [8, 8, 1], work_groups: WorkGroups::Relative { x: 0.5, y: 0.5 }, indirect: None }),
        },
        ProgramSpec { name: "final", kind: ProgramKind::Composite { group: PassGroup::Final, index: 0 }, profile: Some("fullscreen"), stages: vec![(ShaderStage::Vertex, fullscreen_vsh()), (ShaderStage::Fragment, final_fsh())], draw_buffers: vec![0], blend: None, alpha_test: None, uses: vec![("colortex0", true)], compute: None },
    ];
    let programs: Vec<Program> = specs.into_iter().map(|s| program(&mut blobs, s, &defines, &table)).collect();
    let mut geometry = IndexMap::new();
    let mut slot = |g: GeometryProgram, program: u32, from: GeometryProgram| {
        geometry.insert(g, GeometrySlot::new(program, from, g));
    };
    use GeometryProgram as G;
    for g in [G::Terrain, G::TerrainSolid, G::TerrainCutout, G::Block, G::DamagedBlock] {
        slot(g, 0, G::Terrain);
    }
    slot(G::Entities, 1, G::Entities);
    slot(G::SkyBasic, 2, G::SkyBasic);
    if !v.without_dh_programs {
        slot(G::DhTerrain, 3, G::DhTerrain);
        slot(G::DhWater, 3, G::DhTerrain);
    }
    slot(G::Water, 4, G::Water);
    for g in [G::Shadow, G::ShadowSolid, G::ShadowCutout, G::ShadowWater] {
        slot(g, 5, G::Shadow);
    }
    let pass = |group, index, computes: Vec<u32>, program, flips_after: Vec<u32>, flip0: bool| {
        let mut flip_state = vec![false; 3];
        flip_state[0] = flip0;
        Pass { group, index, computes, program, flips_after, flip_state }
    };
    let passes = vec![
        pass(PassGroup::Shadow, 0, vec![], None, vec![], false),
        pass(PassGroup::GbuffersOpaque, 0, vec![], None, vec![], false),
        pass(PassGroup::GbuffersTranslucent, 0, vec![], None, vec![], false),
        pass(PassGroup::Composite, 0, vec![7], Some(6), vec![0], false),
        pass(PassGroup::Final, 0, vec![], Some(8), vec![], true),
    ];
    let shadow = ShadowSettings { enabled: true, resolution: 512, distance: 64.0, ..Default::default() };
    let dim = DimensionPipeline {
        folder: "world0".into(),
        dimension_ids: vec!["minecraft:overworld".into(), "*".into()],
        targets: RenderTargets {
            colortex: vec![color_target(0, TextureFormat::RGBA16F), color_target(1, TextureFormat::RGBA8), color_target(2, TextureFormat::RGBA8)],
            shadowcolor: vec![color_target(0, TextureFormat::RGBA8)],
            shadow,
            uses_depthtex1: true,
            uses_depthtex2: false,
            noise_texture_resolution: 64,
            noise_texture: None,
            custom_textures: Vec::new(),
            images: Vec::new(),
            buffers: vec![StorageBuffer { index: 0, size: 256, relative: None, file: None }],
        },
        settings: PackSettings { sun_path_rotation: 20.0, ..Default::default() },
        uniforms: UniformLayout { frame: frame_layout(), draw: draw_layout() },
        custom_uniforms: vec![CustomUniform { name: "myCustom".into(), ty: GlslType::FLOAT, expression: "if(rainStrength > 0.5, 0.0, 1.0)".into(), is_variable: false, location: None }],
        bindings: table,
        programs,
        geometry,
        passes,
        gbuffer_attachments: vec![0, 1, 2],
        shadow_attachments: vec![0],
        end_of_frame_copies: vec![],
        distant_horizons: DhPipeline {
            strategy: if v.native_dh { DhStrategy::Native } else { DhStrategy::Synthesized },
            unified_projection: !v.native_dh,
            shadow_enabled: false,
        },
    };
    let mut id_maps = IdMaps::default();
    id_maps.blocks.insert(1, vec!["minecraft:grass_block".into(), "minecraft:sand".into()]);
    id_maps.blocks.insert(2, vec!["minecraft:oak_leaves".into()]);
    id_maps.blocks.insert(3, vec!["minecraft:water".into()]);
    id_maps.entities.insert(10, vec!["minecraft:pig".into()]);
    let pack = CompiledPack {
        format_version: sb_core::MODEL_FORMAT_VERSION,
        info: PackInfo {
            name: "sb-runtime test pack".into(),
            source_hash: String::new(),
            shaderbridge_version: sb_core::SHADERBRIDGE_VERSION.into(),
            features_enabled: Vec::new(),
            features_unsupported: Vec::new(),
            environment: CompileEnvironment { depth_mode: v.depth_mode, ..Default::default() },
        },
        options: OptionsModel::default(),
        id_maps,
        dimensions: vec![dim],
        diagnostics: Default::default(),
        blobs: Vec::new(),
    };
    (pack, blobs)
}

/// A runtime on the CPU device with validation, or `None` (test skipped) when no
/// Vulkan device is available.
pub fn runtime() -> Option<Runtime> {
    match Runtime::new(&RuntimeOptions { validation: true, prefer_cpu_device: true, device_name_filter: None }) {
        Ok(rt) => {
            let info = rt.device_info();
            eprintln!("device: {} ({}), validation: {}, sync validation: {}", info.name, info.device_type, info.validation, info.sync_validation);
            assert_eq!(info.validation, info.sync_validation, "the validation layer is active without synchronization validation");
            Some(rt)
        }
        Err(e @ (RuntimeError::Loader(_) | RuntimeError::NoDevice(_) | RuntimeError::Unsupported(_))) => {
            eprintln!("skipping: no usable Vulkan device ({e})");
            None
        }
        Err(e) => panic!("Runtime::new failed: {e}"),
    }
}

pub fn small_scene() -> SceneParams {
    SceneParams { render_distance: 3, dh_render_distance: 12, ..Default::default() }
}

/// Render a pack with the default request settings.
pub fn render(rt: &mut Runtime, pack: &CompiledPack, blobs: &BlobTable, scene: SceneParams, (w, h): (u32, u32), frames: u32, capture: bool) -> RenderOutput {
    rt.render(&RenderRequest {
        pack,
        blobs,
        dimension: "world0",
        width: w,
        height: h,
        frames,
        scene,
        depth_mode: pack.info.environment.depth_mode,
        textures: &NoTextures,
        capture_targets: capture,
    })
    .unwrap_or_else(|e| panic!("render failed: {e}"))
}

/// Assert that the validation layer (core and synchronization validation) reported
/// nothing at all: every error and every warning is a bug. Requires the layer to be
/// active with synchronization validation whenever it is installed.
pub fn assert_no_validation_messages(out: &RenderOutput) {
    if !out.validation_messages.is_empty() {
        for m in &out.validation_messages {
            eprintln!("{m}");
        }
        panic!("{} validation messages ({} errors)", out.validation_messages.len(), out.validation_errors().count());
    }
}

/// Mean absolute difference per channel (0..1) between two images of the same size.
pub fn mean_abs_diff(a: &image::RgbaImage, b: &image::RgbaImage) -> f64 {
    assert_eq!(a.dimensions(), b.dimensions());
    let total: u64 = a.as_raw().iter().zip(b.as_raw()).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
    total as f64 / a.as_raw().len() as f64 / 255.0
}

/// Per-channel variance of the luminance (0..1 scale).
pub fn luminance_variance(img: &image::RgbaImage) -> f64 {
    let lum: Vec<f64> = img.pixels().map(|p| (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])) / 255.0).collect();
    let mean = lum.iter().sum::<f64>() / lum.len() as f64;
    lum.iter().map(|l| (l - mean) * (l - mean)).sum::<f64>() / lum.len() as f64
}

/// Directory for rendered images (`target/sb-renders`).
pub fn render_dir() -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sb-renders");
    let _ = std::fs::create_dir_all(&dir);
    dir
}
