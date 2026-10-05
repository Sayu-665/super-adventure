//! Serde JSON fixtures for the Java model tests (`dev.shaderbridge.model.SerdeFixtureTest`).
//!
//! * `<out_dir>`: writes `serde_shapes.json` (every variant of every serde representation in
//!   `sb_core::model`) and `compiled_pack_serde.json` (a CompiledPack using all of them).
//! * `--check <file>`: parses the JSON the Java test wrote back (`build/test-run/
//!   model-fixture-roundtrip.json`) with serde and checks it equals the fixture pack.
//! * `--parse <file>`: checks that a CompiledPack JSON file (e.g. the hand-written sample) is
//!   accepted by serde.
//! * `--registry <file>`: writes every builtin of `sb_uniforms::registry::all()` as
//!   `name<TAB>glsl type<TAB>frame|draw` lines (`dev.shaderbridge.uniforms.BuiltinUniformsTest`).
//! * `--std140 <file>`: writes the `sb_core::GlslType` std140 layout of every scalar, vector and
//!   matrix shape, plain and as arrays, as `scalar<TAB>rows<TAB>cols<TAB>array<TAB>align<TAB>size<TAB>stride`
//!   lines (`dev.shaderbridge.uniforms.Std140WriterTest`).
use indexmap::IndexMap;
use sb_core::model::*;
use sb_core::program::{AlphaFunc, BlendFactor, BlendMode, GeometryProgram, PassGroup};
use sb_core::{Diagnostic, Diagnostics, GlslType, ScalarKind, Severity, ShaderStage, SourceLocation, TextureFormat};
use serde_json::{Value, json};

fn v<T: serde::Serialize>(x: &T) -> Value {
    serde_json::to_value(x).unwrap()
}

