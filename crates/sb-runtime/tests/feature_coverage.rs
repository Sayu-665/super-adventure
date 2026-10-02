//! A second hand-assembled pack that exercises the model features the minimal pack does
//! not: per-program attachments (no shared gbuffer list), an integer render target,
//! geometry and tessellation stages, push constants, a pack uniform block, a shadowcomp
//! pass (shadowcolor ping-pong), mipmapped targets, a viewport scale, a compute pass with
//! storage images (colorimg + custom image), an indirect dispatch from an SSBO with
//! initial contents, a relative-size SSBO, PNG and raw 3D custom textures, unknown
//! samplers (including one that needs a 3D fallback), a non-cleared target accumulating
//! over frames with an end-of-frame copy, and no `final` program.

mod common;

use common::shaders::{DRAW_DECL, FRAME_DECL, PRELUDE, fullscreen_vsh, terrain_vsh};
use common::{GPU_LOCK, assert_no_validation_errors, frame_layout, draw_layout, luminance_variance, render_dir, runtime, small_scene};
use indexmap::IndexMap;
use sb_compile::{CompileOptions, compile_glsl};
use sb_core::model::*;
use sb_core::program::{BlendMode, GeometryProgram};
use sb_core::{GlslType, PassGroup, ShaderStage, TextureFormat};
use sb_runtime::RenderRequest;

fn compile(src: &str, stage: ShaderStage) -> Vec<u32> {
    compile_glsl(src, stage, "coverage", &CompileOptions::default(), None).unwrap_or_else(|e| panic!("{e}\n{}\n{src}", e.log))
}

fn terrain_fsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{DRAW_DECL}{}",
        r#"
layout(set = 1, binding = 0) uniform sampler2D Sampler0;
layout(push_constant) uniform Push { float tint; } pc;
layout(location = 0) in vec2 texcoord;
layout(location = 1) in vec4 color;
layout(location = 2) in vec2 lmcoord;
layout(location = 3) in vec3 normal;
layout(location = 4) flat in int blockId;
layout(location = 0) out vec4 sb_FragData0;
layout(location = 1) out uint sb_FragData1;
void main() {
    sb_FragData0 = texture(Sampler0, texcoord) * color * (1.0 + pc.tint);
    sb_FragData1 = uint(max(blockId, 0)) + 1u;
    if (!(sb_FragData0.a > alphaTestRef)) discard;
}
"#
    )
}

fn water_gsh() -> String {
    format!(
        "#version 460\n{}",
        r#"
layout(triangles) in;
layout(triangle_strip, max_vertices = 3) out;
layout(location = 0) in vec2 texcoordIn[];
layout(location = 1) in vec4 colorIn[];
layout(location = 0) out vec2 texcoord;
layout(location = 1) out vec4 color;
void main() {
    for (int i = 0; i < 3; i++) {
        gl_Position = gl_in[i].gl_Position;
        texcoord = texcoordIn[i];
        color = colorIn[i];
        EmitVertex();
    }
    EndPrimitive();
}
"#
    )
}

fn water_fsh() -> String {
    r#"#version 460
layout(set = 1, binding = 0) uniform sampler2D Sampler0;
layout(location = 0) in vec2 texcoord;
layout(location = 1) in vec4 color;
layout(location = 0) out vec4 sb_FragData0;
void main() { sb_FragData0 = vec4((texture(Sampler0, texcoord) * color).rgb, 0.6); }
"#
    .to_string()
}

fn sky_vsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{DRAW_DECL}{}",
        r#"
layout(std140, set = 0, binding = 4) uniform DynamicTransforms { mat4 ModelViewMat; mat4 TextureMat; vec4 ColorModulator; vec3 ModelOffset; } sb_hDynamic;
layout(location = 0) in vec3 Position;
layout(location = 0) out vec4 colorV;
void main() {
    gl_Position = projectionMatrix * (sb_hDynamic.ModelViewMat * vec4(Position + sb_hDynamic.ModelOffset, 1.0));
    colorV = sb_hDynamic.ColorModulator;
}
"#
    )
}

