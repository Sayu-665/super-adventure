//! Hand-written Vulkan GLSL for the test pack, written the way sb-transform emits it for
//! the draw profiles (`vanilla_terrain`, `vanilla_entity`, `vanilla_position`,
//! `dh_terrain`, `fullscreen`): explicit set/binding from the binding table, the pack-
//! global `sb_Frame` / `sb_Draw` blocks with `layout(offset = N)` members, the profile
//! host blocks, and the depth epilogue / depth-read rewrite of the selected depth mode
//! (`SB_REVERSED`).

/// Shared prelude: depth-mode macros (what the translator bakes in per mode).
pub const PRELUDE: &str = r#"
#ifdef SB_REVERSED
#define SB_DEPTH_EPILOGUE gl_Position.z = 0.5 * (gl_Position.w - gl_Position.z);
#define SB_READ_DEPTH(x) (1.0 - (x))
#define SB_SHADOW_REF(x) (1.0 - (x))
#else
#define SB_DEPTH_EPILOGUE gl_Position.z = 0.5 * (gl_Position.z + gl_Position.w);
#define SB_READ_DEPTH(x) (x)
#define SB_SHADOW_REF(x) (x)
#endif
"#;

/// `sb_Frame` member offsets (pack-global layout, see `frame_layout`).
pub const FRAME_DECL: &str = r#"
layout(std140, set = 0, binding = 0) uniform sb_Frame {
    layout(offset = 0) mat4 gbufferModelView;
    layout(offset = 64) mat4 gbufferProjection;
    layout(offset = 128) mat4 gbufferProjectionInverse;
    layout(offset = 192) mat4 gbufferModelViewInverse;
    layout(offset = 256) mat4 shadowModelView;
    layout(offset = 320) mat4 shadowProjection;
    layout(offset = 384) mat4 dhProjection;
    layout(offset = 448) mat4 dhProjectionInverse;
    layout(offset = 512) vec3 sunPosition;
    layout(offset = 524) float frameTimeCounter;
    layout(offset = 528) vec3 fogColor;
    layout(offset = 540) float rainStrength;
    layout(offset = 544) vec3 skyColor;
    layout(offset = 556) float viewWidth;
    layout(offset = 560) float viewHeight;
    layout(offset = 564) float myCustom;
    layout(offset = 568) float testDefault;
    layout(offset = 572) float near;
    layout(offset = 576) float far;
    layout(offset = 580) float dhFarPlane;
};
"#;

pub const DRAW_DECL: &str = r#"
layout(std140, set = 0, binding = 1) uniform sb_Draw {
    layout(offset = 0) float alphaTestRef;
    layout(offset = 4) int entityId;
    layout(offset = 16) mat4 projectionMatrix;
};
"#;

const TERRAIN_HOST: &str = r#"
layout(std140, set = 0, binding = 2) uniform Globals { ivec3 CameraBlockPos; float GlintAlpha; vec3 CameraOffset; float GameTime; vec2 ScreenSize; int MenuBlurRadius; int UseRgss; } sb_hGlobals;
layout(std140, set = 0, binding = 3) uniform TerrainUniform { mat4 ModelViewMat; ivec2 TextureSize; } sb_hTerrain;
layout(location = 0) in vec3 Position;
layout(location = 1) in vec4 Color;
layout(location = 2) in vec2 UV0;
layout(location = 3) in ivec2 UV2;
layout(location = 4) in ivec3 ChunkPosition;
layout(location = 5) in float ChunkVisibility;
layout(location = 6) in vec3 sb_Normal;
layout(location = 7) in ivec2 sb_Entity;
layout(location = 8) in vec2 sb_MidTexCoord;
layout(location = 9) in vec4 sb_Tangent;
layout(location = 10) in ivec4 sb_MidBlock;
"#;

/// gbuffers_terrain / gbuffers_water vertex stage (vanilla_terrain profile).
pub fn terrain_vsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{FRAME_DECL}{TERRAIN_HOST}{}",
        r#"