fn shapes() -> Value {
    let mut m = serde_json::Map::new();
    m.insert("blob_kind".into(), v(&[BlobKind::Spirv, BlobKind::Glsl, BlobKind::Bytes]));
    m.insert(
        "depth_mode".into(),
        v(&[DepthMode::ForwardZeroToOne, DepthMode::ReversedZeroToOne, DepthMode::GlNegOneToOne]),
    );
    m.insert("output_target".into(), v(&[OutputTarget::Vulkan, OutputTarget::Renderpearl]));
    m.insert(
        "option_kind".into(),
        v(&[OptionKind::BooleanDefine, OptionKind::ValueDefine, OptionKind::Const]),
    );
    m.insert("dh_strategy".into(), v(&[DhStrategy::Native, DhStrategy::Synthesized, DhStrategy::Disabled]));
    m.insert("severity".into(), v(&[Severity::Info, Severity::Warning, Severity::Error]));
    m.insert(
        "scalar_kind".into(),
        v(&[ScalarKind::Float, ScalarKind::Int, ScalarKind::Uint, ScalarKind::Bool, ScalarKind::Double]),
    );
    m.insert(
        "shader_stage".into(),
        v(&[
            ShaderStage::Vertex,
            ShaderStage::TessControl,
            ShaderStage::TessEval,
            ShaderStage::Geometry,
            ShaderStage::Fragment,
            ShaderStage::Compute,
        ]),
    );
    m.insert("geometry_program".into(), v(&GeometryProgram::ALL));
    m.insert(
        "pass_group".into(),
        v(&[
            PassGroup::Setup,
            PassGroup::Begin,
            PassGroup::Shadow,
            PassGroup::ShadowComp,
            PassGroup::Prepare,
            PassGroup::GbuffersOpaque,
            PassGroup::Deferred,
            PassGroup::GbuffersTranslucent,
            PassGroup::Composite,
            PassGroup::Final,
        ]),
    );
    m.insert(
        "alpha_func".into(),
        v(&[
            AlphaFunc::Never,
            AlphaFunc::Less,
            AlphaFunc::Equal,
            AlphaFunc::LEqual,
            AlphaFunc::Greater,
            AlphaFunc::NotEqual,
            AlphaFunc::GEqual,
            AlphaFunc::Always,
        ]),
    );
    m.insert(
        "blend_factor".into(),
        v(&[
            BlendFactor::Zero,
            BlendFactor::One,
            BlendFactor::SrcColor,
            BlendFactor::OneMinusSrcColor,
            BlendFactor::DstColor,
            BlendFactor::OneMinusDstColor,
            BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha,
            BlendFactor::DstAlpha,
            BlendFactor::OneMinusDstAlpha,
            BlendFactor::SrcAlphaSaturate,
        ]),
    );
    m.insert("texture_format".into(), v(&TextureFormat::ALL));
    m.insert(
        "resource_ref".into(),
        v(&[
            ResourceRef::ColorTex(3),
            ResourceRef::DepthTex(1),
            ResourceRef::ShadowTex(0),
            ResourceRef::ShadowTexHw(1),
            ResourceRef::ShadowColor(2),
            ResourceRef::Noise,
            ResourceRef::Atlas,
            ResourceRef::Lightmap,
            ResourceRef::Normals,
            ResourceRef::Specular,
            ResourceRef::Overlay,
            ResourceRef::DhDepthTex(0),
            ResourceRef::DhBlockAtlas,
            ResourceRef::White,
            ResourceRef::CustomTexture("composite.cloudTex.3d".into()),
            ResourceRef::Image("imgVoxel".into()),
            ResourceRef::ColorImage(5),
            ResourceRef::ShadowColorImage(1),
            ResourceRef::Ssbo(4),
            ResourceRef::UniformBlock("sb_Frame".into()),
            ResourceRef::Unknown("mySampler".into()),
        ]),
    );
    m.insert(
        "resource_kind".into(),
        v(&[
            ResourceKind::Sampler { dim: "2d".into(), shadow: true, sample_type: "float".into() },
            ResourceKind::StorageImage {
                dim: "3d".into(),
                format: Some("rgba16f".into()),
                sample_type: "uint".into(),
                readonly: false,
                writeonly: true,
            },
            ResourceKind::StorageImage {
                dim: "2d".into(),
                format: None,
                sample_type: "float".into(),
                readonly: true,
                writeonly: false,
            },
            ResourceKind::StorageBuffer,
            ResourceKind::UniformBuffer,
        ]),
    );
    m.insert(
        "screen_entry".into(),
        v(&[
            ScreenEntry::Option("SHADOWS".into()),
            ScreenEntry::Screen("LIGHTING".into()),
            ScreenEntry::Profile,
            ScreenEntry::Empty,
            ScreenEntry::Rest,
        ]),
    );
    m.insert(
        "uniform_source".into(),
        v(&[
            UniformSource::Builtin("frameTimeCounter".into()),
            UniformSource::Custom("myCustom".into()),
            UniformSource::Unset,
        ]),
    );
    m.insert(
        "target_size".into(),
        v(&[
            TargetSize::Relative { x: 0.5, y: 0.25 },
            TargetSize::Absolute { width: 64, height: 32 },
            TargetSize::PerAxis { x: AxisSize::Relative(0.5), y: AxisSize::Absolute(64) },
        ]),
    );
    m.insert("axis_size".into(), v(&[AxisSize::Relative(0.75), AxisSize::Absolute(128)]));
    m.insert(
        "image_size".into(),
        v(&[
            ImageSize::Relative { x: 0.5, y: 1.0 },
            ImageSize::Absolute1D { width: 256 },
            ImageSize::Absolute2D { width: 512, height: 256 },
            ImageSize::Absolute3D { width: 64, height: 32, depth: 16 },
        ]),
    );
    m.insert(
        "texture_source".into(),
        v(&[
            TextureSource::PackImage { path: "textures/noise.png".into() },
            TextureSource::Resource { location: "minecraft:textures/atlas/blocks.png".into() },
            TextureSource::Dynamic { name: "minecraft:dynamic/lightmap_1".into() },
            TextureSource::Raw {
                path: "data/lut.dat".into(),
                target: "2d_rect".into(),
                dimensions: 3,
                format: TextureFormat::RGBA16F,
                size: [64, 32, 16],
                pixel_format: "RGBA".into(),
                pixel_type: "HALF_FLOAT".into(),
            },
        ]),
    );
    m.insert(
        "program_kind".into(),
        v(&[
            ProgramKind::Geometry { program: GeometryProgram::DamagedBlock },
            ProgramKind::Composite { group: PassGroup::Composite, index: 3 },
            ProgramKind::Compute { group: PassGroup::ShadowComp, index: 2, letter: Some('b') },
            ProgramKind::Compute { group: PassGroup::Deferred, index: 0, letter: None },
            ProgramKind::GeometryCompute { program: GeometryProgram::Shadow, letter: Some('a') },
            ProgramKind::GeometryCompute { program: GeometryProgram::Shadow, letter: None },
        ]),
    );
    m.insert(
        "work_groups".into(),
        v(&[WorkGroups::Absolute { x: 1, y: 2, z: 3 }, WorkGroups::Relative { x: 0.5, y: 0.25 }]),
    );
    m.insert(
        "glsl_type".into(),
        v(&[
            GlslType::FLOAT,
            GlslType::IVEC3,
            GlslType::MAT3,
            GlslType::matrix(3, 4),
            GlslType::BVEC2,
            GlslType::VEC4.with_array(5),
            GlslType::scalar(ScalarKind::Double),
        ]),
    );
    m.insert(
        "device_caps".into(),
        json!([
            v(&DeviceCaps::default()),
            v(&DeviceCaps { comparison_samplers: false, max_descriptors_per_program: Some(32), ..DeviceCaps::default() }),
            // An older producer without the newer fields: serde defaults apply.
            {"geometry_shader": false, "tessellation_shader": false, "storage_image_read_without_format": false,
             "storage_image_write_without_format": false, "depth_clip_control": true,
             "max_push_constants_size": 256, "max_color_attachments": 8}
        ]),
    );
    m.insert(
        "nan_floats".into(),
        json!({
            "alpha_test": v(&AlphaTest { func: AlphaFunc::Greater, reference: f32::NAN }),
            "viewport": v(&ViewportScale { scale: f32::INFINITY, offset_x: 0.0, offset_y: f32::NEG_INFINITY }),
        }),
    );
    Value::Object(m)
}