fn sky_tcs() -> String {
    r#"#version 460
layout(vertices = 3) out;
layout(location = 0) in vec4 colorV[];
layout(location = 0) out vec4 colorC[];
void main() {
    gl_out[gl_InvocationID].gl_Position = gl_in[gl_InvocationID].gl_Position;
    colorC[gl_InvocationID] = colorV[gl_InvocationID];
    gl_TessLevelOuter[0] = 2.0; gl_TessLevelOuter[1] = 2.0; gl_TessLevelOuter[2] = 2.0;
    gl_TessLevelInner[0] = 2.0;
}
"#
    .to_string()
}

fn sky_tes() -> String {
    format!(
        "#version 460\n{PRELUDE}{}",
        r#"
layout(triangles, equal_spacing, ccw) in;
layout(location = 0) in vec4 colorC[];
layout(location = 0) out vec4 color;
void main() {
    gl_Position = gl_TessCoord.x * gl_in[0].gl_Position + gl_TessCoord.y * gl_in[1].gl_Position + gl_TessCoord.z * gl_in[2].gl_Position;
    color = gl_TessCoord.x * colorC[0] + gl_TessCoord.y * colorC[1] + gl_TessCoord.z * colorC[2];
    SB_DEPTH_EPILOGUE
}
"#
    )
}

fn sky_fsh() -> String {
    r#"#version 460
layout(location = 0) in vec4 color;
layout(location = 0) out vec4 sb_FragData0;
void main() { sb_FragData0 = vec4(color.rgb, 1.0); }
"#
    .to_string()
}

fn shadow_vsh() -> String {
    common::shaders::shadow_vsh()
}

fn shadow_fsh() -> String {
    common::shaders::shadow_fsh()
}

fn shadowcomp_fsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{}",
        r#"
layout(set = 1, binding = 8) uniform sampler2D shadowtex0;
layout(set = 1, binding = 9) uniform sampler2D shadowcolor0;
layout(location = 0) in vec2 texcoord;
layout(location = 0) out vec4 sb_FragData0;
void main() {
    float d = SB_READ_DEPTH(texture(shadowtex0, texcoord).r);
    sb_FragData0 = vec4(texture(shadowcolor0, texcoord).rgb * 0.5, d < 1.0 ? 1.0 : 0.0);
}
"#
    )
}

fn composite_fsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{FRAME_DECL}{}",
        r#"
layout(std140, set = 0, binding = 7) uniform PackBlock { vec4 packValue; } pb;
layout(set = 1, binding = 2) uniform sampler2D colortex0;
layout(set = 1, binding = 3) uniform usampler2D colortex3;
layout(set = 1, binding = 5) uniform sampler2D colortex5;
layout(set = 1, binding = 6) uniform sampler2D colortex6;
layout(set = 1, binding = 9) uniform sampler2D shadowcolor0;
layout(set = 1, binding = 10) uniform sampler2D sb_tex_composite_mytex;
layout(set = 1, binding = 11) uniform sampler3D sb_tex_composite_lut_3d;
layout(set = 1, binding = 12) uniform sampler2D mystery;
layout(set = 1, binding = 13) uniform sampler3D mystery3d;
layout(set = 1, binding = 14) uniform sampler2D myimgSampler;
layout(std430, set = 2, binding = 0) readonly buffer Data { vec4 v[]; } data;
layout(location = 0) in vec2 texcoord;
layout(location = 0) out vec4 sb_FragData0;
layout(location = 1) out vec4 sb_FragData1;
void main() {
    vec3 base = textureLod(colortex0, texcoord, 2.0).rgb * 0.5 + texture(colortex0, texcoord).rgb * 0.5;
    uint id = texelFetch(colortex3, ivec2(gl_FragCoord.xy), 0).r;
    float idf = id > 0u ? 1.0 : 0.9;
    vec3 c5 = texture(colortex5, texcoord).rgb;
    float custom = texture(sb_tex_composite_mytex, texcoord).g * texture(sb_tex_composite_lut_3d, vec3(texcoord, 0.5)).r;
    float unknown = texture(mystery, texcoord).a * texture(mystery3d, vec3(0.5)).a;
    float img = texture(myimgSampler, vec2(0.5)).r;
    float ok = data.v[0].x * 0.5 * img * unknown;
    sb_FragData0 = vec4((base * idf + c5 * 0.05 + custom * 0.01 + texture(shadowcolor0, texcoord).rgb * 0.01 + pb.packValue.rgb) * ok, 1.0);
    sb_FragData1 = texture(colortex6, texcoord) + vec4(0.1);
}
"#
    )
}

