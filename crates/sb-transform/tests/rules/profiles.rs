//! ARCHITECTURE §6 / spec §1: draw profiles — every built-in profile, profile constants,
//! push constants, the Sodium terrain profile, host blocks/samplers/inputs, profile
//! globals and runtime-registered profiles.

use sb_core::ShaderStage;
use sb_core::model::{AlphaTest, OutputTarget};
use sb_core::program::AlphaFunc;
use sb_transform::{PackBuilder, builtin_profiles, parse_profile, profile};
use sb_uniforms::{ProgramClass, ResourceContext};

use crate::harness::*;

/// The minimal legacy pair of the spec.
const LEGACY_VS: &str = "#version 120\nvarying vec4 texcoord;\nvarying vec4 color;\nvoid main() {\n  gl_Position = ftransform();\n  texcoord = gl_TextureMatrix[0] * gl_MultiTexCoord0;\n  color = gl_Color;\n}\n";
const LEGACY_FS: &str = "#version 120\nuniform sampler2D texture;\nvarying vec4 texcoord;\nvarying vec4 color;\nvoid main() {\n  gl_FragData[0] = texture2D(texture, texcoord.st) * color;\n}\n";

/// A core-profile (Iris `va*` attributes) pair reading every attribute and matrix.
const CORE_VS: &str = "#version 330 core\n\
    in vec3 vaPosition;\nin vec4 vaColor;\nin vec2 vaUV0;\nin ivec2 vaUV1;\nin ivec2 vaUV2;\nin vec3 vaNormal;\n\
    in vec4 mc_Entity;\nin vec2 mc_midTexCoord;\nin vec4 at_tangent;\nin vec3 at_midBlock;\n\
    uniform mat4 modelViewMatrix;\nuniform mat4 projectionMatrix;\nuniform mat3 normalMatrix;\nuniform mat4 textureMatrix;\nuniform vec3 chunkOffset;\n\
    out vec2 uv;\nout vec2 light;\nout vec4 tint;\nflat out int blockId;\nout vec3 normal;\nout vec4 extra;\n\
    void main() {\n\
      gl_Position = projectionMatrix * modelViewMatrix * vec4(vaPosition + chunkOffset, 1.0);\n\
      uv = (textureMatrix * vec4(vaUV0, 0.0, 1.0)).xy;\n\
      light = vec2(vaUV2) / 240.0 + vec2(vaUV1) * 0.0;\n\
      tint = vaColor;\n\
      blockId = int(mc_Entity.x);\n\
      normal = normalMatrix * vaNormal;\n\
      extra = at_tangent + vec4(at_midBlock, 0.0) + vec4(mc_midTexCoord, 0.0, 0.0);\n\
    }\n";
const CORE_FS: &str = "#version 330 core\nuniform sampler2D gtexture;\nuniform sampler2D lightmap;\n\
    in vec2 uv;\nin vec2 light;\nin vec4 tint;\nflat in int blockId;\nin vec3 normal;\nin vec4 extra;\n\
    /* RENDERTARGETS: 0,2 */\nlayout(location = 0) out vec4 color;\nlayout(location = 1) out vec4 data;\n\
    void main() {\n\
      color = texture(gtexture, uv) * texture(lightmap, light) * tint;\n\
      data = vec4(normal * 0.5 + 0.5, float(blockId)) + extra * 0.0;\n\
    }\n";