fn color_target(index: u32, clear_color: Option<[f32; 4]>, size: TargetSize) -> ColorTarget {
    ColorTarget {
        index,
        format: TextureFormat::RGBA16F,
        clear: index != 2,
        clear_color,
        mipmap_programs: if index == 0 { vec![4] } else { vec![] },
        size,
        used: true,
    }
}

fn stage(stage: ShaderStage, base: u32, file: &str) -> StageModule {
    StageModule {
        stage,
        entry_point: "main".into(),
        spirv: Some(BlobId(base)),
        glsl_vulkan: Some(BlobId(base + 1)),
        glsl_renderpearl: if stage == ShaderStage::Compute { None } else { Some(BlobId(base + 1)) },
        source_file: file.into(),
    }
}

fn base_program(name: &str, kind: ProgramKind) -> Program {
    Program {
        name: name.into(),
        kind,
        draw_profile: Some("fullscreen".into()),
        requires_raw_vulkan: false,
        stages: vec![],
        draw_buffers: vec![0],
        output_slots: vec![0],
        output_types: vec!["float".into()],
        blend: None,
        blend_per_buffer: IndexMap::new(),
        alpha_test: None,
        viewport: ViewportScale::default(),
        mipmap_targets: vec![],
        bindings_used: vec![],
        vertex_inputs: vec![],
        push_constant_size: 0,
        compute: None,
        cull: None,
        synthesized_from: None,
    }
}