fn composite1_fsh() -> String {
    r#"#version 460
layout(set = 1, binding = 2) uniform sampler2D colortex0;
layout(location = 0) in vec2 texcoord;
layout(location = 0) out vec4 sb_FragData0;
void main() { sb_FragData0 = vec4(texture(colortex0, texcoord).rgb, 1.0); }
"#
    .to_string()
}

fn compute_a() -> String {
    r#"#version 460
layout(local_size_x = 8, local_size_y = 8) in;
layout(rgba16f, set = 2, binding = 16) uniform writeonly image2D colorimg5;
layout(r32f, set = 2, binding = 17) uniform writeonly image2D myimg;
void main() {
    ivec2 p = ivec2(gl_GlobalInvocationID.xy);
    if (all(lessThan(p, imageSize(colorimg5)))) imageStore(colorimg5, p, vec4(0.25, 0.5, 0.75, 1.0));
    if (all(lessThan(p, imageSize(myimg)))) imageStore(myimg, p, vec4(1.0));
}
"#
    .to_string()
}

fn compute_b() -> String {
    r#"#version 460
layout(local_size_x = 1) in;
layout(std430, set = 2, binding = 0) buffer Data { vec4 v[]; } data;
void main() { data.v[0] = vec4(2.0); }
"#
    .to_string()
}

fn ct(index: u32, format: TextureFormat) -> ColorTarget {
    ColorTarget { index, format, clear: true, clear_color: None, mipmap_programs: Vec::new(), size: TargetSize::default(), used: true }
}