layout(location = 0) out vec2 texcoord;
layout(location = 1) out vec4 color;
layout(location = 2) out vec2 lmcoord;
layout(location = 3) out vec3 normal;
layout(location = 4) flat out int blockId;
void main() {
    vec4 pos = vec4(Position + vec3(ChunkPosition - sb_hGlobals.CameraBlockPos) + sb_hGlobals.CameraOffset, 1.0);
    gl_Position = gbufferProjection * (sb_hTerrain.ModelViewMat * pos);
    texcoord = UV0;
    color = vec4(Color.rgb, Color.a * ChunkVisibility);
    lmcoord = (vec2(UV2) + 8.0) / 256.0;
    normal = mat3(sb_hTerrain.ModelViewMat) * sb_Normal;
    blockId = sb_Entity.x + int(sb_MidTexCoord.x * 0.0) + int(sb_Tangent.w * 0.0) + sb_MidBlock.w * 0;
    SB_DEPTH_EPILOGUE
}
"#
    )
}

const GBUFFER_FS_HEAD: &str = r#"
layout(set = 1, binding = 0) uniform sampler2D Sampler0;
layout(set = 1, binding = 1) uniform sampler2D Sampler2;
layout(location = 0) in vec2 texcoord;
layout(location = 1) in vec4 color;
layout(location = 2) in vec2 lmcoord;
layout(location = 3) in vec3 normal;
layout(location = 4) flat in int blockId;
layout(location = 0) out vec4 sb_FragData0;
layout(location = 1) out vec4 sb_FragData1;
layout(location = 2) out vec4 sb_FragData2;
"#;

/// gbuffers_terrain fragment stage: albedo, normal, lightmap; alpha test epilogue.
pub fn terrain_fsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{DRAW_DECL}{GBUFFER_FS_HEAD}{}",
        r#"
void main() {
    vec4 albedo = texture(Sampler0, texcoord) * color;
    sb_FragData0 = albedo;
    sb_FragData1 = vec4(normalize(normal) * 0.5 + 0.5, 1.0);
    sb_FragData2 = vec4(texture(Sampler2, lmcoord).rgb, blockId >= 0 ? 1.0 : 0.5);
    if (!(sb_FragData0.a > alphaTestRef)) discard;
}
"#
    )
}

/// gbuffers_water fragment stage: translucent water into colortex0 (blended).
pub fn water_fsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{GBUFFER_FS_HEAD}{}",
        r#"
void main() {
    vec4 albedo = texture(Sampler0, texcoord) * color;
    sb_FragData0 = vec4(albedo.rgb, 0.7);
    sb_FragData1 = vec4(normalize(normal) * 0.5 + 0.5, 1.0);
    sb_FragData2 = vec4(texture(Sampler2, lmcoord).rgb, 1.0);
}
"#
    )
}

/// gbuffers_entities (vanilla_entity profile).
pub fn entity_vsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{DRAW_DECL}{}",
        r#"
layout(std140, set = 0, binding = 4) uniform DynamicTransforms { mat4 ModelViewMat; mat4 TextureMat; vec4 ColorModulator; vec3 ModelOffset; } sb_hDynamic;
layout(set = 1, binding = 13) uniform sampler2D Sampler1;
layout(location = 0) in vec3 Position;
layout(location = 1) in vec4 Color;
layout(location = 2) in vec2 UV0;
layout(location = 3) in ivec2 UV1;
layout(location = 4) in ivec2 UV2;
layout(location = 5) in vec3 Normal;
layout(location = 0) out vec2 texcoord;
layout(location = 1) out vec4 color;
layout(location = 2) out vec2 lmcoord;
layout(location = 3) out vec3 normal;
layout(location = 4) flat out int blockId;
void main() {
    gl_Position = projectionMatrix * (sb_hDynamic.ModelViewMat * vec4(Position + sb_hDynamic.ModelOffset, 1.0));
    texcoord = (sb_hDynamic.TextureMat * vec4(UV0, 0.0, 1.0)).xy;
    vec4 overlay = texelFetch(Sampler1, UV1, 0);
    color = Color * sb_hDynamic.ColorModulator * vec4(mix(vec3(1.0), overlay.rgb, 1.0 - overlay.a), 1.0);
    lmcoord = (vec2(UV2) + 8.0) / 256.0;
    normal = mat3(sb_hDynamic.ModelViewMat) * Normal;
    blockId = entityId;
    SB_DEPTH_EPILOGUE
}
"#
    )
}

