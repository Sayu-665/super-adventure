//! ARCHITECTURE §8 "Fixups for leniency" / spec §3.2: code lenient (NVIDIA) drivers
//! accept and glslang rejects, identifier hygiene, and the glsl-lang workarounds.

use sb_core::ShaderStage;

use crate::harness::*;

const VS: &str = "#version 130\nvoid main() { gl_Position = ftransform(); }\n";

/// A fragment-only test (fullscreen program with a trivial vertex shader).
fn fs(src: &str) -> Out {
    T::fullscreen().vs(VS).fs(src).run()
}

#[test]
fn unused_functions_are_removed_transitively() {
    // `dead` would not compile (undeclared identifier); unreachable code is never
    // compiled by lenient drivers either.
    let out = fs("#version 130\nfloat helper(float x) { return x * 2.0; }\nfloat helper(vec2 v) { return v.x; }\n\
                  float dead() { return undeclaredThing * helper(1.0); }\nfloat used() { return helper(0.5); }\n\
                  void main() { gl_FragData[0] = vec4(used()); }\n");
    contains_all(out.fs(), &["float used()", "float helper(float x)"]);
    contains_none(out.fs(), &["dead", "undeclaredThing"]);
}

#[test]
fn const_locals_initialized_from_const_parameters_lose_const() {
    let out = fs("#version 130\nfloat f(const float x) { const float y = x * 2.0; const float z = y + 1.0; const float k = 3.0; return z * k; }\n\
                  void main() { gl_FragData[0] = vec4(f(1.0)); }\n");
    contains_all(out.fs(), &["float y = x * 2.0;", "float z = y + 1.0;", "const float k = 3.0;"]);
}

#[test]
fn lone_semicolons_and_precision_qualifiers_are_dropped() {
    let out = fs("#version 130\nprecision highp float;\n;\nlowp vec4 tint = vec4(1.0);;\nmediump float f(highp float x) { highp float y = x; return y; };\n\
                  void main() { gl_FragData[0] = tint * f(1.0); }\n");
    contains_none(out.fs(), &["precision", "highp", "mediump", "lowp"]);
    contains_all(out.fs(), &["vec4 tint = vec4(1.0);", "float f(float x)"]);
}

#[test]
fn unsized_struct_member_arrays_move_to_the_declarator() {
    let cs = "#version 430\nlayout(local_size_x = 1) in;\nstruct Light { vec4 color; };\nlayout(std430, binding = 3) buffer Lights { uint count; Light[] lights; };\n\
              void main() { lights[gl_GlobalInvocationID.x].color = vec4(float(count)); }\n";
    let out = T::new("fullscreen").with(|o| o.program_class = sb_uniforms::ProgramClass::Compute).stage(ShaderStage::Compute, cs).run();
    contains_all(out.glsl(ShaderStage::Compute), &["Light lights[];"]);
}

#[test]
fn implicit_widening_conversions_compile_at_460() {
    // GLSL 4.60 converts int -> uint/float implicitly; no rewrite is needed.
    let out = fs("#version 120\nuint u = 1;\nfloat f = 1;\nvec3 v = vec3(1);\nfloat g() { return 1; }\nuint h(uint x) { return x + 1; }\n\
                  void main() { float k = true ? 1 : 2.0; uint w = u + 1; gl_FragData[0] = vec4(f + g() + k + float(h(2)) + float(w), v); }\n");
    contains_all(out.fs(), &["uint u = 1;", "float f = 1;", "return 1;"]);
}

#[test]
fn implicit_narrowing_conversions_are_made_explicit() {
    let out = fs("#version 130\nuniform float frameTimeCounter;\nuint packed;\nint toInt(float x) { return x * 2.0; }\n\
                  void main() {\n  uint id;\n  id = float(frameTimeCounter > 1.0);\n  int i = id;\n  ivec2 p = gl_FragCoord.xy;\n  uint u = 2.5;\n  float f = 1;\n  packed = 3.0;\n\
                  gl_FragData[0] = vec4(float(id) + float(i) + float(p.x) + float(u) + f + float(toInt(0.5)));\n}\n");
    contains_all(
        out.fs(),
        &[
            "id = uint(float(frameTimeCounter > 1.0));",
            "int i = int(id);",
            "ivec2 p = ivec2(gl_FragCoord.xy);",
            "uint u = uint(2.5);",
            "float f = 1;",
            "packed = uint(3.0);",
            "return int(x * 2.0);",
        ],
    );
    assert!(out.has_diag("xf.implicit-conversion"));
    let cs = "#version 430\nlayout(local_size_x = 8, local_size_y = 8) in;\nlayout(rgba8) uniform writeonly image2D colorimg0;\n\
              void main() { int x = gl_GlobalInvocationID.x; ivec2 p = gl_GlobalInvocationID.xy; imageStore(colorimg0, p + ivec2(x), vec4(1.0)); }\n";
    let out = T::new("fullscreen").with(|o| o.program_class = sb_uniforms::ProgramClass::Compute).stage(ShaderStage::Compute, cs).run();
    contains_all(out.glsl(ShaderStage::Compute), &["int x = int(gl_GlobalInvocationID.x);", "ivec2 p = ivec2(gl_GlobalInvocationID.xy);"]);
}

