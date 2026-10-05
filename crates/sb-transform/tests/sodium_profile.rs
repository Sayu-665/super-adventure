//! The `sodium_terrain` draw profile against Sodium's real terrain interface.
//!
//! Sodium 0.9.2 and 0.9.3-alpha.1 for Minecraft 26.3 (checked with `javap` and against
//! their `assets/sodium/shaders`): `CompactChunkVertex.VERTEX_FORMAT`, the declarations of
//! `chunk_vertex.glsl`, `globals.glsl` and `block_layer_opaque.vsh`, `ShaderChunkRenderer`'s
//! bind groups and `DefaultChunkRenderer`'s 20-byte push constants. The extension attributes
//! after Sodium's 20 bytes are ShaderBridge's: the Java host meshes them into Sodium's
//! buffers (`dev.shaderbridge.compat.sodium.TerrainVertexLayout`), and `sb-runtime` writes
//! the same layout (`scene::formats::SODIUM_TERRAIN`). A change on either side must show
//! up here.

use sb_transform::profile;

/// Sodium's compact chunk vertex (offsets 0, 8, 12, 16; 20 bytes) as its shader declares it,
/// then ShaderBridge's extension (offsets 20, 24, 28, 32; 36 bytes).
const INPUTS: [(&str, &str, u32); 8] = [
    // RG32_UINT: 20-bit position, high 10 bits per axis in .x, low 10 bits in .y.
    ("a_Position", "uvec2", 0),
    // RGBA8_UNORM: colour times ambient occlusion.
    ("a_Color", "vec4", 1),
    // RG16_UINT: 15-bit texture coordinate and the bias sign in bit 15.
    ("a_TexCoord", "uvec2", 2),
    // RGBA8_UINT: block light, sky light (16 L + 8), material bits, section index.
    ("a_LightAndData", "uvec4", 3),
    // R32_UINT: ((block id + 1) << 1) | is fluid.
    ("sb_Entity", "uint", 4),
    // RGBA8_SNORM: face normal.
    ("sb_Normal", "vec4", 5),
    // RG16_UINT: quad texture centre * 32768.
    ("sb_MidTexCoord", "uvec2", 6),
    // RGBA8_SNORM: (block centre - vertex) * 64 as bytes, w = light emission.
    ("sb_MidBlock", "vec4", 7),
];

#[test]
fn vertex_inputs_are_sodiums_compact_vertex_then_the_extension() {
    let p = profile("sodium_terrain").expect("built-in sodium_terrain profile");
    let inputs: Vec<(&str, &str, u32)> = p.inputs.iter().map(|i| (i.name.as_str(), i.ty.as_str(), i.location)).collect();
    assert_eq!(inputs, INPUTS);
    assert!(p.inputs.iter().all(|i| !i.instanced));
    assert!(p.world_space);
    assert!(!p.fullscreen);
}

#[test]
fn push_constants_are_the_ones_sodium_pushes_per_region() {
    // DefaultChunkRenderer.render: float x, y, z at 0, 4, 8 (region origin - camera),
    // int now - region creation time at 12, int region id at 16; PUSH_CONSTANT_RANGE = 20.
    // block_layer_opaque.vsh: layout(push_constant) uniform PC { vec3 u_RegionOffset; int
    // u_CurrentTime; uint u_RegionID; } (std430: offsets 0, 12, 16).
    let p = profile("sodium_terrain").unwrap();
    assert_eq!(p.push_constants, "vec3 u_RegionOffset; int u_CurrentTime; uint u_RegionID;");
}

