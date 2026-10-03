//! ARCHITECTURE §8 "Built-in variables" and "Texture functions": compatibility builtins
//! through the draw-profile semantics, matrices, Iris attributes, core-profile names,
//! `gl_Fog`, legacy sampling functions and storage qualifiers.

use sb_core::ShaderStage;

use crate::harness::*;

const FS: &str = "#version 120\nvarying vec4 v;\nvoid main() { gl_FragData[0] = v; }\n";

/// Translate a vertex shader writing `v` with the terrain profile.
fn vs(src: &str) -> Out {
    T::gbuffers().vs(src).fs(FS).run()
}

#[test]
fn vertex_attributes_read_the_profile_semantics() {
    let out = vs("#version 120\nvarying vec4 v;\nvoid main() {\n  gl_Position = gl_Vertex;\n  v = gl_Color + gl_MultiTexCoord0 + gl_MultiTexCoord1 + gl_MultiTexCoord2 + gl_MultiTexCoord3 + gl_MultiTexCoord5 + vec4(gl_Normal, 0.0) + gl_SecondaryColor + vec4(gl_FogCoord);\n}\n");
    contains_all(
        out.vs(),
        &[
            "vec4 sb_gl_Vertex = vec4(Position + vec3(ChunkPosition - sb_hGlobals.CameraBlockPos) + sb_hGlobals.CameraOffset, 1.0);",
            "vec4 sb_gl_Color = Color;",
            "vec4 sb_gl_MultiTexCoord0 = vec4(UV0, 0.0, 1.0);",
            "vec4 sb_gl_MultiTexCoord1 = vec4(vec2(UV2), 0.0, 1.0);",
            "vec3 sb_gl_Normal = sb_Normal;",
            "v = sb_gl_Color + sb_gl_MultiTexCoord0 + sb_gl_MultiTexCoord1 + sb_gl_MultiTexCoord1 + sb_mc_midTexCoord + sb_gl_MultiTexCoordZero",
            "vec4 sb_gl_MultiTexCoordZero = vec4(0.0, 0.0, 0.0, 1.0);",
        ],
    );
    for builtin in ["gl_Vertex", "gl_MultiTexCoord", "gl_Normal", "gl_Color", "gl_SecondaryColor", "gl_FogCoord"] {
        assert!(!out.vs().replace(&format!("sb_{builtin}"), "").contains(builtin), "{builtin} left in:\n{}", out.vs());
    }
}

#[test]
fn matrices_and_their_variants() {
    let out = vs("#version 120\nvarying vec4 v;\nuniform int frameCounter;\nvoid main() {\n  gl_Position = gl_ModelViewProjectionMatrix * gl_Vertex;\n  vec3 n = gl_NormalMatrix * gl_Normal;\n  vec4 t = gl_TextureMatrix[0] * gl_MultiTexCoord0 + gl_TextureMatrix[1] * gl_MultiTexCoord1 + gl_TextureMatrix[2][3] + gl_TextureMatrix[5][0];\n  mat4 d = gl_TextureMatrix[frameCounter & 1];\n  mat4 i = gl_ModelViewMatrixInverse * gl_ProjectionMatrixInverse * gl_ModelViewMatrixTranspose * gl_ModelViewProjectionMatrixInverseTranspose * gl_TextureMatrixInverse[0];\n  v = vec4(n, 1.0) + t + d[0] + i[0];\n}\n");
    contains_all(
        out.vs(),
        &[
            "gl_Position = sb_ModelViewProjection * sb_gl_Vertex;",
            "mat4 sb_ModelViewProjection = sb_Projection * sb_ModelView;",
            "vec3 n = sb_NormalMatrix * sb_gl_Normal;",
            "mat3 sb_NormalMatrix = mat3(transpose(inverse(sb_ModelView)));",
            "sb_TextureMatrix * sb_gl_MultiTexCoord0 + sb_LightmapMatrix * sb_gl_MultiTexCoord1 + sb_LightmapMatrix[3] + (mat4(1.0))[0]",
            "mat4 sb_LightmapMatrix = mat4(vec4(0.00390625, 0.0, 0.0, 0.0), vec4(0.0, 0.00390625, 0.0, 0.0), vec4(0.0, 0.0, 0.00390625, 0.0), vec4(0.03125, 0.03125, 0.03125, 1.0));",
            "mat4 d = sb_TextureMatrices[frameCounter & 1];",
            "mat4 sb_TextureMatrices[8] = mat4[8](sb_TextureMatrix, sb_LightmapMatrix, sb_LightmapMatrix,",
            "sb_ModelViewInverse * sb_ProjectionInverse * sb_ModelViewTranspose * sb_ModelViewProjectionInverseTranspose * sb_TextureMatrixInverse",
            "mat4 sb_ModelViewInverse = inverse(sb_ModelView);",
            "mat4 sb_ModelViewProjectionInverseTranspose = transpose(inverse(sb_ModelViewProjection));",
            "mat4 sb_ModelView = sb_hTerrain.ModelViewMat;",
            "mat4 sb_Projection = gbufferProjection;",
        ],
    );
}