/// gbuffers_skybasic (vanilla_position profile).
pub fn sky_vsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{DRAW_DECL}{}",
        r#"
layout(std140, set = 0, binding = 4) uniform DynamicTransforms { mat4 ModelViewMat; mat4 TextureMat; vec4 ColorModulator; vec3 ModelOffset; } sb_hDynamic;
layout(location = 0) in vec3 Position;
layout(location = 0) out vec4 color;
void main() {
    gl_Position = projectionMatrix * (sb_hDynamic.ModelViewMat * vec4(Position + sb_hDynamic.ModelOffset, 1.0));
    color = sb_hDynamic.ColorModulator;
    SB_DEPTH_EPILOGUE
}
"#
    )
}

pub fn sky_fsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{}",
        r#"
layout(location = 0) in vec4 color;
layout(location = 0) out vec4 sb_FragData0;
layout(location = 1) out vec4 sb_FragData1;
void main() {
    sb_FragData0 = vec4(color.rgb, 1.0);
    sb_FragData1 = vec4(0.5, 0.5, 1.0, 1.0);
}
"#
    )
}

/// dh_terrain (dh_terrain profile, world-space semantics).
pub fn dh_vsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{FRAME_DECL}{}",
        r#"
layout(std140, set = 0, binding = 5) uniform vertUniqueUniformBlock { vec3 uModelOffset; } sb_hDhUnique;
layout(std140, set = 0, binding = 6) uniform vertSharedUniformBlock { bool uIsWhiteWorld; float uWorldYOffset; float uMircoOffset; float uEarthRadius; float uFrameMod8; float uViewWidth; float uViewHeight; vec3 uCameraPos; mat4 uCombinedMatrix; } sb_hDhShared;
layout(location = 0) in uvec3 vPosition;
layout(location = 1) in uint meta;
layout(location = 2) in vec4 vColor;
layout(location = 3) in uint irisMaterial;
layout(location = 4) in uint irisNormal;
layout(location = 5) in uint textureTile;
layout(location = 0) out vec4 color;
layout(location = 1) out vec3 normal;
layout(location = 2) out vec2 lmcoord;
layout(location = 3) out vec3 blockPos;
layout(location = 4) flat out uint tile;
vec3 sb_dhNormal(uint i) {
    if (i == 0u) return vec3(0.0, -1.0, 0.0);
    if (i == 1u) return vec3(0.0, 1.0, 0.0);
    if (i == 2u) return vec3(0.0, 0.0, -1.0);
    if (i == 3u) return vec3(0.0, 0.0, 1.0);
    if (i == 4u) return vec3(-1.0, 0.0, 0.0);
    return vec3(1.0, 0.0, 0.0);
}
void main() {
    vec3 p = vec3(vPosition) + (sb_hDhUnique.uModelOffset - sb_hDhShared.uCameraPos);
    gl_Position = dhProjection * (gbufferModelView * vec4(p, 1.0));
    color = vColor;
    normal = mat3(gbufferModelView) * sb_dhNormal(irisNormal);
    lmcoord = vec2((float((meta >> 4u) & 15u) + 0.5) / 16.0, (float(meta & 15u) + 0.5) / 16.0);
    blockPos = vec3(vPosition) + vec3(float(irisMaterial) * 0.0);
    tile = textureTile;
    SB_DEPTH_EPILOGUE
}
"#
    )
}

pub fn dh_fsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{}",
        r#"