fn pack() -> CompiledPack {
    let mut extra_macros = IndexMap::new();
    extra_macros.insert("SB_FLAG".to_string(), None);
    extra_macros.insert("SB_LEVEL".to_string(), Some("2".to_string()));
    let environment = CompileEnvironment {
        minecraft_version: "26.3".into(),
        os: "LINUX".into(),
        vendor: "MESA".into(),
        renderer: "GALLIUM".into(),
        distant_horizons: true,
        extra_macros,
        targets: vec![OutputTarget::Vulkan, OutputTarget::Renderpearl],
        depth_mode: DepthMode::ReversedZeroToOne,
        device: DeviceCaps { comparison_samplers: false, max_descriptors_per_program: Some(32), ..DeviceCaps::default() },
    };

    let options = vec![
        PackOption {
            name: "SHADOWS".into(),
            kind: OptionKind::BooleanDefine,
            default: "true".into(),
            value: "false".into(),
            allowed: vec![],
            comment: Some("Enable shadows".into()),
            file: "lib/settings.glsl".into(),
            line: 12,
        },
        PackOption {
            name: "SHADOW_RES".into(),
            kind: OptionKind::ValueDefine,
            default: "2048".into(),
            value: "1024".into(),
            allowed: vec!["1024".into(), "2048".into(), "4096".into()],
            comment: None,
            file: "lib/settings.glsl".into(),
            line: 13,
        },
        PackOption {
            name: "sunPathRotation".into(),
            kind: OptionKind::Const,
            default: "-40.0".into(),
            value: "-40.0".into(),
            allowed: vec!["-40.0".into(), "0.0".into(), "40.0".into()],
            comment: Some("".into()),
            file: "world0/composite.fsh".into(),
            line: 1,
        },
    ];
    let mut screens = IndexMap::new();
    screens.insert(
        "LIGHTING".to_string(),
        Screen { entries: vec![ScreenEntry::Option("SHADOW_RES".into()), ScreenEntry::Rest], columns: None },
    );
    screens.insert("EMPTY".to_string(), Screen { entries: vec![], columns: Some(1) });
    let mut low = IndexMap::new();
    low.insert("SHADOWS".to_string(), "false".to_string());
    low.insert("SHADOW_RES".to_string(), "1024".to_string());
    let mut profiles = IndexMap::new();
    profiles.insert("LOW".to_string(), low);
    profiles.insert("HIGH".to_string(), IndexMap::new());
    let mut profile_disabled_programs = IndexMap::new();
    profile_disabled_programs.insert("LOW".to_string(), vec!["world0/composite2".to_string()]);
    let mut lang = IndexMap::new();
    lang.insert("option.SHADOWS".to_string(), "Shadows".to_string());
    lang.insert("value.SHADOW_RES.1024".to_string(), "Low (1024)".to_string());
    let options_model = OptionsModel {
        options,
        main_screen: vec![
            ScreenEntry::Profile,
            ScreenEntry::Empty,
            ScreenEntry::Option("SHADOWS".into()),
            ScreenEntry::Screen("LIGHTING".into()),
            ScreenEntry::Rest,
        ],
        main_screen_columns: Some(3),
        screens,
        sliders: vec!["SHADOW_RES".into()],
        profiles,
        current_profile: Some("LOW".into()),
        profile_disabled_programs,
        lang,
    };

    let mut blocks = IndexMap::new();
    blocks.insert(10001, vec!["minecraft:oak_leaves".to_string(), "%minecraft:logs".to_string()]);
    blocks.insert(-1, vec!["minecraft:wheat:age=7".to_string()]);
    let mut items = IndexMap::new();
    items.insert(5000, vec!["minecraft:torch".to_string()]);
    let mut entities = IndexMap::new();
    entities.insert(0, vec!["minecraft:player".to_string()]);
    let mut layers = IndexMap::new();
    layers.insert("translucent".to_string(), vec!["minecraft:glass".to_string()]);
    let mut dims = IndexMap::new();
    dims.insert("world0".to_string(), vec!["minecraft:overworld".to_string(), "*".to_string()]);
    let id_maps = IdMaps { blocks, items, entities, layers, dimensions: dims };

    let shadow = ShadowSettings {
        enabled: true,
        resolution: 2048,
        fov: Some(90.0),
        distance: 128.0,
        near_plane: -1.0,
        far_plane: -1.0,
        distance_render_mul: 1.0,
        entity_distance_mul: 0.5,
        interval_size: 4.0,
        voxel_distance: 32.0,
        hardware_filtering: [true, false],
        mipmap: [false, true],
        nearest: [true, true],
        color_mipmap: vec![true; 8],
        color_nearest: vec![false, true, false, false, false, false, false, false],
        culling: "reversed".into(),
        render_terrain: true,
        render_translucent: false,
        render_entities: true,
        render_player: true,
        render_block_entities: false,
        render_light_block_entities: true,
        dh_shadow_enabled: false,
    };
    let targets = RenderTargets {
        colortex: vec![
            color_target(0, None, TargetSize::Relative { x: 1.0, y: 1.0 }),
            color_target(1, Some([1.0, 0.5, 0.25, 0.0]), TargetSize::Absolute { width: 256, height: 128 }),
            color_target(2, None, TargetSize::PerAxis { x: AxisSize::Relative(0.5), y: AxisSize::Absolute(64) }),
        ],
        shadowcolor: vec![color_target(0, Some([0.0, 0.0, 0.0, 1.0]), TargetSize::default())],
        shadow,
        uses_depthtex1: true,
        uses_depthtex2: false,
        noise_texture_resolution: 256,
        noise_texture: Some(TextureSource::PackImage { path: "textures/noise.png".into() }),
        custom_textures: vec![
            CustomTexture {
                sampler: "cloudTex".into(),
                stage: "composite".into(),
                source: TextureSource::Raw {
                    path: "data/cloud.dat".into(),
                    target: "3d".into(),
                    dimensions: 3,
                    format: TextureFormat::R8,
                    size: [32, 32, 32],
                    pixel_format: "RED".into(),
                    pixel_type: "UNSIGNED_BYTE".into(),
                },
                blur: true,
                clamp: false,
            },
            CustomTexture {
                sampler: "lightmapTex".into(),
                stage: "custom".into(),
                source: TextureSource::Dynamic { name: "minecraft:dynamic/lightmap_1".into() },
                blur: false,
                clamp: true,
            },
            CustomTexture {
                sampler: "atlasTex".into(),
                stage: "gbuffers".into(),
                source: TextureSource::Resource { location: "minecraft:textures/atlas/blocks.png".into() },
                blur: false,
                clamp: false,
            },
        ],
        images: vec![
            CustomImage {
                name: "imgVoxel".into(),
                sampler_name: Some("voxelSampler".into()),
                format: TextureFormat::R32UI,
                pixel_format: "RED_INTEGER".into(),
                pixel_type: "UNSIGNED_INT".into(),
                clear: true,
                size: ImageSize::Absolute3D { width: 64, height: 32, depth: 16 },
            },
            CustomImage {
                name: "imgHalf".into(),
                sampler_name: None,
                format: TextureFormat::RGBA16F,
                pixel_format: "RGBA".into(),
                pixel_type: "HALF_FLOAT".into(),
                clear: false,
                size: ImageSize::Relative { x: 0.5, y: 0.5 },
            },
            CustomImage {
                name: "img1d".into(),
                sampler_name: None,
                format: TextureFormat::R32F,
                pixel_format: "RED".into(),
                pixel_type: "FLOAT".into(),
                clear: false,
                size: ImageSize::Absolute1D { width: 256 },
            },
            CustomImage {
                name: "img2d".into(),
                sampler_name: None,
                format: TextureFormat::RGBA8,
                pixel_format: "RGBA".into(),
                pixel_type: "UNSIGNED_BYTE".into(),
                clear: true,
                size: ImageSize::Absolute2D { width: 512, height: 256 },
            },
        ],
        buffers: vec![
            StorageBuffer { index: 0, size: 5_000_000_000, relative: None, file: Some("data/init.bin".into()) },
            StorageBuffer { index: 4, size: 16, relative: Some([0.5, 1.0]), file: None },
        ],
    };
    let mut back_face = IndexMap::new();
    back_face.insert("solid".to_string(), true);
    back_face.insert("translucent".to_string(), false);
    let mut raw = IndexMap::new();
    raw.insert("clouds".to_string(), "off".to_string());
    let settings = PackSettings {
        clouds: "off".into(),
        dh_clouds: "on".into(),
        old_hand_light: false,
        dynamic_hand_light: true,
        sun_path_rotation: -40.0,
        back_face,
        raw,
        fallback_tex: 3,
        ..PackSettings::default()
    };
    let frame = BlockLayout {
        name: "sb_Frame".into(),
        set: 0,
        binding: 0,
        size: 96,
        members: vec![
            BlockMember {
                name: "frameTimeCounter".into(),
                ty: GlslType::FLOAT,
                offset: 0,
                source: UniformSource::Builtin("frameTimeCounter".into()),
                default: None,
            },
            BlockMember {
                name: "myCustom".into(),
                ty: GlslType::VEC3,
                offset: 16,
                source: UniformSource::Custom("myCustom".into()),
                default: None,
            },
            BlockMember {
                name: "unknownThing".into(),
                ty: GlslType::MAT2,
                offset: 32,
                source: UniformSource::Unset,
                default: Some(vec![1.0, 0.0, 0.0, 1.0]),
            },
            BlockMember {
                name: "weights".into(),
                ty: GlslType::FLOAT.with_array(2),
                offset: 64,
                source: UniformSource::Unset,
                default: Some(vec![0.5, 0.25]),
            },
        ],
    };
    let draw = BlockLayout {
        name: "sb_Draw".into(),
        set: 0,
        binding: 1,
        size: 16,
        members: vec![BlockMember {
            name: "entityId".into(),
            ty: GlslType::INT,
            offset: 0,
            source: UniformSource::Builtin("entityId".into()),
            default: None,
        }],
    };
    let custom_uniforms = vec![
        CustomUniform {
            name: "myCustom".into(),
            ty: GlslType::VEC3,
            expression: "vec3(sin(frameTimeCounter), 0, 1)".into(),
            is_variable: false,
            location: Some(SourceLocation { file: "shaders.properties".into(), line: 40, column: Some(3) }),
        },
        CustomUniform {
            name: "tmp".into(),
            ty: GlslType::FLOAT,
            expression: "frameTimeCounter * 2".into(),
            is_variable: true,
            location: None,
        },
    ];
    let bindings = BindingTable {
        entries: vec![
            BindingEntry {
                name: "colortex0".into(),
                set: 1,
                binding: 0,
                kind: ResourceKind::Sampler { dim: "2d".into(), shadow: false, sample_type: "float".into() },
                resource: ResourceRef::ColorTex(0),
            },
            BindingEntry {
                name: "shadowtex0HW".into(),
                set: 1,
                binding: 1,
                kind: ResourceKind::Sampler { dim: "2d".into(), shadow: true, sample_type: "float".into() },
                resource: ResourceRef::ShadowTexHw(0),
            },
            BindingEntry {
                name: "imgVoxel".into(),
                set: 2,
                binding: 0,
                kind: ResourceKind::StorageImage {
                    dim: "3d".into(),
                    format: Some("r32ui".into()),
                    sample_type: "uint".into(),
                    readonly: false,
                    writeonly: false,
                },
                resource: ResourceRef::Image("imgVoxel".into()),
            },
            BindingEntry {
                name: "ssbo4".into(),
                set: 2,
                binding: 4,
                kind: ResourceKind::StorageBuffer,
                resource: ResourceRef::Ssbo(4),
            },
            BindingEntry {
                name: "sb_Frame".into(),
                set: 0,
                binding: 0,
                kind: ResourceKind::UniformBuffer,
                resource: ResourceRef::UniformBlock("sb_Frame".into()),
            },
            BindingEntry {
                name: "gtexture".into(),
                set: 1,
                binding: 2,
                kind: ResourceKind::Sampler { dim: "2d".into(), shadow: false, sample_type: "float".into() },
                resource: ResourceRef::White,
            },
            // An emulated comparison sampler: declared as a plain sampler.
            BindingEntry {
                name: "shadowtex1".into(),
                set: 1,
                binding: 3,
                kind: ResourceKind::Sampler { dim: "2d".into(), shadow: false, sample_type: "float".into() },
                resource: ResourceRef::ShadowTex(1),
            },
        ],
    };

    let mut terrain = base_program(
        "world0/gbuffers_terrain",
        ProgramKind::Geometry { program: GeometryProgram::Terrain },
    );
    terrain.draw_profile = Some("vanilla_terrain".into());
    terrain.stages = vec![
        stage(ShaderStage::Vertex, 0, "world0/gbuffers_terrain.vsh"),
        stage(ShaderStage::Fragment, 2, "world0/gbuffers_terrain.fsh"),
    ];
    terrain.draw_buffers = vec![0, 4];
    terrain.output_slots = vec![0, 2];
    terrain.output_types = vec!["float".into(), "uint".into()];
    terrain.blend = Some(BlendMode::TRANSLUCENT);
    terrain.blend_per_buffer.insert(4, None);
    terrain.blend_per_buffer.insert(
        0,
        Some(BlendMode {
            src_color: BlendFactor::One,
            dst_color: BlendFactor::Zero,
            src_alpha: BlendFactor::SrcAlphaSaturate,
            dst_alpha: BlendFactor::OneMinusDstColor,
        }),
    );
    terrain.alpha_test = Some(AlphaTest { func: AlphaFunc::Greater, reference: 0.1 });
    terrain.viewport = ViewportScale { scale: 0.5, offset_x: 0.25, offset_y: 0.0 };
    terrain.mipmap_targets = vec![1];
    terrain.bindings_used = vec![
        BindingUse {
            name: "colortex0".into(),
            set: 1,
            binding: 0,
            use_alt: true,
            stages: vec![ShaderStage::Vertex, ShaderStage::Fragment],
            shadow_emulated: false,
        },
        BindingUse {
            name: "shadowtex1".into(),
            set: 1,
            binding: 3,
            use_alt: false,
            stages: vec![ShaderStage::Fragment],
            shadow_emulated: true,
        },
    ];
    terrain.vertex_inputs = vec![
        VertexInput { location: 0, name: "Position".into(), ty: "vec3".into(), semantic: Some("position".into()) },
        VertexInput { location: 5, name: "mc_Entity".into(), ty: "vec4".into(), semantic: None },
    ];
    terrain.push_constant_size = 16;
    terrain.cull = Some(false);

    let mut dh = base_program("world0/dh_terrain", ProgramKind::Geometry { program: GeometryProgram::DhTerrain });
    dh.draw_profile = Some("dh_terrain".into());
    dh.synthesized_from = Some("world0/gbuffers_terrain".into());
    dh.requires_raw_vulkan = true;
    dh.cull = Some(true);

    let mut comp = base_program(
        "world0/composite3_b",
        ProgramKind::Compute { group: PassGroup::Composite, index: 3, letter: Some('b') },
    );
    comp.draw_profile = None;
    comp.requires_raw_vulkan = true;
    comp.stages = vec![stage(ShaderStage::Compute, 4, "world0/composite3_b.csh")];
    comp.draw_buffers = vec![];
    comp.output_slots = vec![];
    comp.output_types = vec![];
    comp.compute = Some(ComputeInfo {
        local_size: [8, 8, 1],
        work_groups: WorkGroups::Relative { x: 0.5, y: 0.5 },
        indirect: Some((4, 16)),
    });
    let mut comp2 = base_program("world0/shadow_a", ProgramKind::GeometryCompute { program: GeometryProgram::Shadow, letter: Some('a') });
    comp2.draw_profile = None;
    comp2.compute = Some(ComputeInfo { local_size: [64, 1, 1], work_groups: WorkGroups::Absolute { x: 4, y: 1, z: 1 }, indirect: None });
    let fin = base_program("world0/final", ProgramKind::Composite { group: PassGroup::Final, index: 0 });

    let mut geometry = IndexMap::new();
    let mut terrain_variants = IndexMap::new();
    terrain_variants.insert("sodium_terrain".to_string(), 3);
    terrain_variants.insert("vanilla_entity".to_string(), 2);
    geometry.insert(GeometryProgram::Terrain, GeometrySlot { program: 0, resolved_from: GeometryProgram::Terrain, variants: terrain_variants });
    geometry.insert(GeometryProgram::DamagedBlock, GeometrySlot { program: 0, resolved_from: GeometryProgram::Terrain, variants: IndexMap::new() });
    geometry.insert(GeometryProgram::DhTerrain, GeometrySlot { program: 1, resolved_from: GeometryProgram::DhTerrain, variants: IndexMap::new() });

    let world0 = DimensionPipeline {
        folder: "world0".into(),
        dimension_ids: vec!["minecraft:overworld".into()],
        targets,
        settings,
        uniforms: UniformLayout { frame, draw },
        custom_uniforms,
        bindings,
        programs: vec![terrain, dh, comp, comp2, fin],
        geometry,
        passes: vec![
            Pass {
                group: PassGroup::GbuffersOpaque,
                index: 0,
                computes: vec![],
                program: None,
                flips_after: vec![],
                flip_state: vec![false, false, false],
            },
            Pass {
                group: PassGroup::Composite,
                index: 3,
                computes: vec![2],
                program: None,
                flips_after: vec![0, 2],
                flip_state: vec![true, false, true],
            },
            Pass {
                group: PassGroup::Final,
                index: 0,
                computes: vec![],
                program: Some(4),
                flips_after: vec![],
                flip_state: vec![false, false, false],
            },
        ],
        gbuffer_attachments: vec![0, 2, 4],
        shadow_attachments: vec![0],
        end_of_frame_copies: vec![2],
        distant_horizons: DhPipeline { strategy: DhStrategy::Synthesized, unified_projection: true, shadow_enabled: false },
    };
    let root = DimensionPipeline {
        folder: "".into(),
        dimension_ids: vec!["*".into()],
        targets: RenderTargets {
            colortex: vec![],
            shadowcolor: vec![],
            shadow: ShadowSettings::default(),
            uses_depthtex1: false,
            uses_depthtex2: false,
            noise_texture_resolution: 64,
            noise_texture: None,
            custom_textures: vec![],
            images: vec![],
            buffers: vec![],
        },
        settings: PackSettings::default(),
        uniforms: UniformLayout::default(),
        custom_uniforms: vec![],
        bindings: BindingTable::default(),
        programs: vec![],
        geometry: IndexMap::new(),
        passes: vec![],
        gbuffer_attachments: vec![],
        shadow_attachments: vec![],
        end_of_frame_copies: vec![],
        distant_horizons: DhPipeline::default(),
    };

    let mut diagnostics = Diagnostics::new();
    diagnostics.push(
        Diagnostic::error("spv.compile", "'foo' : undeclared identifier")
            .at(SourceLocation { file: "lib/a.glsl".into(), line: 7, column: Some(12) })
            .in_program("world0/gbuffers_terrain")
            .in_stage(ShaderStage::TessEval),
    );
    diagnostics.push(Diagnostic::warning("xf.unknown-builtin", "unknown").at(SourceLocation::new("b.fsh", 3)));
    diagnostics.push(Diagnostic::info("opt.screen-unknown-option", "note"));

    CompiledPack {
        format_version: sb_core::MODEL_FORMAT_VERSION,
        info: PackInfo {
            name: "Fixture Pack.zip".into(),
            source_hash: "0123abcd".into(),
            shaderbridge_version: "0.1.0".into(),
            features_enabled: vec!["SSBO".into(), "CUSTOM_IMAGES".into()],
            features_unsupported: vec!["PER_BUFFER_BLENDING_X".into()],
            environment,
        },
        options: options_model,
        id_maps,
        dimensions: vec![world0, root],
        diagnostics,
        blobs: vec![
            BlobInfo { kind: BlobKind::Spirv, offset: 0, len: 20 },
            BlobInfo { kind: BlobKind::Glsl, offset: 24, len: 13 },
            BlobInfo { kind: BlobKind::Spirv, offset: 40, len: 20 },
            BlobInfo { kind: BlobKind::Glsl, offset: 64, len: 13 },
            BlobInfo { kind: BlobKind::Spirv, offset: 80, len: 20 },
            BlobInfo { kind: BlobKind::Glsl, offset: 104, len: 13 },
            BlobInfo { kind: BlobKind::Bytes, offset: 120, len: 3 },
        ],
    }
}