#[test]
fn matrices_are_available_in_later_stages() {
    // Fragment stages can read the matrices (not the vertex attributes).
    let fs = "#version 120\nvarying vec4 v;\nvoid main() { gl_FragData[0] = gl_ProjectionMatrix * gl_ModelViewMatrixInverse * v; }\n";
    let out = T::gbuffers().vs("#version 120\nvarying vec4 v;\nvoid main() { gl_Position = ftransform(); v = gl_Vertex; }\n").fs(fs).run();
    contains_all(out.fs(), &["mat4 sb_Projection = gbufferProjection;", "mat4 sb_ModelView = sb_hTerrain.ModelViewMat;", "sb_Projection * sb_ModelViewInverse * v"]);
}

#[test]
fn ftransform_uses_the_profile_matrices() {
    let out = vs("#version 120\nvarying vec4 v;\nvoid main() { gl_Position = ftransform(); v = vec4(1.0); }\n");
    contains_all(out.vs(), &["gl_Position = sb_Projection * (sb_ModelView * sb_gl_Vertex);"]);
}

#[test]
fn iris_attributes_become_globals_of_the_declared_type() {
    let out = vs("#version 330\nin int mc_Entity;\nin vec2 mc_midTexCoord;\nin vec3 at_midBlock;\nin vec4 at_tangent;\nin vec3 at_velocity;\nin vec3 vaPosition;\nin ivec2 vaUV2;\nin float mc_chunkFade;\nin vec4 myCustomAttribute;\nout vec4 v;\n\
                  void main() { gl_Position = vec4(vaPosition, 1.0); v = vec4(float(mc_Entity), mc_midTexCoord, mc_chunkFade) + vec4(at_midBlock + at_velocity, 0.0) + at_tangent + vec4(vaUV2, 0.0, 0.0) + myCustomAttribute; }\n");
    contains_all(
        out.vs(),
        &[
            "int mc_Entity = int((sb_mc_Entity).x);",
            "vec2 mc_midTexCoord = vec2((sb_mc_midTexCoord).xy);",
            "vec3 at_midBlock = vec3((sb_at_midBlock).xyz);",
            "vec4 at_tangent = sb_at_tangent;",
            "vec3 at_velocity = sb_at_velocity;",
            "vec3 vaPosition = sb_gl_Vertex.xyz - sb_ChunkOffset;",
            "ivec2 vaUV2 = ivec2((sb_gl_MultiTexCoord1).xy);",
            "float mc_chunkFade = ChunkVisibility;",
            "vec4 myCustomAttribute = vec4(0);",
        ],
    );
    assert!(out.has_diag("xf.unknown-attribute"));
    contains_none(out.vs(), &["in int mc_Entity", "in vec3 vaPosition"]);
}