#[test]
fn missing_returns_get_a_default_value() {
    let out = fs("#version 130\nfloat f(float x) { if (x > 0.0) return x; }\nvec3 g(bool c) { if (c) { return vec3(1.0); } else { return vec3(0.0); } }\n\
                  void main() { gl_FragData[0] = vec4(g(true), f(1.0)); }\n");
    contains_all(out.fs(), &["return float(0);"]);
    contains_none(out.fs(), &["return vec3(0);"]);
}

#[test]
fn global_consts_with_non_constant_initializers_are_demoted() {
    let out = fs("#version 330\nuniform float viewWidth;\nconst float a = 2.0 * sqrt(4.0);\nconst float b = a / viewWidth;\nconst mat3 m = transpose(mat3(1.0));\nconst int n = int(a);\nconst float arr[n] = float[n](1.0, 2.0, 3.0, 4.0);\n\
                  out vec4 c;\nvoid main() { c = vec4(a + b + m[0].x + arr[1]); }\n");
    contains_all(out.fs(), &["const float a =", "\nfloat b = a / viewWidth;", "\nmat3 m = transpose(mat3(1.0));", "const int n = int(a);", "const float arr[n]"]);
}

#[test]
fn pack_functions_named_like_builtins_are_renamed() {
    let out = fs("#version 130\nfloat fma(float a, float b, float c) { return a * b + c; }\nvec4 textureGather(vec2 uv) { return vec4(uv, 0.0, 1.0); }\nfloat min3(float a, float b, float c) { return min(a, min(b, c)); }\n\
                  float saturate(float x) { return clamp(x, 0.0, 1.0); }\n\
                  void main() { gl_FragData[0] = textureGather(vec2(0.5)) * fma(1.0, 2.0, 3.0) * min3(1.0, 2.0, 3.0) * saturate(2.0); }\n");
    contains_all(
        out.fs(),
        &["float sb_u_fma(float a, float b, float c)", "vec4 sb_u_textureGather(vec2 uv)", "float sb_u_min3(", "sb_u_fma(1.0, 2.0, 3.0)", "float saturate(float x)"],
    );
}

#[test]
fn amd_trinary_minmax_is_polyfilled() {
    let out = fs("#version 130\n#extension GL_AMD_shader_trinary_minmax : enable\nvoid main() { gl_FragData[0] = vec4(min3(1.0, 2.0, 3.0), max3(vec2(1.0), vec2(2.0), vec2(0.0)), mid3(1, 2, 3)); }\n");
    contains_all(out.fs(), &["sb_min3(1.0, 2.0, 3.0)", "float sb_min3(float a, float b, float c)", "vec2 sb_max3(vec2 a, vec2 b, vec2 c)", "int sb_mid3(int a, int b, int c)"]);
    contains_none(out.fs(), &["GL_AMD_shader_trinary_minmax"]);
}

#[test]
fn reserved_words_and_vulkan_type_keywords_are_escaped() {
    let out = fs("#version 120\nfloat sampler = 1.0;\nfloat texture2D = 2.0;\nvec3 shared(vec3 x) { return x; }\nfloat resource = 3.0;\n\
                  void main() { float common = sampler + texture2D + resource; gl_FragData[0] = vec4(shared(vec3(common)), 1.0); }\n");
    contains_all(
        out.fs(),
        &["float sb_kw_sampler = 1.0;", "float sb_kw_texture2D = 2.0;", "float sb_kw_resource = 3.0;", "sb_kw_common", "sb_kw_shared(vec3(sb_kw_common))"],
    );
}

#[test]
fn variables_named_texture_and_other_builtins_are_renamed() {
    let out = fs("#version 120\nuniform sampler2D colortex0;\nfloat step = 0.25;\n\
                  vec4 f(vec2 uv) { vec4 texture = texture2D(colortex0, uv); float length = 2.0; return texture * length; }\n\
                  void main() { gl_FragData[0] = f(vec2(step(0.5, 1.0))) * step + vec4(length(vec2(1.0))); }\n");
    contains_all(
        out.fs(),
        &[
            "vec4 sb_kw_texture = texture(colortex0, uv);",
            "float sb_kw_length = 2.0;",
            "return sb_kw_texture * sb_kw_length;",
            "float sb_kw_step = 0.25;",
            "f(vec2(step(0.5, 1.0))) * sb_kw_step",
            "vec4(length(vec2(1.0)))",
        ],
    );
}