layout(set = 1, binding = 1) uniform sampler2D uLightMap;
layout(set = 1, binding = 10) uniform sampler2D uBlockAtlas;
layout(location = 0) in vec4 color;
layout(location = 1) in vec3 normal;
layout(location = 2) in vec2 lmcoord;
layout(location = 3) in vec3 blockPos;
layout(location = 4) flat in uint tile;
layout(location = 0) out vec4 sb_FragData0;
layout(location = 1) out vec4 sb_FragData1;
layout(location = 2) out vec4 sb_FragData2;
void main() {
    vec4 c = color;
    if (tile != 0u) {
        ivec2 atlasSize = textureSize(uBlockAtlas, 0);
        vec2 origin = vec2(float(tile % 256u), float(tile / 256u)) * 16.0;
        vec2 uv = (origin + fract(blockPos.xz) * 16.0) / vec2(atlasSize);
        c *= texture(uBlockAtlas, uv);
    }
    sb_FragData0 = vec4(c.rgb, 1.0);
    sb_FragData1 = vec4(normalize(normal) * 0.5 + 0.5, 1.0);
    sb_FragData2 = vec4(texture(uLightMap, lmcoord).rgb, 1.0);
}
"#
    )
}

/// shadow (vanilla_terrain profile with the shadow-pass world-space semantics).
pub fn shadow_vsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{FRAME_DECL}{TERRAIN_HOST}{}",
        r#"
layout(location = 0) out vec2 texcoord;
layout(location = 1) out vec4 color;
void main() {
    vec4 pos = vec4(Position + vec3(ChunkPosition - sb_hGlobals.CameraBlockPos) + sb_hGlobals.CameraOffset, 1.0);
    gl_Position = shadowProjection * (shadowModelView * pos);
    texcoord = UV0;
    color = Color;
    SB_DEPTH_EPILOGUE
}
"#
    )
}

pub fn shadow_fsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{DRAW_DECL}{}",
        r#"
layout(set = 1, binding = 0) uniform sampler2D Sampler0;
layout(location = 0) in vec2 texcoord;
layout(location = 1) in vec4 color;
layout(location = 0) out vec4 sb_FragData0;
void main() {
    sb_FragData0 = texture(Sampler0, texcoord) * color;
    if (!(sb_FragData0.a > alphaTestRef)) discard;
}
"#
    )
}

/// The fullscreen vertex stage (fullscreen profile).
pub fn fullscreen_vsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{}",
        r#"
layout(location = 0) out vec2 texcoord;
void main() {
    int i = gl_VertexIndex % 6;
    vec2 uv = vec2(float(i == 1 || i == 2 || i == 4), float(i == 2 || i == 4 || i == 5));
    texcoord = uv;
    gl_Position = vec4(uv * 2.0 - 1.0, 0.0, 1.0);
    SB_DEPTH_EPILOGUE
}
"#
    )
}

/// composite: deferred lighting with shadows (manual compare and a hardware compare
/// sampler), sky from the inverse matrices, and (`SB_NATIVE_DH`) DH depth.
pub fn composite_fsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{FRAME_DECL}{}",
        r#"