#[test]
fn undeclared_core_attributes_are_provided() {
    // Iris declares the `va*` attributes itself when a core-profile program uses them.
    let out = vs("#version 330\nout vec4 v;\nvoid main() { gl_Position = vec4(vaPosition, 1.0); v = vaColor * vec4(vaUV0, vec2(vaUV2)); }\n");
    contains_all(out.vs(), &["vec3 vaPosition = sb_gl_Vertex.xyz - sb_ChunkOffset;", "vec4 vaColor = sb_gl_Color;", "vec2 vaUV0 = vec2((sb_gl_MultiTexCoord0).xy);", "ivec2 vaUV2 = ivec2((sb_gl_MultiTexCoord1).xy);"]);
}

#[test]
fn core_profile_matrix_names_use_the_semantics() {
    let out = T::new("vanilla_entity")
        .vs("#version 330\nin vec3 vaPosition;\nuniform mat4 modelViewMatrix;\nuniform mat4 projectionMatrix;\nuniform mat3 normalMatrix;\nuniform vec3 chunkOffset;\nuniform mat4 textureMatrix = mat4(1.0);\nout vec4 v;\n\
             void main() { gl_Position = projectionMatrix * modelViewMatrix * vec4(vaPosition + chunkOffset, 1.0); v = textureMatrix * vec4(normalMatrix * vec3(1.0), 1.0); }\n")
        .fs("#version 330\nin vec4 v;\nuniform mat4 modelViewMatrixInverse;\nout vec4 c;\nvoid main() { c = modelViewMatrixInverse * v; }\n")
        .run();
    contains_all(
        out.vs(),
        &["gl_Position = sb_Projection * sb_ModelView * vec4(vaPosition + sb_ChunkOffset, 1.0);", "v = sb_TextureMatrix * vec4(sb_NormalMatrix * vec3(1.0), 1.0);", "mat4 sb_ModelView = sb_hDynamic.ModelViewMat;"],
    );
    contains_none(out.vs(), &["uniform mat4 modelViewMatrix", "projectionMatrix *"]);
    contains_all(out.fs(), &["c = sb_ModelViewInverse * v;"]);
    // Compat code using a core-profile name without declaring it.
    let out = vs("#version 120\nvarying vec4 v;\nvoid main() { gl_Position = projectionMatrix * gl_ModelViewMatrix * gl_Vertex; v = vec4(1.0); }\n");
    contains_all(out.vs(), &["gl_Position = sb_Projection * sb_ModelView * sb_gl_Vertex;"]);
}

#[test]
fn fixed_function_fog_and_clip_vertex() {
    let out = vs("#version 120\nvarying vec4 v;\nvoid main() { gl_Position = ftransform(); gl_ClipVertex = gl_ModelViewMatrix * gl_Vertex; v = gl_Fog.color * gl_Fog.density + vec4(gl_Fog.start, gl_Fog.end, gl_Fog.scale, 1.0); }\n");
    contains_all(
        out.vs(),
        &[
            "struct sb_FogParameters { vec4 color; float density; float start; float end; float scale; };",
            "sb_FogParameters sb_gl_Fog = sb_FogParameters(sb_FogColor, fogDensity, fogStart, fogEnd, fogScale);",
            "v = sb_gl_Fog.color * sb_gl_Fog.density",
        ],
    );
    contains_none(out.vs(), &["gl_ClipVertex"]);
    for m in ["sb_FogColor", "fogDensity", "fogStart", "fogEnd", "fogScale"] {
        assert!(out.prog.frame_members_used.iter().any(|x| x == m), "{m}: {:?}", out.prog.frame_members_used);
    }
}

#[test]
fn unsupported_fixed_function_state_is_an_error() {
    let errors = T::gbuffers()
        .vs("#version 120\nvarying vec4 v;\nvoid main() { gl_Position = ftransform(); v = gl_LightSource[0].diffuse; }\n")
        .fs(FS)
        .errors();
    assert!(errors.iter().any(|e| e == "xf.unsupported-builtin"), "{errors:?}");
    let out = vs("#version 120\nvarying vec4 v;\nvoid main() { gl_Position = ftransform(); v = vec4(float(gl_MaxTextureCoords)); }\n");
    contains_all(out.vs(), &["const int sb_MaxTextureCoords = 8;"]);
}