#[test]
fn sampler_named_texture_is_the_atlas() {
    let out = T::gbuffers()
        .vs("#version 120\nvarying vec2 uv;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.xy; }\n")
        .fs("#version 120\nuniform sampler2D texture;\nuniform sampler2D gtexture;\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = texture2D(texture, uv) + texture(gtexture, uv); }\n")
        .run();
    // Both names canonicalize to the atlas, declared once (as the host sampler).
    contains_all(out.fs(), &["texture(Sampler0, uv) + texture(Sampler0, uv)"]);
    assert_eq!(out.fs().matches("uniform sampler2D Sampler0;").count(), 1);
}

#[test]
fn pack_identifiers_with_our_prefix_are_renamed() {
    let out = fs("#version 130\nfloat sb_value = 1.0;\nfloat sb_kw_input = 2.0;\nvec3 sb_helper(vec3 x) { return x; }\nvoid main() { gl_FragData[0] = vec4(sb_helper(vec3(sb_value + sb_kw_input)), 1.0); }\n");
    contains_all(out.fs(), &["float sbu_value = 1.0;", "float sb_kw_input = 2.0;", "vec3 sbu_helper(vec3 x)"]);
}

#[test]
fn identical_duplicate_declarations_are_removed() {
    // (A struct defined twice is already a syntax error for the parser, as for drivers.)
    let out = fs("#version 130\nconst float PI = 3.14159;\nfloat sq(float x) { return x * x; }\nvarying vec2 uv;\n\
                  const float PI = 3.14159;\nfloat sq(float x) { return x * x; }\nvarying vec2 uv;\n\
                  void main() { gl_FragData[0] = vec4(PI * sq(2.0), uv, 1.0); }\n");
    assert_eq!(out.fs().matches("const float PI = 3.14159;").count(), 1);
    assert_eq!(out.fs().matches("float sq(float x)").count(), 1);
}

#[test]
fn opaque_ternaries_are_hoisted_out_of_calls() {
    let out = fs("#version 130\nuniform sampler2D colortex1;\nuniform sampler2D colortex2;\nuniform int frameCounter;\n\
                  vec4 sampleIt(sampler2D s, vec2 uv) { return texture(s, uv); }\n\
                  void main() { bool odd = (frameCounter & 1) == 1;\n  vec4 a = textureLod(odd ? colortex1 : colortex2, vec2(0.5), 0.0);\n  vec4 b = sampleIt(odd ? colortex2 : colortex1, vec2(0.25));\n  gl_FragData[0] = a + b; }\n");
    contains_all(
        out.fs(),
        &[
            "vec4 a = odd ? textureLod(colortex1, vec2(0.5), 0.0) : textureLod(colortex2, vec2(0.5), 0.0);",
            "vec4 b = odd ? sampleIt(colortex2, vec2(0.25)) : sampleIt(colortex1, vec2(0.25));",
        ],
    );
    assert!(out.has_diag("xf.opaque-ternary"));
}

#[test]
fn non_constant_texel_offsets_are_folded_into_the_coordinate() {
    let out = fs("#version 130\nuniform sampler2D colortex0;\nvec4 tap(const ivec2 o) { return textureOffset(colortex0, vec2(0.5), o) + texelFetchOffset(colortex0, ivec2(1), 0, o) + textureLodOffset(colortex0, vec2(0.5), 1.0, o); }\n\
                  void main() { gl_FragData[0] = tap(ivec2(1, 0)) + textureOffset(colortex0, vec2(0.5), ivec2(1, 1)); }\n");
    contains_all(
        out.fs(),
        &[
            "texture(colortex0, vec2(0.5) + vec2(o) / vec2(textureSize(colortex0, 0)))",
            "texelFetch(colortex0, ivec2(1) + o, 0)",
            "textureLod(colortex0, vec2(0.5) + vec2(o) / vec2(textureSize(colortex0, int(1.0))), 1.0)",
            // Constant offsets keep the offset form.
            "textureOffset(colortex0, vec2(0.5), ivec2(1, 1))",
        ],
    );
    assert!(out.has_diag("xf.dynamic-offset"));
}

#[test]
fn glsl_lang_workarounds() {
    // Unsuffixed literals >= 0x80000000 are legal (negative ints); f32-overflowing float
    // literals print as bit patterns, not `inf`.
    let out = fs("#version 330\nout vec4 c;\nvoid main() { int a = 0x80000000; int b = 4294967295; float big = 1e39; float tiny = -1e39; c = vec4(float(a), float(b), big, tiny); }\n");
    contains_all(out.fs(), &["int a = int(2147483648u);", "int b = int(4294967295u);", "float big = uintBitsToFloat(0x7F800000u);", "float tiny = -uintBitsToFloat(0x7F800000u);"]);
    contains_none(out.fs(), &["inf"]);
}

#[test]
fn global_initializers_reading_uniforms_are_kept() {
    let out = fs("#version 130\nuniform float viewWidth;\nuniform float viewHeight;\nvec2 texel = 1.0 / vec2(viewWidth, viewHeight);\nvoid main() { gl_FragData[0] = vec4(texel, 0.0, 1.0); }\n");
    contains_all(out.fs(), &["vec2 texel = 1.0 / vec2(viewWidth, viewHeight);"]);
}