/// std140 layouts computed by `sb_core::GlslType` for every shape (array `-` = not an array).
fn std140() -> String {
    let mut out = String::from("# Generated from sb_core::GlslType by java/src/test/rust/model-fixtures (--std140).\n");
    let mut shapes = Vec::new();
    for scalar in [ScalarKind::Float, ScalarKind::Int, ScalarKind::Uint, ScalarKind::Bool, ScalarKind::Double] {
        for rows in 1..=4u8 {
            shapes.push(GlslType { scalar, rows, cols: 1, array: None });
        }
    }
    for scalar in [ScalarKind::Float, ScalarKind::Double] {
        for cols in 2..=4u8 {
            for rows in 2..=4u8 {
                shapes.push(GlslType { scalar, rows, cols, array: None });
            }
        }
    }
    for shape in shapes {
        for array in [None, Some(0), Some(1), Some(3), Some(7)] {
            let ty = GlslType { array, ..shape };
            let scalar = serde_json::to_value(ty.scalar).unwrap();
            let array = array.map_or("-".to_string(), |n| n.to_string());
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                scalar.as_str().unwrap(),
                ty.rows,
                ty.cols,
                array,
                ty.std140_align(),
                ty.std140_size(),
                ty.std140_array_stride()
            ));
        }
    }
    out
}