#[test]
fn legacy_sampling_functions_map_to_core_names() {
    let fs = "#version 120\n#extension GL_ARB_shader_texture_lod : enable\nuniform sampler2D colortex0;\nuniform sampler3D colortex1;\nuniform samplerCube colortex2;\nuniform sampler2DRect colortex3;\nuniform sampler2DShadow shadowtex0;\nvarying vec4 v;\n\
              void main() {\n  vec4 a = texture2D(colortex0, v.xy) + texture2DLod(colortex0, v.xy, 1.0) + texture2DProj(colortex0, v) + texture2DGradARB(colortex0, v.xy, vec2(0.0), vec2(0.0)) + texture2DLodEXT(colortex0, v.xy, 0.0);\n\
              a += texture3D(colortex1, v.xyz) + textureCube(colortex2, v.xyz) + textureCubeLod(colortex2, v.xyz, 0.0) + texture2DRect(colortex3, v.xy) + texelFetch2D(colortex0, ivec2(0), 0);\n\
              a += vec4(textureSize2D(colortex0, 0), 0.0, 0.0) + shadow2D(shadowtex0, v.xyz) + shadow2DProj(shadowtex0, v);\n  gl_FragData[0] = a; }\n";
    let out = T::fullscreen().vs("#version 120\nvarying vec4 v;\nvoid main() { gl_Position = ftransform(); v = gl_MultiTexCoord0; }\n").fs(fs).run();
    contains_all(
        out.fs(),
        &[
            "texture(colortex0, v.xy) + textureLod(colortex0, v.xy, 1.0) + textureProj(colortex0, v) + textureGrad(colortex0, v.xy, vec2(0.0), vec2(0.0)) + textureLod(colortex0, v.xy, 0.0)",
            "texture(colortex1, v.xyz) + texture(colortex2, v.xyz) + textureLod(colortex2, v.xyz, 0.0) + texture(colortex3, sb_rect(v.xy, textureSize(colortex3, 0))) + texelFetch(colortex0, ivec2(0), 0)",
            "vec4(textureSize(colortex0, 0), 0.0, 0.0) + vec4(texture(shadowtex0, v.xyz)) + vec4(textureProj(shadowtex0, v))",
        ],
    );
    contains_none(out.fs(), &["GL_ARB_shader_texture_lod", "texture2D", "texture3D", "textureCube"]);
    assert!(out.prog.requires_raw_vulkan, "sampler3D needs raw Vulkan");
}

#[test]
fn pack_definitions_of_legacy_names_win_over_the_mapping() {
    // A pack polyfill with a legacy name keeps working (renamed with its calls); other
    // arities still map to the core function.
    let fs = "#version 130\nuniform sampler2D colortex0;\nvarying vec2 uv;\nvec4 texture2DLod(sampler2D s, vec2 c, float l, float bias) { return textureLod(s, c, l + bias); }\n\
              void main() { gl_FragData[0] = texture2DLod(colortex0, uv, 0.0, 1.0) + texture2DLod(colortex0, uv, 1.0); }\n";
    let out = T::fullscreen().vs("#version 130\nvarying vec2 uv;\nvoid main() { gl_Position = ftransform(); uv = vec2(0.0); }\n").fs(fs).run();
    contains_all(out.fs(), &["vec4 sb_u_texture2DLod(sampler2D s, vec2 c, float l, float bias)", "sb_u_texture2DLod(colortex0, uv, 0.0, 1.0) + textureLod(colortex0, uv, 1.0)"]);
}