fn coverage_pack() -> (CompiledPack, BlobTable) {
    let mut blobs = BlobTable::default();
    let s2 = |name: &str, binding: u32, resource: ResourceRef| BindingEntry { name: name.into(), set: 1, binding, kind: ResourceKind::Sampler { dim: "2d".into(), shadow: false, sample_type: "float".into() }, resource };
    let ubo = |name: &str, binding: u32| BindingEntry { name: name.into(), set: 0, binding, kind: ResourceKind::UniformBuffer, resource: ResourceRef::UniformBlock(name.into()) };
    let img = |name: &str, binding: u32, format: &str, resource: ResourceRef| BindingEntry {
        name: name.into(),
        set: 2,
        binding,
        kind: ResourceKind::StorageImage { dim: "2d".into(), format: Some(format.into()), sample_type: "float".into(), readonly: false, writeonly: true },
        resource,
    };
    let table = BindingTable {
        entries: vec![
            ubo("Globals", 2),
            ubo("TerrainUniform", 3),
            ubo("DynamicTransforms", 4),
            ubo("PackBlock", 7),
            s2("gtexture", 0, ResourceRef::Atlas),
            s2("lightmap", 1, ResourceRef::Lightmap),
            s2("colortex0", 2, ResourceRef::ColorTex(0)),
            BindingEntry { name: "colortex3".into(), set: 1, binding: 3, kind: ResourceKind::Sampler { dim: "2d".into(), shadow: false, sample_type: "uint".into() }, resource: ResourceRef::ColorTex(3) },
            s2("colortex5", 5, ResourceRef::ColorTex(5)),
            s2("colortex6", 6, ResourceRef::ColorTex(6)),
            s2("shadowtex0", 8, ResourceRef::ShadowTex(0)),
            s2("shadowcolor0", 9, ResourceRef::ShadowColor(0)),
            s2("sb_tex_composite_mytex", 10, ResourceRef::CustomTexture("composite.mytex".into())),
            BindingEntry { name: "sb_tex_composite_lut_3d".into(), set: 1, binding: 11, kind: ResourceKind::Sampler { dim: "3d".into(), shadow: false, sample_type: "float".into() }, resource: ResourceRef::CustomTexture("composite.lut.3d".into()) },
            s2("mystery", 12, ResourceRef::Unknown("mystery".into())),
            BindingEntry { name: "mystery3d".into(), set: 1, binding: 13, kind: ResourceKind::Sampler { dim: "3d".into(), shadow: false, sample_type: "float".into() }, resource: ResourceRef::Unknown("mystery3d".into()) },
            s2("myimgSampler", 14, ResourceRef::Image("myimg".into())),
            BindingEntry { name: "Data".into(), set: 2, binding: 0, kind: ResourceKind::StorageBuffer, resource: ResourceRef::Ssbo(0) },
            img("colorimg5", 16, "rgba16f", ResourceRef::ColorImage(5)),
            img("myimg", 17, "r32f", ResourceRef::Image("myimg".into())),
        ],
    };
    let mut add = |name: &str, kind: ProgramKind, profile: Option<&str>, stages: Vec<(ShaderStage, String)>, draw_buffers: Vec<u32>| -> Program {
        let stages = stages
            .into_iter()
            .map(|(stage, src)| {
                let words = compile(&src, stage);
                StageModule { stage, entry_point: "main".into(), spirv: Some(blobs.push_spirv(&words)), glsl_vulkan: None, glsl_renderpearl: None, source_file: name.into() }
            })
            .collect();
        Program {
            name: name.into(),
            kind,
            draw_profile: profile.map(str::to_string),
            requires_raw_vulkan: true,
            stages,
            output_slots: (0..draw_buffers.len() as u32).collect(),
            output_types: Vec::new(),
            draw_buffers,
            blend: None,
            blend_per_buffer: IndexMap::new(),
            alpha_test: None,
            viewport: ViewportScale::default(),
            mipmap_targets: Vec::new(),
            bindings_used: Vec::new(),
            vertex_inputs: Vec::new(),
            push_constant_size: 0,
            compute: None,
            cull: None,
            synthesized_from: None,
        }
    };
    let geo = |p| ProgramKind::Geometry { program: p };
    let fs = |g, i| ProgramKind::Composite { group: g, index: i };
    let mut programs = vec![
        add("gbuffers_terrain", geo(GeometryProgram::Terrain), Some("vanilla_terrain"), vec![(ShaderStage::Vertex, terrain_vsh()), (ShaderStage::Fragment, terrain_fsh())], vec![0, 3]),
        add("gbuffers_water", geo(GeometryProgram::Water), Some("vanilla_terrain"), vec![(ShaderStage::Vertex, terrain_vsh()), (ShaderStage::Geometry, water_gsh()), (ShaderStage::Fragment, water_fsh())], vec![0]),
        add(
            "gbuffers_skybasic",
            geo(GeometryProgram::SkyBasic),
            Some("vanilla_position"),
            vec![(ShaderStage::Vertex, sky_vsh()), (ShaderStage::TessControl, sky_tcs()), (ShaderStage::TessEval, sky_tes()), (ShaderStage::Fragment, sky_fsh())],
            vec![0],
        ),
        add("shadow", geo(GeometryProgram::Shadow), Some("vanilla_terrain"), vec![(ShaderStage::Vertex, shadow_vsh()), (ShaderStage::Fragment, shadow_fsh())], vec![0]),
        add("shadowcomp", fs(PassGroup::ShadowComp, 0), Some("fullscreen"), vec![(ShaderStage::Vertex, fullscreen_vsh()), (ShaderStage::Fragment, shadowcomp_fsh())], vec![0]),
        add("composite", fs(PassGroup::Composite, 0), Some("fullscreen"), vec![(ShaderStage::Vertex, fullscreen_vsh()), (ShaderStage::Fragment, composite_fsh())], vec![0, 6]),
        add("composite1", fs(PassGroup::Composite, 1), Some("fullscreen"), vec![(ShaderStage::Vertex, fullscreen_vsh()), (ShaderStage::Fragment, composite1_fsh())], vec![4]),
        add("composite_a", ProgramKind::Compute { group: PassGroup::Composite, index: 0, letter: Some('a') }, None, vec![(ShaderStage::Compute, compute_a())], vec![]),
        add("composite_b", ProgramKind::Compute { group: PassGroup::Composite, index: 0, letter: Some('b') }, None, vec![(ShaderStage::Compute, compute_b())], vec![]),
    ];
    programs[0].alpha_test = Some(AlphaTest { func: sb_core::program::AlphaFunc::Greater, reference: 0.1 });
    programs[0].output_types = vec!["float".into(), "uint".into()];
    programs[0].blend = Some(BlendMode::TRANSLUCENT); // must be ignored for the uint target
    programs[1].blend = Some(BlendMode::TRANSLUCENT);
    programs[5].mipmap_targets = vec![0];
    programs[6].viewport = ViewportScale { scale: 0.5, offset_x: 0.0, offset_y: 0.0 };
    programs[6].bindings_used = vec![BindingUse { name: "colortex0".into(), set: 1, binding: 2, use_alt: true, stages: vec![ShaderStage::Fragment] }];
    programs[7].compute = Some(ComputeInfo { local_size: [8, 8, 1], work_groups: WorkGroups::Absolute { x: 64, y: 32, z: 1 }, indirect: None });
    programs[8].compute = Some(ComputeInfo { local_size: [1, 1, 1], work_groups: WorkGroups::Absolute { x: 0, y: 0, z: 0 }, indirect: Some((1, 0)) });

    let mut geometry = IndexMap::new();
    use GeometryProgram as G;
    for (g, p) in [(G::TerrainSolid, 0), (G::TerrainCutout, 0), (G::Water, 1), (G::SkyBasic, 2), (G::ShadowSolid, 3), (G::ShadowCutout, 3), (G::ShadowWater, 3)] {
        geometry.insert(g, GeometrySlot { program: p, resolved_from: g });
    }
    let flips = |set: &[usize]| {
        let mut v = vec![false; 7];
        for &i in set {
            v[i] = true;
        }
        v
    };
    let passes = vec![
        Pass { group: PassGroup::Shadow, index: 0, computes: vec![], program: None, flips_after: vec![], flip_state: flips(&[]) },
        Pass { group: PassGroup::ShadowComp, index: 0, computes: vec![], program: Some(4), flips_after: vec![], flip_state: flips(&[]) },
        Pass { group: PassGroup::GbuffersOpaque, index: 0, computes: vec![], program: None, flips_after: vec![], flip_state: flips(&[]) },
        Pass { group: PassGroup::GbuffersTranslucent, index: 0, computes: vec![], program: None, flips_after: vec![], flip_state: flips(&[]) },
        Pass { group: PassGroup::Composite, index: 0, computes: vec![7, 8], program: Some(5), flips_after: vec![0, 6], flip_state: flips(&[]) },
        Pass { group: PassGroup::Composite, index: 1, computes: vec![], program: Some(6), flips_after: vec![4], flip_state: flips(&[0, 6]) },
    ];
    let mut colortex = vec![ct(0, TextureFormat::RGBA16F), ct(3, TextureFormat::R32UI), ct(4, TextureFormat::RGBA8), ct(5, TextureFormat::RGBA16F), ct(6, TextureFormat::RGBA16F)];
    colortex[0].mipmap_programs = vec![5];
    colortex[4].clear = false;
    let mut frame = frame_layout();
    frame.members.retain(|m| m.name != "myCustom");
    let dim = DimensionPipeline {
        folder: String::new(),
        dimension_ids: vec!["*".into()],
        targets: RenderTargets {
            colortex,
            shadowcolor: vec![ct(0, TextureFormat::RGBA8)],
            shadow: ShadowSettings { enabled: true, resolution: 256, distance: 48.0, ..Default::default() },
            uses_depthtex1: false,
            uses_depthtex2: false,
            noise_texture_resolution: 32,
            noise_texture: Some(sb_core::model::TextureSource::PackImage { path: "textures/missing_noise.png".into() }),
            custom_textures: vec![
                CustomTexture { sampler: "mytex".into(), stage: "composite".into(), source: sb_core::model::TextureSource::PackImage { path: "textures/mytex.png".into() }, blur: true, clamp: true },
                CustomTexture {
                    sampler: "lut".into(),
                    stage: "composite".into(),
                    source: sb_core::model::TextureSource::Raw {
                        path: "textures/lut.bin".into(),
                        target: "3d".into(),
                        dimensions: 3,
                        format: TextureFormat::R8,
                        size: [4, 4, 4],
                        pixel_format: "RED".into(),
                        pixel_type: "UNSIGNED_BYTE".into(),
                    },
                    blur: false,
                    clamp: false,
                },
            ],
            images: vec![CustomImage { name: "myimg".into(), sampler_name: Some("myimgSampler".into()), format: TextureFormat::R32F, pixel_format: "RED".into(), pixel_type: "FLOAT".into(), clear: true, size: ImageSize::Absolute2D { width: 16, height: 16 } }],
            buffers: vec![
                StorageBuffer { index: 0, size: 16, relative: Some([0.25, 0.25]), file: None },
                StorageBuffer { index: 1, size: 64, relative: None, file: Some("data/cmd.bin".into()) },
            ],
        },
        settings: PackSettings::default(),
        uniforms: UniformLayout { frame, draw: draw_layout() },
        custom_uniforms: Vec::new(),
        bindings: table,
        programs,
        geometry,
        passes,
        gbuffer_attachments: Vec::new(),
        shadow_attachments: Vec::new(),
        end_of_frame_copies: vec![6],
        distant_horizons: DhPipeline::default(),
    };
    let pack = CompiledPack {
        format_version: sb_core::MODEL_FORMAT_VERSION,
        info: PackInfo {
            name: "coverage".into(),
            source_hash: String::new(),
            shaderbridge_version: sb_core::SHADERBRIDGE_VERSION.into(),
            features_enabled: Vec::new(),
            features_unsupported: Vec::new(),
            environment: CompileEnvironment { distant_horizons: false, ..Default::default() },
        },
        options: OptionsModel::default(),
        id_maps: IdMaps::default(),
        dimensions: vec![dim],
        diagnostics: Default::default(),
        blobs: Vec::new(),
    };
    let _ = GlslType::FLOAT;
    (pack, blobs)
}