/// The builtin-uniform registry as `name<TAB>type<TAB>frequency` lines, in registry order.
fn registry() -> String {
    let mut out = String::from("# Generated from sb_uniforms::registry::all() by java/src/test/rust/model-fixtures (--registry).\n");
    for b in sb_uniforms::registry::all() {
        let frequency = match b.frequency {
            sb_uniforms::registry::Frequency::Frame => "frame",
            sb_uniforms::registry::Frequency::Draw => "draw",
        };
        out.push_str(&format!("{}\t{}\t{}\n", b.name, b.ty, frequency));
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--check" {
        // Parse JSON written by the Java side back into the Rust model.
        let text = std::fs::read_to_string(&args[2]).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        let pack: CompiledPack = serde_json::from_value(value["pack"].clone()).expect("CompiledPack from Java");
        assert_eq!(pack, self::pack(), "Java round trip differs");
        let env: CompileEnvironment = serde_json::from_value(value["environment"].clone()).expect("CompileEnvironment from Java");
        println!("Java JSON parses in Rust: pack equal, environment = {env:?}");
        return;
    }
    if args.len() == 3 && args[1] == "--parse" {
        let text = std::fs::read_to_string(&args[2]).unwrap();
        match CompiledPack::from_json(&text) {
            Ok(p) => println!("parsed: {} dimensions", p.dimensions.len()),
            Err(e) => {
                eprintln!("ERROR: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args.len() == 3 && args[1] == "--std140" {
        std::fs::write(&args[2], std140()).unwrap();
        return;
    }
    if args.len() == 3 && args[1] == "--registry" {
        std::fs::write(&args[2], registry()).unwrap();
        return;
    }
    let Some(out) = args.get(1).map(std::path::Path::new) else {
        eprintln!("usage: sb-java-model-fixtures <out_dir> | --check <file> | --parse <file> | --registry <file> | --std140 <file>");
        std::process::exit(2);
    };
    let pack = pack();
    let text = serde_json::to_string_pretty(&pack).unwrap();
    assert_eq!(CompiledPack::from_json(&text).unwrap(), pack);
    std::fs::write(out.join("compiled_pack_serde.json"), text + "\n").unwrap();
    std::fs::write(out.join("serde_shapes.json"), serde_json::to_string_pretty(&shapes()).unwrap() + "\n").unwrap();
}