#[test]
fn geometry_storage_qualifiers() {
    let vs = "#version 120\nvarying vec2 uv;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.xy; }\n";
    let gs = "#version 150\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\nin vec2 uv[];\nvarying out vec2 guv;\n\
              void main() { for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; guv = uv[i]; EmitVertex(); } }\n";
    let fs = "#version 120\nvarying vec2 guv;\nvoid main() { gl_FragData[0] = vec4(guv, 0.0, 1.0); }\n";
    let out = T::fullscreen().vs(vs).gs(gs).fs(fs).run();
    contains_all(out.glsl(ShaderStage::Geometry), &["out vec2 guv;"]);
    contains_none(out.glsl(ShaderStage::Geometry), &["varying"]);
}

#[test]
fn rectangle_samplers_become_2d_samplers_with_normalized_coordinates() {
    // Vulkan has no rectangle textures (the SampledRect capability is GL-only).
    let fs = "#version 140\nuniform sampler2DRect colortex3;\nuniform usampler2DRect colortex4;\nuniform sampler2DRectShadow shadowtex0;\nin vec2 px;\nout vec4 c;\n\
              vec4 tap(sampler2DRect s, vec2 p) { return texture(s, p) + textureOffset(s, p, ivec2(1)); }\n\
              void main() {\n  ivec2 size = textureSize(colortex3);\n  c = tap(colortex3, px) + texelFetch(colortex3, ivec2(px)) + texture2DRect(colortex3, px) + textureProj(colortex3, vec3(px, 1.0));\n\
              c += vec4(texelFetch(colortex4, ivec2(1, 2)).r) + vec4(texture(shadowtex0, vec3(px, 0.5))) + textureGrad(colortex3, px, vec2(1.0), vec2(1.0)) + vec4(size, 0.0, 0.0);\n}\n";
    let vs = "#version 140\nout vec2 px;\nvoid main() { gl_Position = vec4(0.0); px = vec2(0.0); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_all(
        out.fs(),
        &[
            "uniform sampler2D colortex3;",
            "uniform usampler2D colortex4;",
            "uniform sampler2DShadow shadowtex0;",
            "vec4 tap(sampler2D s, vec2 p)",
            "return texture(s, sb_rect(p, textureSize(s, 0))) + textureOffset(s, sb_rect(p, textureSize(s, 0)), ivec2(1));",
            "ivec2 size = textureSize(colortex3, 0);",
            "texelFetch(colortex3, ivec2(px), 0)",
            "texture(colortex3, sb_rect(px, textureSize(colortex3, 0)))",
            "textureProj(colortex3, sb_rect(vec3(px, 1.0), textureSize(colortex3, 0)))",
            "texelFetch(colortex4, ivec2(1, 2), 0).r",
            "texture(shadowtex0, sb_rect(vec3(px, 0.5), textureSize(shadowtex0, 0)))",
            "textureGrad(colortex3, sb_rect(px, textureSize(colortex3, 0)), vec2(1.0) / vec2(textureSize(colortex3, 0)), vec2(1.0) / vec2(textureSize(colortex3, 0)))",
            "vec3 sb_rect(vec3 p, ivec2 s) { return vec3(p.xy / vec2(s), p.z); }",
        ],
    );
    contains_none(out.fs(), &["Rect "]);
    assert!(!out.refl(ShaderStage::Fragment).capabilities.iter().any(|c| c.contains("Rect")));
}

#[test]
fn iris_default_vertex_shader_feeds_fixed_function_fragment_inputs() {
    // Programs that ship only a `.fsh` get Iris's default vertex shader.
    let fs = "#version 120\nuniform sampler2D texture;\nvoid main() { gl_FragData[0] = texture2D(texture, gl_TexCoord[0].st) * gl_Color * gl_TexCoord[1].x; }\n";
    let out = T::gbuffers().vs(sb_transform::DEFAULT_VERTEX_SHADER).fs(fs).run();
    contains_all(out.vs(), &["sb_v_TexCoord[0] = sb_TextureMatrix * sb_gl_MultiTexCoord0;", "sb_v_Color = sb_gl_Color;"]);
    contains_all(out.fs(), &["texture(Sampler0, sb_v_TexCoord[0].st) * sb_v_Color * sb_v_TexCoord[1].x"]);
}
