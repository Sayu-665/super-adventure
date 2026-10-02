//! ARCHITECTURE §4: depth modes, Y flip, reversed-depth reads, vertex/instance ids.

use sb_core::ShaderStage;
use sb_core::model::DepthMode;

use crate::harness::*;

const FORWARD: &str = "gl_Position.z = 0.5 * (gl_Position.z + gl_Position.w);";
const REVERSED: &str = "gl_Position.z = 0.5 * (gl_Position.w - gl_Position.z);";

#[test]
fn forward_zero_to_one_remaps_in_the_last_pre_raster_stage() {
    let out = T::gbuffers().vs(VS_UV).fs(FS_UV).run();
    contains_all(out.vs(), &[FORWARD, "invariant gl_Position;", "sb_user_main();"]);
    contains_none(out.vs(), &[REVERSED, "gl_Position.y = -gl_Position.y;"]);
    contains_none(out.fs(), &["gl_Position"]);
}

#[test]
fn reversed_zero_to_one_remap_and_gl_mode() {
    let out = T::gbuffers().vs(VS_UV).fs(FS_UV).with(|o| o.depth_mode = DepthMode::ReversedZeroToOne).run();
    contains_all(out.vs(), &[REVERSED]);
    contains_none(out.vs(), &[FORWARD]);
    let out = T::gbuffers().vs(VS_UV).fs(FS_UV).with(|o| o.depth_mode = DepthMode::GlNegOneToOne).run();
    contains_none(out.vs(), &[FORWARD, REVERSED]);
}

#[test]
fn flip_y_escape_hatch() {
    let out = T::gbuffers().vs(VS_UV).fs(FS_UV).with(|o| o.flip_y = true).run();
    contains_all(out.vs(), &["gl_Position.y = -gl_Position.y;", FORWARD]);
}

#[test]
fn early_return_still_runs_the_epilogue() {
    let vs = "#version 120\nuniform int frameCounter;\nvoid main() { gl_Position = ftransform(); if (frameCounter > 3) return; gl_Position.x += 1.0; }\n";
    let out = T::gbuffers().vs(vs).fs("#version 120\nvoid main() { gl_FragColor = vec4(1.0); }\n").run();
    let main_pos = out.vs().find("void main()").unwrap();
    let tail = &out.vs()[main_pos..];
    contains_all(tail, &["sb_user_main();", FORWARD]);
    assert!(tail.find("sb_user_main();").unwrap() < tail.find(FORWARD).unwrap());
}

#[test]
fn geometry_shader_remaps_before_every_emit() {
    let gs = "#version 150\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\nin vec2 uv[];\nout vec2 guv;\nvoid main() {\n  for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; guv = uv[i]; EmitVertex(); }\n  EndPrimitive();\n}\n";
    let vs = "#version 150\nin vec3 vaPosition;\nout vec2 uv;\nvoid main() { gl_Position = vec4(vaPosition, 1.0); uv = vec2(0.0); }\n";
    let fs = "#version 150\nin vec2 guv;\nout vec4 outColor0;\nvoid main() { outColor0 = vec4(guv, 0.0, 1.0); }\n";
    let out = T::gbuffers().vs(vs).gs(gs).fs(fs).run();
    contains_none(out.vs(), &[FORWARD, "invariant gl_Position"]);
    let g = out.glsl(ShaderStage::Geometry);
    contains_all(g, &["sb_emitVertex();", "void sb_emitVertex() {", FORWARD, "invariant gl_Position;"]);
    assert!(out.prog.requires_raw_vulkan);
}

#[test]
fn vertex_and_instance_ids() {
    let vs = "#version 130\nvoid main() { gl_Position = vec4(float(gl_VertexID), float(gl_InstanceID), 0.0, 1.0); }\n";
    let out = T::gbuffers().vs(vs).fs("#version 130\nvoid main() { gl_FragColor = vec4(1.0); }\n").run();
    contains_all(out.vs(), &["float(gl_VertexIndex)", "float(gl_InstanceIndex)"]);
    contains_none(out.vs(), &["gl_VertexID", "gl_InstanceID"]);
}

fn reversed() -> T {
    T::fullscreen().with(|o| {
        o.depth_mode = DepthMode::ReversedZeroToOne;
        o.invert_depth_reads = true;
    })
}

