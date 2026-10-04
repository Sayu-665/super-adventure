//! Spec §2 (phase A, `StageInfo`) and §3.5 (line mapping back to the pack sources).

use sb_core::{MemorySources, ShaderStage};

use crate::harness::*;

fn analyze(stage: ShaderStage, src: &str) -> sb_transform::AnalyzedStage {
    let file = file_of(stage, "p");
    let sources = MemorySources::new().with(&file, src);
    analyze_file(&sources, stage, &file).unwrap_or_else(|d| panic!("{d:#?}"))
}

#[test]
fn stage_info_collects_declarations_and_uses() {
    let fs = "#version 120\n#extension GL_ARB_shader_texture_lod : enable\n\
              uniform float frameTimeCounter;\nuniform vec3 lights[4];\nuniform float strength = 1.5;\nuniform sampler2D gcolor;\nuniform sampler2DShadow shadowtex0;\n\
              varying vec2 uv;\nflat varying int id;\n\
              void main() { gl_FragData[0] = texture2D(gcolor, uv) * frameTimeCounter * strength + vec4(lights[1], darknessFactor); gl_FragData[2] = gl_Fog.color; }\n";
    let a = analyze(ShaderStage::Fragment, fs);
    let i = &a.info;
    assert_eq!(i.version, 120);
    assert!(i.compat);
    assert_eq!(a.extensions.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["GL_ARB_shader_texture_lod"]);
    let loose: Vec<(&str, String)> = i.loose_uniforms.iter().map(|u| (u.name.as_str(), u.ty.glsl_name())).collect();
    assert_eq!(loose, [("frameTimeCounter", "float".to_string()), ("lights", "vec3".to_string()), ("strength", "float".to_string())]);
    assert_eq!(i.loose_uniforms[1].ty.array, Some(4));
    assert_eq!(i.loose_uniforms[2].default, Some(vec![1.5]));
    assert_eq!(i.loose_uniforms[0].location.as_ref().map(|l| (l.file.as_str(), l.line)), Some(("p.fsh", 3)));
    let opaque: Vec<(&str, &str)> = i.opaque_uniforms.iter().map(|o| (o.name.as_str(), o.glsl_type.as_str())).collect();
    assert_eq!(opaque, [("gcolor", "sampler2D"), ("shadowtex0", "sampler2DShadow")]);
    let inputs: Vec<(&str, &str, Option<&str>, bool)> =
        i.inputs.iter().map(|v| (v.name.as_str(), v.ty.as_str(), v.interpolation.as_deref(), v.legacy)).collect();
    assert_eq!(inputs, [("uv", "vec2", None, true), ("id", "int", Some("flat"), true)]);
    assert_eq!(i.frag_data_indices.iter().copied().collect::<Vec<_>>(), [0, 2]);
    assert!(!i.frag_data_dynamic && !i.uses_frag_color);
    assert!(i.compat_builtins.contains("gl_Fog") && i.compat_builtins.contains("gl_FragData"));
    // Implicit uniforms: gl_Fog members, alphaTestRef (fragment), injected builtins.
    let implicit: Vec<&str> = i.implicit_uniforms.iter().map(|(n, _)| n.as_str()).collect();
    for n in ["sb_FogColor", "fogDensity", "alphaTestRef", "darknessFactor"] {
        assert!(implicit.contains(&n), "{n} not in {implicit:?}");
    }
    let decls = i.uniform_decls(Some("composite"));
    assert!(decls.iter().any(|d| d.name == "strength"));
}

#[test]
fn stage_info_of_core_and_compute_stages() {
    let vs = "#version 330 core\nin vec3 vaPosition;\nin vec4 mc_Entity;\nlayout(location = 2) out vec4 tint;\nout Data { vec2 uv; } o;\nvoid main() { gl_Position = vec4(vaPosition, 1.0); tint = mc_Entity; o.uv = vec2(0.0); }\n";
    let a = analyze(ShaderStage::Vertex, vs);
    assert!(!a.info.compat);
    assert!(a.info.iris_attributes.contains("vaPosition") && a.info.iris_attributes.contains("mc_Entity"));
    let outs: Vec<(&str, Option<u32>, bool)> = a.info.outputs.iter().map(|v| (v.name.as_str(), v.location, v.block)).collect();
    assert_eq!(outs, [("tint", Some(2), false), ("Data", None, true)]);
    let cs = "#version 430\nconst int N = 4;\nlayout(local_size_x = N * 2, local_size_y = 4) in;\nlayout(std430, binding = 3) buffer B { float v[]; };\nuniform U { vec4 x; };\nlayout(rgba16f) readonly uniform image2D colorimg1;\nvoid main() { v[0] = x.x + imageLoad(colorimg1, ivec2(0)).r; }\n";
    let a = analyze(ShaderStage::Compute, cs);
    assert_eq!(a.info.local_size, Some([8, 4, 1]));
    assert_eq!(a.info.storage_blocks[0].binding, Some(3));
    assert_eq!(a.info.uniform_blocks[0].name, "U");
    let img = &a.info.opaque_uniforms[0];
    assert_eq!((img.format.as_deref(), img.readonly, img.is_image()), (Some("rgba16f"), true, true));
}