#[test]
fn host_blocks_and_samplers_are_sodiums_bind_groups() {
    // ShaderChunkRenderer.BIND_GROUP = { u_BlockTex sampler, u_Globals UBO, u_SectionTimeInfo
    // R32_SINT texel buffer }, LIGHT_GROUP = { u_LightTex sampler }.
    let p = profile("sodium_terrain").unwrap();
    assert_eq!(p.blocks.len(), 1);
    let globals = &p.blocks[0];
    assert_eq!(globals.name, "u_Globals");
    // globals.glsl, std140, in Sodium's order (UniformBufferManager writes the same order).
    let members: Vec<&str> = globals.members.split(';').map(str::trim).filter(|m| !m.is_empty()).collect();
    assert_eq!(
        members,
        [
            "mat4 u_ProjectionMatrix",
            "mat4 u_ModelViewMatrix",
            "vec4 u_FogColor",
            "vec2 u_EnvironmentFog",
            "vec2 u_RenderFog",
            "vec2 u_TexelSize",
            "vec2 u_TexCoordShrink",
            "float u_FadePeriodInv",
            "bool u_UseRGSS",
        ]
    );
    let samplers: Vec<(&str, &str, Vec<&str>)> =
        p.samplers.iter().map(|s| (s.name.as_str(), s.ty.as_str(), s.provides.iter().map(String::as_str).collect())).collect();
    assert_eq!(samplers, [("u_BlockTex", "sampler2D", vec!["gtexture"]), ("u_LightTex", "sampler2D", vec!["lightmap"])]);
    // u_SectionTimeInfo is not declared: programs do not read Sodium's fade-in times.
    assert!(!p.code_vertex.contains("u_SectionTimeInfo") && p.globals.iter().all(|g| !g.init.contains("u_SectionTimeInfo")));
}

#[test]
fn semantics_decode_sodiums_encoding() {
    let p = profile("sodium_terrain").unwrap();
    let code = p.code_vertex.as_str();
    // _deinterleave_u20x3 and VERTEX_SCALE / VERTEX_OFFSET of chunk_vertex.glsl.
    for needle in [
        "(uvec3(v.x) >> uvec3(0u, 10u, 20u)) & 0x3FFu",
        "(uvec3(v.y) >> uvec3(0u, 10u, 20u)) & 0x3FFu",
        "(hi << 10u) | lo",
        "* (32.0 / 1048576.0) - 8.0 + u_RegionOffset + sb_sodiumDrawTranslation()",
        // LocalSectionIndex: x = bits 5..7, y = bits 0..1, z = bits 2..4, times 16 blocks.
        "vec3(float((w >> 5u) & 7u), float(w & 3u), float((w >> 2u) & 7u)) * 16.0",
        // _get_texcoord / _get_texcoord_bias and v_TexCoord of block_layer_opaque.vsh.
        "vec2(a_TexCoord & 0x7FFFu) / 32768.0",
        "mix(vec2(-1.0), vec2(1.0), bvec2(a_TexCoord >> 15u))",
        "bias * sb_hSodium.u_TexCoordShrink + coord",
    ] {
        assert!(code.contains(needle), "missing `{needle}` in\n{code}");
    }
    assert_eq!(p.semantic("position"), "vec4(sb_sodiumPosition(), 1.0)");
    assert_eq!(p.semantic("color"), "a_Color");
    assert_eq!(p.semantic("uv0"), "vec4(sb_sodiumTexCoord(), 0.0, 1.0)");
    // CompactChunkVertex.encodeLight stores clamp(16 L + 8, 8, 248); packs expect 16 L.
    assert_eq!(p.semantic("lightmap"), "vec4(max(vec2(a_LightAndData.xy) - 8.0, vec2(0.0)), 0.0, 1.0)");
    // Sodium's u_ModelViewMatrix is the camera's view rotation (positions are camera-relative).
    assert_eq!(p.semantic("model_view"), "sb_hSodium.u_ModelViewMatrix");
    assert_eq!(p.semantic("chunk_offset"), "(u_RegionOffset + sb_sodiumDrawTranslation())");
}

#[test]
fn extension_semantics_decode_the_host_encoding() {
    let p = profile("sodium_terrain").unwrap();
    // TerrainExtension (Java): entity = ((id + 1) << 1) | fluid; normal = round(n * 127);
    // mid texture coordinate = round(c * 32768); mid block = round((centre - v) * 64), w = emission.
    assert_eq!(p.semantic("entity"), "vec4(float(int(sb_Entity >> 1u) - 1), float(sb_Entity & 1u), 0.0, 1.0)");
    assert_eq!(p.semantic("normal"), "normalize(sb_Normal.xyz)");
    assert_eq!(p.semantic("mid_tex_coord"), "vec4(vec2(sb_MidTexCoord) / 32768.0, 0.0, 1.0)");
    assert_eq!(p.semantic("mid_block"), "sb_MidBlock * 127.0");
}