#[test]
fn every_builtin_profile_translates_legacy_and_core_programs() {
    assert!(builtin_profiles().len() >= 13);
    for p in builtin_profiles() {
        for target in [OutputTarget::Vulkan, OutputTarget::Renderpearl] {
            for (vs, fs) in [(LEGACY_VS, LEGACY_FS), (CORE_VS, CORE_FS)] {
                let out = T::new(&p.name)
                    .with(|o| {
                        o.target = target;
                        if !p.fullscreen {
                            o.alpha_test = Some(AlphaTest { func: AlphaFunc::Greater, reference: 0.1 });
                        }
                    })
                    .vs(vs)
                    .fs(fs)
                    .run();
                assert_eq!(out.prog.stages.len(), 2, "{}", p.name);
                if p.fullscreen || p.inputs.is_empty() {
                    assert!(out.prog.vertex_inputs.is_empty(), "{}: {:?}", p.name, out.prog.vertex_inputs);
                } else {
                    assert!(!out.prog.vertex_inputs.is_empty(), "{}", p.name);
                    // Every reported input is declared with the profile's location and type.
                    for i in &out.prog.vertex_inputs {
                        let pi = p.inputs.iter().find(|x| x.name == i.name).unwrap_or_else(|| panic!("{}: {}", p.name, i.name));
                        assert_eq!((pi.location, &pi.ty), (i.location, &i.ty));
                        let refl = out.refl(ShaderStage::Vertex).inputs.iter().find(|r| r.name == i.name);
                        if let Some(r) = refl {
                            assert_eq!(r.location, i.location, "{}: {}", p.name, i.name);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn fullscreen_profile_generates_a_quad_without_inputs() {
    let vs = "#version 120\nvarying vec2 uv;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.st; gl_FrontColor = gl_Color; }\n";
    let out = T::fullscreen().vs(vs).fs("#version 120\nvarying vec2 uv;\nvoid main() { gl_FragColor = vec4(uv, 0.0, 1.0) * gl_Color; }\n").run();
    let v = out.vs();
    contains_all(
        v,
        &[
            "int i = gl_VertexIndex % 6;",
            "vec2(float(i == 1 || i == 2 || i == 4), float(i == 2 || i == 4 || i == 5))",
            "mat4 sb_Projection = mat4(2.0, 0.0, 0.0, 0.0,  0.0, 2.0, 0.0, 0.0,  0.0, 0.0, 0.0, 0.0,  -1.0, -1.0, 0.0, 1.0);",
            "mat4 sb_ModelView = mat4(1.0);",
            "vec4 sb_gl_Color = vec4(1.0);",
        ],
    );
    assert!(out.prog.vertex_inputs.is_empty());
    assert!(out.refl(ShaderStage::Vertex).inputs.is_empty());
}

#[test]
fn profile_constants_precede_the_helper_code() {
    let vs = "#version 120\nattribute vec4 mc_Entity;\nvarying float id;\nvoid main() { gl_Position = ftransform(); id = mc_Entity.x; }\n";
    let fs = "#version 120\nvarying float id;\nvoid main() { gl_FragData[0] = vec4(id); }\n";
    let out = T::new("dh_terrain")
        .with(|o| {
            o.profile_constants.insert("SB_DH_BLOCK_ID_1".into(), 18);
            o.profile_constants.insert("SB_DH_BLOCK_ID_13".into(), 2);
        })
        .vs(vs)
        .fs(fs)
        .run();
    let v = out.vs();
    contains_all(v, &["const int SB_DH_BLOCK_ID_1 = 18;", "const int SB_DH_BLOCK_ID_13 = 2;", "const int SB_DH_BLOCK_ID_2 = -1;"]);
    assert!(v.find("const int SB_DH_BLOCK_ID_1 =").unwrap() < v.find("int sb_dhPackBlockId(uint m)").unwrap());
}

#[test]
fn host_blocks_samplers_and_inputs_are_declared_only_when_used() {
    let vs = "#version 120\nvarying vec4 c;\nvoid main() { gl_Position = gl_ProjectionMatrix * gl_ModelViewMatrix * gl_Vertex; c = gl_Color; }\n";
    let fs = "#version 120\nvarying vec4 c;\nvoid main() { gl_FragData[0] = c; }\n";
    let out = T::gbuffers().vs(vs).fs(fs).run();
    let v = out.vs();
    contains_all(v, &["uniform TerrainUniform {", "uniform Globals {", "in vec3 Position;", "in vec4 Color;", "in ivec3 ChunkPosition;"]);
    contains_none(v, &["UV0", "UV2", "sb_Normal", "Sampler0", "Sampler2"]);
    contains_none(out.fs(), &["TerrainUniform", "Globals", "Sampler0"]);
    let names: Vec<(&str, u32)> = out.prog.vertex_inputs.iter().map(|i| (i.name.as_str(), i.location)).collect();
    assert_eq!(names, [("Position", 0), ("Color", 1), ("ChunkPosition", 4)]);
    let sem: Vec<Option<&str>> = out.prog.vertex_inputs.iter().map(|i| i.semantic.as_deref()).collect();
    assert_eq!(sem, [Some("position"), Some("color"), Some("position")]);
    assert!(out.prog.resources_used.iter().any(|(n, s)| n == "TerrainUniform" && s == &[ShaderStage::Vertex]), "{:?}", out.prog.resources_used);

    // Pack samplers satisfied by host samplers are renamed to the host names.
    let fs = "#version 120\nuniform sampler2D texture;\nuniform sampler2D lightmap;\nvarying vec4 c;\nvoid main() { gl_FragData[0] = texture2D(texture, c.xy) * texture2D(lightmap, c.zw); }\n";
    let out = T::gbuffers().vs(vs).fs(fs).run();
    contains_all(out.fs(), &["uniform sampler2D Sampler0;", "uniform sampler2D Sampler2;", "texture(Sampler0, c.xy) * texture(Sampler2, c.zw)"]);
    contains_none(out.fs(), &["gtexture", "uniform sampler2D lightmap"]);
}

#[test]
fn vertex_attributes_follow_the_profile_semantics() {
    let vs = "#version 120\nattribute vec4 mc_Entity;\nattribute vec2 mc_midTexCoord;\nattribute vec4 at_tangent;\nvarying vec4 v;\n\
              void main() { gl_Position = ftransform(); v = vec4(mc_Entity.xy, mc_midTexCoord) + at_tangent + vec4(gl_Normal, 0.0) + gl_MultiTexCoord1; }\n";
    let fs = "#version 120\nvarying vec4 v;\nvoid main() { gl_FragData[0] = v; }\n";
    let out = T::gbuffers().vs(vs).fs(fs).run();
    contains_all(
        out.vs(),
        &[
            "vec4 sb_mc_Entity = vec4(float(sb_Entity.x), float(sb_Entity.y), 0.0, 1.0);",
            "vec4 mc_Entity = sb_mc_Entity;",
            "vec2 mc_midTexCoord = vec2((sb_mc_midTexCoord).xy);",
            "vec4 sb_gl_MultiTexCoord1 = vec4(vec2(UV2), 0.0, 1.0);",
        ],
    );
    let names: Vec<&str> = out.prog.vertex_inputs.iter().map(|i| i.name.as_str()).collect();
    for n in ["sb_Entity", "sb_MidTexCoord", "sb_Tangent", "sb_Normal", "UV2"] {
        assert!(names.contains(&n), "{n} missing from {names:?}");
    }
}

#[test]
fn sodium_terrain_profile_decodes_the_compact_format_with_push_constants() {
    let vs = "#version 120\nattribute vec4 mc_Entity;\nattribute vec4 at_midBlock;\nvarying vec2 uv;\nvarying vec2 lm;\nvarying vec4 v;\n\
              void main() {\n  gl_Position = ftransform();\n  uv = (gl_TextureMatrix[0] * gl_MultiTexCoord0).xy;\n  lm = (gl_TextureMatrix[1] * gl_MultiTexCoord1).xy;\n  v = mc_Entity + at_midBlock + vec4(gl_Normal, 0.0) + gl_Color;\n}\n";
    let fs = "#version 120\nuniform sampler2D texture;\nuniform sampler2D lightmap;\nvarying vec2 uv;\nvarying vec2 lm;\nvarying vec4 v;\nvoid main() { gl_FragData[0] = texture2D(texture, uv) * texture2D(lightmap, lm) * v; }\n";
    for target in [OutputTarget::Vulkan, OutputTarget::Renderpearl] {
        let out = T::new("sodium_terrain").with(|o| o.target = target).vs(vs).fs(fs).run();
        let v = out.vs();
        contains_all(
            v,
            &[
                "layout(push_constant) uniform sb_hPush { vec3 u_RegionOffset; int u_CurrentTime; uint u_RegionID; };",
                "uniform u_Globals {",
                "vec3 sb_sodiumDeinterleave(uvec2 v)",
                "vec4 sb_gl_MultiTexCoord1 = vec4(max(vec2(a_LightAndData.xy) - 8.0, vec2(0.0)), 0.0, 1.0);",
                "vec4 sb_mc_Entity = vec4(float(int(sb_Entity >> 1u) - 1), float(sb_Entity & 1u), 0.0, 1.0);",
                "vec4 sb_at_midBlock = sb_MidBlock * 127.0;",
                "mat4 sb_Projection = gbufferProjection;",
            ],
        );
        contains_all(out.fs(), &["uniform sampler2D u_BlockTex;", "uniform sampler2D u_LightTex;"]);
        contains_none(out.fs(), &["push_constant"]);
        // Push constants: std430 offsets 0, 12, 16 (20 bytes, as Sodium pushes them).
        let pc = out.refl(ShaderStage::Vertex).push_constants.clone().expect("push constant block");
        let offsets: Vec<(String, u32)> = pc.members.iter().map(|m| (m.name.clone(), m.offset)).collect();
        assert_eq!(offsets, [("u_RegionOffset".to_string(), 0), ("u_CurrentTime".to_string(), 12), ("u_RegionID".to_string(), 16)]);
        assert_eq!(out.refl(ShaderStage::Vertex).push_constant_size, 20);
        assert_eq!(out.refl(ShaderStage::Fragment).push_constant_size, 0);
        let names: Vec<(&str, u32, &str)> = out.prog.vertex_inputs.iter().map(|i| (i.name.as_str(), i.location, i.ty.as_str())).collect();
        for want in [("a_Position", 0, "uvec2"), ("a_Color", 1, "vec4"), ("a_TexCoord", 2, "uvec2"), ("a_LightAndData", 3, "uvec4"), ("sb_Entity", 4, "uint"), ("sb_Normal", 5, "vec4"), ("sb_MidBlock", 7, "vec4")] {
            assert!(names.contains(&want), "{want:?} missing from {names:?}");
        }
        assert!(!names.iter().any(|n| n.0 == "sb_MidTexCoord"), "{names:?}");
    }
}

#[test]
fn sodium_chunk_offset_and_core_profile_positions() {
    // Core-profile packs add chunkOffset to vaPosition: the sum is the camera-relative
    // position, so vaPosition is section-local.
    let out = T::new("sodium_terrain").vs(CORE_VS).fs(CORE_FS).run();
    contains_all(
        out.vs(),
        &["vec3 sb_ChunkOffset = (u_RegionOffset + sb_sodiumDrawTranslation());", "vec3 vaPosition = sb_gl_Vertex.xyz - sb_ChunkOffset;"],
    );
}

fn custom_profile() -> sb_transform::DrawProfile {
    parse_profile(
        r#"
name = "test_custom"
description = "A host draw path registered at runtime."
push_constants = "vec4 pc_Tint; float pc_Time;"
[[inputs]]
name = "inPos"
type = "vec3"
location = 0
[[inputs]]
name = "inUv"
type = "vec2"
location = 3
[[blocks]]
name = "HostData"
instance = "sb_hHost"
members = "mat4 Proj; mat4 View;"
[[samplers]]
name = "HostAtlas"
provides = ["gtexture"]
[semantics]
position = "vec4(inPos, 1.0)"
uv0 = "vec4(inUv, 0.0, 1.0)"
color = "pc_Tint"
model_view = "sb_hHost.View"
projection = "sb_hHost.Proj"
[[globals]]
name = "hostTime"
type = "float"
init = "pc_Time"
varying = true
[code]
fragment = """
vec4 sb_hostFade(vec4 c) { return c * pc_Tint.a; }
"""
"#,
    )
    .expect("custom profile")
}

#[test]
fn runtime_profiles_with_push_constants_in_several_stages() {
    let p = custom_profile();
    assert_eq!(p.push_constants, "vec4 pc_Tint; float pc_Time;");
    let vs = "#version 120\nvarying vec2 uv;\nvarying vec4 c;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.xy; c = gl_Color; }\n";
    let fs = "#version 120\nuniform sampler2D texture;\nuniform float hostTime;\nvarying vec2 uv;\nvarying vec4 c;\nvoid main() { gl_FragData[0] = texture2D(texture, uv) * c * pc_Tint * hostTime; }\n";
    let out = T::custom(p).vs(vs).fs(fs).run();
    for stage in [ShaderStage::Vertex, ShaderStage::Fragment] {
        contains_all(out.glsl(stage), &["layout(push_constant) uniform sb_hPush { vec4 pc_Tint; float pc_Time; };"]);
        assert_eq!(out.refl(stage).push_constant_size, 20, "{stage}");
    }
    contains_all(out.vs(), &["vec4 sb_gl_Color = pc_Tint;", "float hostTime = pc_Time;", "uniform HostData {"]);
    contains_all(out.fs(), &["uniform sampler2D HostAtlas;", "in float hostTime;", "texture(HostAtlas, uv)"]);
    let names: Vec<&str> = out.prog.vertex_inputs.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["inPos", "inUv"]);
}

#[test]
fn invalid_push_constant_declarations_are_rejected() {
    for bad in [
        "name = \"x\"\npush_constants = \"vec4 a\"\n",
        "name = \"x\"\npush_constants = \"sampler2D s;\"\n",
        "name = \"x\"\npush_constants = \"   \"\n[[samplers]]\nname = \"s\"\n",
        "name = \"x\"\npush_constants = \"vec4 s;\"\n[[samplers]]\nname = \"s\"\n",
        "name = \"x\"\npush_constants = \"vec4 a; float a;\"\n",
    ] {
        let r = parse_profile(bad);
        // An all-blank member list means "no push constants".
        if bad.contains("\"   \"") {
            assert!(r.is_ok(), "{bad}: {r:?}");
        } else {
            assert!(r.is_err(), "{bad}");
        }
    }
}

#[test]
fn world_space_profiles_add_shadow_matrices_to_the_layout() {
    let rc = ResourceContext::new(ProgramClass::Gbuffers);
    for name in ["vanilla_terrain", "vanilla_terrain_basic", "dh_terrain", "sodium_terrain"] {
        let p = profile(name).unwrap();
        assert!(p.world_space, "{name}");
        let mut b = PackBuilder::new(rc.clone());
        b.add_profile(p, ProgramClass::Gbuffers);
        let data = b.finish();
        for m in ["shadowModelView", "shadowProjection"] {
            assert!(data.layout.frame.member(m).is_some(), "{name}: {m}");
        }
    }
    for name in ["vanilla_entity", "vanilla_particle", "fullscreen", "dh_generic"] {
        assert!(!profile(name).unwrap().world_space, "{name}");
    }
    assert!(profile("sodium_terrain").unwrap().referenced_builtins().contains(&"gbufferProjection".to_string()));
}

#[test]
fn profile_globals_replace_pack_declarations_and_reach_later_stages() {
    // vanilla_entity's `entityColor` is a varying profile global (Iris EntityPatcher).
    let vs = "#version 120\nuniform vec4 entityColor;\nvarying vec4 c;\nvoid main() { gl_Position = ftransform(); c = gl_Color * entityColor.a; }\n";
    let fs = "#version 120\nuniform vec4 entityColor;\nvarying vec4 c;\nvoid main() { gl_FragData[0] = mix(c, entityColor, entityColor.a); }\n";
    let out = T::new("vanilla_entity").vs(vs).fs(fs).run();
    assert!(out.has_diag("xf.profile-global"));
    contains_all(out.vs(), &["vec4 entityColor = sb_entityOverlayColor();", "sb_vary_entityColor = entityColor;"]);
    contains_all(out.fs(), &["in vec4 entityColor;"]);
    contains_none(out.fs(), &["uniform vec4 entityColor"]);

    // DH fragment helpers read forwarded vertex globals.
    let vs = "#version 330\nin vec3 vaPosition;\nvoid main() { gl_Position = vec4(vaPosition, 1.0); }\n";
    let fs = "#version 330\nout vec4 c;\nvoid main() { c = dh_hasTexture() ? dh_sampleTexture() : vec4(1.0); }\n";
    let out = T::new("dh_terrain").vs(vs).fs(fs).run();
    contains_all(out.fs(), &["flat in uint sb_dhTextureTile;", "in vec3 sb_dhBlockPos;", "flat in uint sb_dhNormalIndex;", "uniform sampler2D uBlockAtlas;"]);
    contains_all(out.vs(), &["sb_vary_sb_dhTextureTile = sb_dhTextureTile;"]);
}

#[test]
fn pack_names_that_collide_with_profile_names_are_renamed() {
    let vs = "#version 120\nvarying vec4 Color;\nfloat Position(float x) { return x * 2.0; }\nvoid main() { vec3 UV0 = vec3(Position(1.0)); gl_Position = ftransform() + vec4(UV0, 0.0); Color = gl_Color; }\n";
    let fs = "#version 120\nvarying vec4 Color;\nvoid main() { gl_FragData[0] = Color; }\n";
    let out = T::gbuffers().vs(vs).fs(fs).run();
    contains_all(out.vs(), &["out vec4 sbu_Color;", "float sbu_Position(float x)", "vec3 sbu_UV0 = vec3(sbu_Position(1.0));", "in vec3 Position;"]);
    contains_all(out.fs(), &["in vec4 sbu_Color;"]);
}

#[test]
fn alpha_test_uses_the_draw_block_member_in_every_gbuffers_profile() {
    let fs = "#version 120\nvarying vec4 texcoord;\nvarying vec4 color;\nvoid main() { gl_FragData[0] = color; }\n";
    for name in ["vanilla_terrain", "vanilla_entity", "vanilla_particle", "dh_terrain", "sodium_terrain"] {
        let out = T::new(name)
            .with(|o| o.alpha_test = Some(AlphaTest { func: AlphaFunc::Greater, reference: 0.1 }))
            .vs(LEGACY_VS)
            .fs(fs)
            .run();
        contains_all(out.fs(), &["if (!(sb_FragData0.a > alphaTestRef)) discard;"]);
        assert!(out.prog.draw_members_used.iter().any(|m| m == "alphaTestRef"), "{name}");
    }
}

#[test]
fn vertex_inputs_declared_like_host_attributes_read_them() {
    // Same name and type as a profile input: the host attribute; another type: renamed
    // (it is not the host's attribute) and zero with a warning.
    let vs = "#version 330\nin vec3 Position;\nin vec2 UV2;\nout vec4 v;\nvoid main() { gl_Position = vec4(Position, 1.0); v = vec4(UV2, 0.0, 1.0); }\n";
    let out = T::gbuffers().vs(vs).fs("#version 330\nin vec4 v;\nout vec4 c;\nvoid main() { c = v; }\n").run();
    contains_all(out.vs(), &["layout(location = 0) in vec3 Position;", "gl_Position = vec4(Position, 1.0);", "vec2 sbu_UV2 = vec2(0);"]);
    assert!(out.has_diag("xf.unknown-attribute"));
    assert!(out.prog.vertex_inputs.iter().any(|i| i.name == "Position" && i.location == 0));
}

#[test]
fn dh_generic_provides_iris_texture_stubs() {
    // Packs without a dh_generic program reuse their dh_terrain fragment shader, which
    // calls the DH texture helpers (Iris DHGenericTransformer injects these stubs).
    let vs = "#version 330\nin vec3 vaPosition;\nvoid main() { gl_Position = vec4(vaPosition, 1.0); }\n";
    let fs = "#version 330\nout vec4 c;\nvoid main() { c = dh_hasTexture() ? dh_sampleTexture() : vec4(0.5); }\n";
    let out = T::new("dh_generic").vs(vs).fs(fs).run();
    contains_all(out.fs(), &["bool dh_hasTexture()", "return false;", "vec4 dh_sampleTexture()"]);
}

/// Vertex-stage expression a profile feeds a compatibility builtin with (the generated
/// semantic global's initializer).
fn semantic_init(out: &Out, global: &str) -> String {
    let v = out.vs();
    let start = v.find(&format!(" {global} = ")).unwrap_or_else(|| panic!("no `{global}` in\n{v}"));
    let rest = &v[start + global.len() + 4..];
    rest[..rest.find(";\n").unwrap()].to_string()
}

#[test]
fn builtins_missing_from_a_vertex_format_read_iris_defaults() {
    // Iris VanillaTransformer: formats without normals read gl_Normal = (0, 0, 1) (GL's
    // initial normal); formats without texture coordinates read (0.5, 0.5, 0, 1).
    let vs = "#version 120\nvarying vec4 v;\nvoid main() { gl_Position = ftransform(); v = vec4(gl_Normal, 1.0) + gl_MultiTexCoord0; }\n";
    let fs = "#version 120\nvarying vec4 v;\nvoid main() { gl_FragData[0] = v; }\n";
    for (p, normal, uv0) in [
        ("vanilla_particle", "vec3(0.0, 0.0, 1.0)", "vec4(UV0, 0.0, 1.0)"),
        ("vanilla_position", "vec3(0.0, 0.0, 1.0)", "vec4(0.5, 0.5, 0.0, 1.0)"),
        ("vanilla_position_color", "vec3(0.0, 0.0, 1.0)", "vec4(0.5, 0.5, 0.0, 1.0)"),
        ("vanilla_position_tex", "vec3(0.0, 0.0, 1.0)", "vec4(UV0, 0.0, 1.0)"),
        ("vanilla_position_tex_color", "vec3(0.0, 0.0, 1.0)", "vec4(UV0, 0.0, 1.0)"),
        ("vanilla_entity", "Normal", "vec4(UV0, 0.0, 1.0)"),
        ("fullscreen", "vec3(0.0, 0.0, 1.0)", "vec4(sb_fullscreenUv(), 0.0, 1.0)"),
    ] {
        let out = T::new(p).vs(vs).fs(fs).run();
        assert_eq!(semantic_init(&out, "sb_gl_Normal"), normal, "{p}");
        assert_eq!(semantic_init(&out, "sb_gl_MultiTexCoord0"), uv0, "{p}");
    }
}

#[test]
fn mc_entity_is_the_block_id_only_in_terrain_formats() {
    // Iris binds mc_Entity only in terrain formats; elsewhere the attribute is unbound and
    // reads GL's default (0, 0, 0, 1). Entity ids (entity.properties, read through
    // `entityId`) share their numbers with block ids, so mc_Entity.x must not carry them:
    // packs that classify materials by mc_Entity.x in shared code (mellow) would
    // misclassify entities.
    let vs = "#version 120\nattribute vec4 mc_Entity;\nvarying vec4 v;\nvoid main() { gl_Position = ftransform(); v = mc_Entity; }\n";
    let fs = "#version 120\nvarying vec4 v;\nvoid main() { gl_FragData[0] = v; }\n";
    for (p, entity) in [
        ("vanilla_entity", "vec4(0.0, 0.0, 0.0, 1.0)"),
        ("vanilla_particle", "vec4(0.0, 0.0, 0.0, 1.0)"),
        ("vanilla_position_tex_color", "vec4(0.0, 0.0, 0.0, 1.0)"),
        ("dh_generic", "vec4(0.0, 0.0, 0.0, 1.0)"),
        ("vanilla_terrain", "vec4(float(sb_Entity.x), float(sb_Entity.y), 0.0, 1.0)"),
    ] {
        let out = T::new(p).vs(vs).fs(fs).run();
        assert_eq!(semantic_init(&out, "sb_mc_Entity"), entity, "{p}");
        contains_none(out.vs(), &["entityId"]);
    }
}

#[test]
fn fullscreen_lightmap_coordinates_follow_iris_composite() {
    // Iris CompositeTransformer: gl_MultiTexCoord1..7 = vec4(0, 0, 0, 1) and every
    // gl_TextureMatrix[i] is the identity.
    let vs = "#version 120\nvarying vec2 lm;\nvoid main() { gl_Position = ftransform(); lm = (gl_TextureMatrix[1] * gl_MultiTexCoord1).st + (gl_TextureMatrix[2] * gl_MultiTexCoord2).st; }\n";
    let fs = "#version 120\nvarying vec2 lm;\nvoid main() { gl_FragData[0] = vec4(lm, 0.0, 1.0); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    assert_eq!(semantic_init(&out, "sb_gl_MultiTexCoord1"), "vec4(0.0, 0.0, 0.0, 1.0)");
    assert_eq!(semantic_init(&out, "sb_LightmapMatrix"), "mat4(1.0)");
    // World geometry keeps the OptiFine lightmap matrix (scale 1/256, offset 1/32).
    let out = T::gbuffers().vs(vs).fs(fs).run();
    assert!(semantic_init(&out, "sb_LightmapMatrix").contains("vec4(0.03125, 0.03125, 0.03125, 1.0)"));
}

#[test]
fn synthesized_dh_programs_read_the_terrain_lightmap_convention() {
    // Native DH programs (Iris DHTerrainTransformer) read pre-normalized lightmap
    // coordinates ((level + 0.5) / 16) with an identity gl_TextureMatrix[1]; programs
    // synthesized from gbuffers_terrain were written for 16 * level (0..240) and the
    // OptiFine matrix, and some scale the raw value (`/ 240.0`, `vaUV2`).
    let vs = "#version 120\nvarying vec2 lm;\nvarying vec2 raw;\nvoid main() { gl_Position = ftransform(); lm = (gl_TextureMatrix[1] * gl_MultiTexCoord1).st; raw = gl_MultiTexCoord1.st / 240.0; }\n";
    let fs = "#version 120\nvarying vec2 lm;\nvarying vec2 raw;\nvoid main() { gl_FragData[0] = vec4(lm, raw); }\n";
    let native = T::new("dh_terrain").vs(vs).fs(fs).run();
    assert_eq!(semantic_init(&native, "sb_gl_MultiTexCoord1"), "vec4((float((meta >> 4u) & 15u) + 0.5) / 16.0, (float(meta & 15u) + 0.5) / 16.0, 0.0, 1.0)");
    assert_eq!(semantic_init(&native, "sb_LightmapMatrix"), "mat4(1.0)");
    let synth = T::new(sb_transform::DH_SYNTH_PROFILE).vs(vs).fs(fs).run();
    assert_eq!(semantic_init(&synth, "sb_gl_MultiTexCoord1"), "vec4(float((meta >> 4u) & 15u) * 16.0, float(meta & 15u) * 16.0, 0.0, 1.0)");
    assert!(semantic_init(&synth, "sb_LightmapMatrix").contains("vec4(0.03125, 0.03125, 0.03125, 1.0)"));
    // Through the matrix both give Iris's value exactly: 16 L / 256 + 1 / 32 = (L + 0.5) / 16.
    for level in 0..16u8 {
        let l = f32::from(level);
        assert_eq!(16.0 * l * 0.003_906_25 + 0.031_25, (l + 0.5) / 16.0);
    }
    // Core code reads vaUV2 in 0..240 units too.
    let core = "#version 330\nin ivec2 vaUV2;\nout vec2 lm;\nvoid main() { gl_Position = vec4(0.0); lm = vec2(vaUV2) / 240.0; }\n";
    let out = T::new(sb_transform::DH_SYNTH_PROFILE).vs(core).fs("#version 330\nin vec2 lm;\nout vec4 c;\nvoid main() { c = vec4(lm, 0.0, 1.0); }\n").run();
    contains_all(out.vs(), &["ivec2 vaUV2 = ivec2((sb_gl_MultiTexCoord1).xy);", "float((meta >> 4u) & 15u) * 16.0"]);
    // Everything else is dh_terrain's.
    let (a, b) = (sb_transform::profile("dh_terrain").unwrap(), sb_transform::profile(sb_transform::DH_SYNTH_PROFILE).unwrap());
    assert_eq!((&a.inputs, &a.blocks, &a.samplers, &a.globals, &a.code_vertex), (&b.inputs, &b.blocks, &b.samplers, &b.globals, &b.code_vertex));
    assert_eq!(a.semantics.position, b.semantics.position);
}

#[test]
fn dh_generic_normals_follow_the_box_face() {
    // DH generic boxes are 24 vertices (north, south, west, east, bottom, top faces of 4
    // vertices each): Iris DHGenericTransformer and DH's own shading derive the face
    // from the vertex index.
    let vs = "#version 120\nvarying vec3 n;\nvoid main() { gl_Position = ftransform(); n = gl_Normal; }\n";
    let fs = "#version 120\nvarying vec3 n;\nvoid main() { gl_FragData[0] = vec4(n, 1.0); }\n";
    let out = T::new("dh_generic").vs(vs).fs(fs).run();
    assert_eq!(semantic_init(&out, "sb_gl_Normal"), "sb_dhGenericNormal()");
    contains_all(
        out.vs(),
        &["int face = gl_VertexIndex % 24 / 4;", "if (face == 0) {\n        return vec3(0.0, 0.0, -1.0);", "if (face == 4) {\n        return vec3(0.0, -1.0, 0.0);", "return vec3(0.0, 1.0, 0.0);"],
    );
}