#[test]
fn reversed_depth_texture_reads_are_inverted() {
    let fs = "#version 130\nuniform sampler2D depthtex0;\nuniform sampler2D depthtex1;\nuniform sampler2D dhDepthTex;\nuniform sampler2D shadowtex1;\nuniform sampler2D colortex0;\nvarying vec2 texcoord;\nvoid main() {\n  float d = texture2D(depthtex0, texcoord).r + textureLod(depthtex1, texcoord, 0.0).x + texelFetch(dhDepthTex, ivec2(0), 0).r;\n  vec4 g = textureGather(shadowtex1, texcoord);\n  vec4 c = texture2D(colortex0, texcoord);\n  gl_FragData[0] = vec4(d) + g + c;\n}\n";
    let vs = "#version 130\nvarying vec2 texcoord;\nvoid main() { gl_Position = ftransform(); texcoord = gl_MultiTexCoord0.xy; }\n";
    let out = reversed().vs(vs).fs(fs).run();
    contains_all(
        out.fs(),
        &[
            "vec4(1.0 - texture(depthtex0, texcoord).x).r",
            "vec4(1.0 - textureLod(depthtex1, texcoord, 0.0).x).x",
            "vec4(1.0 - texelFetch(dhDepthTex0, ivec2(0), 0).x).r",
            "vec4(1.0) - textureGather(shadowtex1, texcoord)",
            "vec4 c = texture(colortex0, texcoord);",
        ],
    );
    // Forward mode leaves reads alone.
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_none(out.fs(), &["1.0 - texture", "vec4(1.0) -"]);
}

#[test]
fn reversed_depth_without_invert_flag_leaves_reads() {
    let fs = "#version 130\nuniform sampler2D depthtex0;\nvoid main() { gl_FragData[0] = texture2D(depthtex0, gl_FragCoord.xy); }\n";
    let out = T::fullscreen().fs(fs).vs(VS_UV).with(|o| o.depth_mode = DepthMode::ReversedZeroToOne).run();
    contains_none(out.fs(), &["1.0 - texture", "1.0 - gl_FragCoord.z"]);
}

#[test]
fn reversed_fragcoord_and_fragdepth() {
    let fs = "#version 130\nvoid main() {\n  gl_FragDepth = gl_FragCoord.z * 0.5;\n  gl_FragDepth += 0.1;\n  float back = gl_FragDepth;\n  gl_FragData[0] = vec4(back);\n}\n";
    let out = reversed().vs(VS_UV).fs(fs).run();
    contains_all(
        out.fs(),
        &[
            "gl_FragDepth = 1.0 - (vec4(gl_FragCoord.xy, 1.0 - gl_FragCoord.z, gl_FragCoord.w)).z * 0.5;",
            "gl_FragDepth = 1.0 - (1.0 - gl_FragDepth + 0.1);",
            "float back = 1.0 - gl_FragDepth;",
        ],
    );
    assert!(out.refl(ShaderStage::Fragment).execution_modes.iter().any(|m| format!("{m:?}").contains("DepthReplacing")));
}

#[test]
fn reversed_comparison_lookups_flip_the_reference() {
    let fs = "#version 130\nuniform sampler2DShadow shadowtex0;\nuniform sampler2DShadow shadow;\nvarying vec2 texcoord;\nvoid main() {\n  float a = shadow2D(shadowtex0, vec3(texcoord, 0.5)).r;\n  float b = shadow2DProj(shadowtex0, vec4(texcoord, 0.5, 1.0)).x;\n  float c = texture(shadowtex0, vec3(texcoord, 0.25));\n  gl_FragData[0] = vec4(a + b + c);\n}\n";
    let out = reversed().vs(VS_UV.replace("uv", "texcoord").as_str()).fs(fs).run();
    contains_all(
        out.fs(),
        &[
            "vec4(texture(shadowtex0, sb_revRef3(vec3(texcoord, 0.5)))).r",
            "vec4(textureProj(shadowtex0, sb_revRefProj(vec4(texcoord, 0.5, 1.0)))).x",
            "texture(shadowtex0, sb_revRef3(vec3(texcoord, 0.25)))",
            "vec3 sb_revRef3(vec3 p) { return vec3(p.xy, 1.0 - p.z); }",
            "vec4 sb_revRefProj(vec4 p) { return vec4(p.xy, p.w - p.z, p.w); }",
        ],
    );
}

#[test]
fn reversed_depth_reads_inside_user_functions_are_cloned() {
    let fs = "#version 130\nuniform sampler2D depthtex0;\nuniform sampler2D colortex1;\nvarying vec2 uv;\nfloat linear(sampler2D s, vec2 p) { return texture2D(s, p).r * 2.0; }\nfloat outer(sampler2D s, vec2 p) { return linear(s, p); }\nvoid main() { gl_FragData[0] = vec4(outer(depthtex0, uv), outer(colortex1, uv), 0.0, 1.0); }\n";
    let out = reversed().vs(VS_UV).fs(fs).run();
    contains_all(
        out.fs(),
        &[
            "float sb_depth_0_outer(sampler2D s, vec2 p)",
            "return sb_depth_0_linear(s, p);",
            "float sb_depth_0_linear(sampler2D s, vec2 p)",
            "return vec4(1.0 - texture(s, p).x).r * 2.0;",
            "outer(colortex1, uv)",
            "sb_depth_0_outer(depthtex0, uv)",
        ],
    );
}