fn pack_files(path: &str) -> Option<Vec<u8>> {
    match path {
        "textures/mytex.png" => {
            let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 255, 0, 255]));
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
            Some(png)
        }
        "textures/lut.bin" => Some(vec![255; 64]),
        "data/cmd.bin" => Some([1u32, 1, 1].iter().flat_map(|v| v.to_le_bytes()).collect()),
        _ => None,
    }
}

#[test]
fn feature_coverage_pack() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    let (pack, blobs) = coverage_pack();
    let files = pack_files;
    let out = rt
        .render(&RenderRequest {
            pack: &pack,
            blobs: &blobs,
            dimension: "",
            width: 160,
            height: 90,
            frames: 3,
            scene: small_scene(),
            depth_mode: DepthMode::ForwardZeroToOne,
            textures: &files,
            capture_targets: true,
        })
        .expect("render");
    out.image.save(render_dir().join("coverage.png")).ok();
    for (name, img) in &out.targets {
        img.save(render_dir().join(format!("coverage_{name}.png"))).ok();
    }
    eprintln!("{:#?}", out.stats);
    assert_no_validation_errors(&out);
    assert!(out.stats.programs_skipped.is_empty(), "{:?}", out.stats.programs_skipped);
    assert_eq!(out.stats.dispatches, 2);
    // The missing noise override and the 3D unknown sampler are reported.
    assert!(out.stats.warnings.iter().any(|w| w.contains("texture.noise")), "{:?}", out.stats.warnings);
    assert!(out.stats.warnings.iter().any(|w| w.contains("mystery3d")), "{:?}", out.stats.warnings);
    assert!(luminance_variance(&out.image) > 0.001, "{}", luminance_variance(&out.image));

    let target = |n: &str| &out.targets.iter().find(|(name, _)| name == n).unwrap_or_else(|| panic!("{n} not captured")).1;
    // Accumulation over 3 frames through the end-of-frame copy: 0.1 per frame.
    let acc = target("colortex6").get_pixel(80, 45);
    assert!((i32::from(acc[0]) - 77).abs() <= 3, "colortex6 = {acc:?}");
    // The compute pass wrote colorimg5.
    let c5 = target("colortex5").get_pixel(10, 10);
    assert_eq!([c5[0], c5[1], c5[2]], [64, 128, 191], "colortex5 = {c5:?}");
    // composite1 rendered with a half-size viewport: GL bottom-left quarter only
    // (captures are flipped to top-down, so that is the image's bottom-left).
    let c4 = target("colortex4");
    assert!(c4.get_pixel(20, 80)[3] > 0, "bottom-left of colortex4 is empty");
    assert_eq!(c4.get_pixel(150, 5)[3], 0, "colortex4 written outside the viewport");
    // The integer target holds block ids + 1 where terrain is.
    let ids = target("colortex3");
    assert!(ids.pixels().any(|p| p[0] > 0), "colortex3 is empty");
    // Shadowcomp wrote shadowcolor0 (alpha = 1 where the shadow map has geometry).
    assert!(target("shadowcolor0").pixels().any(|p| p[3] == 255) && target("shadowcolor0").pixels().any(|p| p[3] == 0));
}