layout(set = 1, binding = 2) uniform sampler2D colortex0;
layout(set = 1, binding = 3) uniform sampler2D colortex1;
layout(set = 1, binding = 4) uniform sampler2D colortex2;
layout(set = 1, binding = 5) uniform sampler2D depthtex0;
layout(set = 1, binding = 6) uniform sampler2D shadowtex0;
layout(set = 1, binding = 7) uniform sampler2D noisetex;
layout(set = 1, binding = 9) uniform sampler2D dhDepthTex0;
layout(set = 1, binding = 11) uniform sampler2DShadow shadowtex1;
layout(set = 1, binding = 12) uniform sampler2D shadowcolor0;
layout(location = 0) in vec2 texcoord;
layout(location = 0) out vec4 sb_FragData0;
void main() {
    vec3 albedo = texture(colortex0, texcoord).rgb;
    float depth = SB_READ_DEPTH(texture(depthtex0, texcoord).r);
    mat4 projInv = gbufferProjectionInverse;
    float fogFar = far;
#ifdef SB_NATIVE_DH
    if (depth >= 1.0) {
        depth = SB_READ_DEPTH(texture(dhDepthTex0, texcoord).r);
        projInv = dhProjectionInverse;
        fogFar = dhFarPlane;
    }
#endif
    vec4 view = projInv * vec4(vec3(texcoord, depth) * 2.0 - 1.0, 1.0);
    view /= view.w;
    if (depth >= 1.0) {
        vec3 dir = normalize(view.xyz);
        float up = dot(dir, normalize(mat3(gbufferModelView) * vec3(0.0, 1.0, 0.0)));
        vec3 sky = mix(fogColor, skyColor, clamp(up * 2.0, 0.0, 1.0));
        sb_FragData0 = vec4(mix(albedo, sky, 0.5), 1.0);
        return;
    }
    vec3 n = texture(colortex1, texcoord).rgb * 2.0 - 1.0;
    vec3 light = texture(colortex2, texcoord).rgb;
    vec3 feet = (gbufferModelViewInverse * vec4(view.xyz, 1.0)).xyz;
    vec4 sp = shadowProjection * (shadowModelView * vec4(feet + n * 0.05, 1.0));
    vec3 s = sp.xyz / sp.w * 0.5 + 0.5;
    float shadow = 1.0;
    if (all(greaterThan(s, vec3(0.0))) && all(lessThan(s, vec3(1.0)))) {
        float occ = SB_READ_DEPTH(texture(shadowtex0, s.xy).r);
        float manual = s.z - 0.002 <= occ ? 1.0 : 0.0;
        float hw = texture(shadowtex1, vec3(s.xy, SB_SHADOW_REF(s.z - 0.002)));
        shadow = 0.5 * (manual + hw);
        shadow *= mix(0.9, 1.0, texture(shadowcolor0, s.xy).a);
    }
    float sun = max(dot(normalize(n), normalize(sunPosition)), 0.0) * shadow;
    float dither = texture(noisetex, texcoord * vec2(viewWidth, viewHeight) / 256.0).r * 0.002;
    vec3 lit = albedo * (light * 0.6 + vec3(1.0, 0.95, 0.85) * sun * 0.8) + dither;
    float fog = clamp(length(view.xyz) / fogFar, 0.0, 1.0);
    sb_FragData0 = vec4(mix(lit, fogColor, fog * fog), 1.0);
}
"#
    )
}

/// A compute pass writing 1 into the SSBO the final pass multiplies with.
pub fn compute_csh() -> String {
    format!(
        "#version 460\n{PRELUDE}{}",
        r#"
layout(local_size_x = 8, local_size_y = 8) in;
layout(std430, set = 2, binding = 0) buffer SbBuf { vec4 data[]; } ssbo;
void main() {
    if (gl_GlobalInvocationID.x == 0u && gl_GlobalInvocationID.y == 0u) {
        ssbo.data[0] = vec4(1.0);
    }
}
"#
    )
}

/// final: tone mapping; multiplies with the SSBO (written by the compute), the custom
/// uniform and the uniform default, so a broken step shows up as a black image.
pub fn final_fsh() -> String {
    format!(
        "#version 460\n{PRELUDE}{FRAME_DECL}{}",
        r#"
layout(set = 1, binding = 2) uniform sampler2D colortex0;
layout(std430, set = 2, binding = 0) readonly buffer SbBuf { vec4 data[]; } ssbo;
layout(location = 0) in vec2 texcoord;
layout(location = 0) out vec4 sb_FragData0;
void main() {
    vec3 c = texture(colortex0, texcoord).rgb * ssbo.data[0].x * myCustom * (testDefault / 0.75);
    c = c / (1.0 + c) * 1.6;
    sb_FragData0 = vec4(pow(clamp(c, 0.0, 1.0), vec3(1.0 / 1.4)), 1.0);
}
"#
    )
}