#[test]
fn parse_errors_point_at_the_original_file() {
    let sources = MemorySources::new()
        .with("p.fsh", "#version 120\n#include \"/lib/a.glsl\"\nvoid main() { gl_FragColor = f(); }\n")
        .with("lib/a.glsl", "// helper\nvec4 f() {\n  return vec4(1.0) +;\n}\n");
    let d = analyze_file(&sources, ShaderStage::Fragment, "p.fsh").unwrap_err();
    let e = d.errors().next().unwrap();
    assert_eq!(e.code, "xf.parse");
    let loc = e.location.as_ref().unwrap();
    assert_eq!((loc.file.as_str(), loc.line), ("lib/a.glsl", 3));
}

#[test]
fn line_map_points_pack_code_at_its_source() {
    let out = T::fullscreen()
        .file("lib/util.glsl", "float twice(float x) {\n  return x * 2.0;\n}\n")
        .vs("#version 130\nvarying vec2 uv;\nvoid main() { gl_Position = ftransform(); uv = vec2(0.0); }\n")
        .fs("#version 130\n#include \"/lib/util.glsl\"\nvarying vec2 uv;\nvoid main() {\n  gl_FragData[0] = vec4(twice(uv.x));\n}\n")
        .run();
    let fs = out.prog.stages.iter().find(|s| s.stage == ShaderStage::Fragment).unwrap();
    assert_eq!(fs.line_map.len(), fs.glsl.lines().count());
    let find = |needle: &str| {
        let i = fs.glsl.lines().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle} not in\n{}", fs.glsl));
        fs.line_map[i].as_ref().map(|l| (l.file.clone(), l.line))
    };
    assert_eq!(find("return x * 2.0;"), Some(("lib/util.glsl".to_string(), 2)));
    assert_eq!(find("sb_FragData0 = vec4(twice(uv.x));"), Some(("p.fsh".to_string(), 5)));
    // Generated code maps nowhere.
    assert_eq!(find("#version 460"), None);
    assert_eq!(find("sb_user_main();"), None);
}

#[test]
fn compile_errors_map_through_the_line_map() {
    // A pack bug (undeclared identifier) is reported at the pack's own file and line.
    let prog = T::fullscreen()
        .vs("#version 130\nvoid main() { gl_Position = ftransform(); }\n")
        .fs("#version 130\nvoid main() {\n  gl_FragData[0] = vec4(undeclaredValue);\n}\n")
        .translate_ok();
    let fs = prog.stages.iter().find(|s| s.stage == ShaderStage::Fragment).unwrap();
    let err = sb_compile::compile_glsl(&fs.glsl, fs.stage, "p.fsh", &sb_compile::CompileOptions::default(), Some(&fs.line_map)).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("p.fsh:3"), "{text}");
}

#[test]
fn line_map_survives_comments_literal_rewrites_and_split_declarations() {
    // Multi-line block comments (blanked before parsing), wrapped integer literals, a
    // global `const` declaration split by demotion and an include all keep their lines;
    // a glslang error inside an included function maps to the include's own line.
    let prog = T::fullscreen()
        .file("lib/b.glsl", "/* helper\n   functions **/\nfloat g(float x) {\n  float y = x * 2.0;\n  return y + undeclaredB;\n}\n")
        .vs("#version 130\nvoid main() { gl_Position = ftransform(); }\n")
        .fs("#version 130\n/* multi\n line\n comment **/\nuniform float viewWidth;\nconst float k = 2.0, w = viewWidth;\n#include \"/lib/b.glsl\"\nvoid main() {\n  int big = 0xFFFFFFFF;\n  gl_FragData[0] = vec4(g(w * k) + float(big));\n}\n")
        .translate_ok();
    let fs = prog.stages.iter().find(|s| s.stage == ShaderStage::Fragment).unwrap();
    let find = |needle: &str| {
        let i = fs.glsl.lines().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle} not in\n{}", fs.glsl));
        fs.line_map[i].as_ref().map(|l| (l.file.clone(), l.line))
    };
    assert_eq!(find("int big = int(4294967295u);"), Some(("p.fsh".to_string(), 9)));
    assert_eq!(find("const float k = 2.0;"), Some(("p.fsh".to_string(), 6)));
    assert_eq!(find("float w = viewWidth;"), Some(("p.fsh".to_string(), 6)));
    assert_eq!(find("float y = x * 2.0;"), Some(("lib/b.glsl".to_string(), 4)));
    let err = sb_compile::compile_glsl(&fs.glsl, fs.stage, "p.fsh", &sb_compile::CompileOptions::default(), Some(&fs.line_map)).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("lib/b.glsl:5") && text.contains("undeclaredB"), "{text}");
}
