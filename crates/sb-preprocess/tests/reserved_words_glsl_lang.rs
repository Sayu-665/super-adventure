//! Keeps the reserved-word escape lists honest: checks which words glsl-lang
//! 0.8 rejects as identifiers, and that every word parses once escaped.
//! The context-sensitive words ([`CONTEXTUAL_RESERVED`]) are also checked to
//! be rejected by glsl-lang at every version, and their keyword uses in old
//! versions to survive preprocessing and parse.

use glsl_lang::ast::TranslationUnit;
use glsl_lang::parse::DefaultParse;
use sb_core::MemorySources;
use sb_preprocess::escape::{
    ALWAYS_RESERVED, CONTEXTUAL_RESERVED, RESERVED_BELOW_400, RESERVED_BELOW_430,
};
use sb_preprocess::{PreprocessOptions, Preprocessor};

/// Words `glslangValidator -V` (15.1) rejects as identifiers in a `#version 450`
/// Vulkan shader (measured; see `escape` module docs).
const GLSLANG_REJECTS_AT_450: &[&str] = &[
    "input",
    "output",
    "filter",
    "common",
    "partition",
    "active",
    "superp",
    "namespace",
    "using",
    "cast",
    "sizeof",
    "external",
    "interface",
    "template",
    "this",
    "class",
    "union",
    "enum",
    "typedef",
    "goto",
    "inline",
    "noinline",
    "public",
    "static",
    "extern",
    "asm",
    "unsigned",
    "fvec2",
    "fvec3",
    "fvec4",
    "hvec2",
    "hvec3",
    "hvec4",
    "half",
    "fixed",
    "long",
    "short",
    "resource",
    "sampler",
    "samplerShadow",
    "sample",
    "subroutine",
    "patch",
    "precise",
    "buffer",
    "shared",
    "smooth",
    "flat",
    "noperspective",
    "coherent",
    "volatile",
    "restrict",
    "readonly",
    "writeonly",
    "layout",
    "precision",
    "lowp",
    "mediump",
    "highp",
    "switch",
    "case",
    "default",
    "double",
];

fn glsl_lang_accepts(src: &str) -> bool {
    TranslationUnit::parse(src).is_ok()
}

fn all_words() -> Vec<&'static str> {
    ALWAYS_RESERVED
        .iter()
        .copied()
        .chain(RESERVED_BELOW_400.iter().map(|(w, _)| *w))
        .chain(RESERVED_BELOW_430.iter().copied())
        .chain(CONTEXTUAL_RESERVED.iter().map(|w| w.word))
        .collect()
}

/// The premise of contextual escaping: glsl-lang rejects these words as
/// identifiers at every version, including the old ones where lenient
/// compilers accept them (so escaping identifier uses can only help).
#[test]
fn glsl_lang_rejects_contextual_words_at_every_version() {
    let mut accepted = Vec::new();
    for w in CONTEXTUAL_RESERVED {
        for v in [110, 120, 130, 140, 150, 330, 400, 420, 450] {
            let src = format!(
                "#version {v}\nvoid main() {{ float {w} = 1.0; {w} += 1.0; }}\n",
                w = w.word
            );
            if glsl_lang_accepts(&src) {
                accepted.push(format!("{} at {v}", w.word));
            }
        }
    }
    assert!(accepted.is_empty(), "glsl-lang accepts: {accepted:?}");
}

/// Keyword uses of the contextual words in old versions (accepted by lenient
/// compilers) are not escaped and parse as before.
#[test]
fn contextual_keyword_uses_survive_and_parse() {
    let src = "#version 120\n#extension GL_EXT_gpu_shader4 : enable\nflat varying vec3 n;\nnoperspective varying float d;\nsmooth varying vec2 uv;\nprecision highp float;\nuniform lowp sampler2D t;\nvoid main() {\n  int i = int(d);\n  switch (i) { case 0: i = 1; break; case -1: case (2): break; default: i = 3; }\n  mediump vec3 c = n;\n  gl_FragData[0] = vec4(c, float(i));\n}\n";
    let m = MemorySources::new().with("main.fsh", src);
    let mut pp = Preprocessor::new(&m);
    let out = pp.preprocess("main.fsh", &PreprocessOptions::default());
    assert!(!out.code.contains("sb_kw_"), "{}", out.code);
    // Parsed as at least 130, like sb-transform does.
    let glsl = format!("#version 130\n{}", out.code);
    if let Err(e) = TranslationUnit::parse(glsl.as_str()) {
        panic!("does not parse: {e}\n{glsl}");
    }
}

#[test]
fn glsl_lang_rejects_the_documented_words() {
    let mut rejected = Vec::new();
    let mut accepted = Vec::new();
    for w in all_words() {
        let src = format!("#version 450\nvoid main() {{ float {w} = 1.0; {w} += 1.0; }}\n");
        if glsl_lang_accepts(&src) {
            accepted.push(w);
        } else {
            rejected.push(w);
        }
    }
    println!("glsl-lang rejects as identifiers: {rejected:?}");
    println!("glsl-lang accepts as identifiers: {accepted:?}");
    for w in [
        "sample",
        "input",
        "output",
        "filter",
        "common",
        "partition",
        "active",
    ] {
        assert!(
            rejected.contains(&w),
            "glsl-lang was expected to reject '{w}'"
        );
    }
    // Every escaped word is rejected by glsl-lang or by glslang (Vulkan, 450).
    for w in &accepted {
        assert!(
            GLSLANG_REJECTS_AT_450.contains(w),
            "'{w}' is accepted by glsl-lang and not known to be rejected by glslang; remove it from the escape list"
        );
    }
    // Control: ordinary identifiers parse, and so does `packed` (hence not escaped).
    assert!(glsl_lang_accepts(
        "#version 450\nvoid main() { float color = 1.0; }\n"
    ));
    assert!(glsl_lang_accepts(
        "#version 450\nvoid main() { float packed = 1.0; }\n"
    ));
}

#[test]
fn escaped_words_parse_with_glsl_lang() {
    let mut src = String::from("#version 120\nvoid main() {\n");
    for w in all_words() {
        src.push_str(&format!("  float {w} = 1.0; {w} += 2.0;\n"));
    }
    src.push_str("}\n");
    let m = MemorySources::new().with("main.fsh", &src);
    let mut pp = Preprocessor::new(&m);
    let out = pp.preprocess("main.fsh", &PreprocessOptions::default());
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    for w in all_words() {
        assert!(
            out.code.contains(&format!("float sb_kw_{w} = 1.0;")),
            "{w} not escaped"
        );
    }
    let glsl = format!("#version 450\n{}", out.code);
    if let Err(e) = TranslationUnit::parse(glsl.as_str()) {
        panic!("escaped output does not parse: {e}\n{glsl}");
    }
}

#[test]
fn qualifier_uses_survive_at_high_versions() {
    let src = "#version 430\nlayout(local_size_x = 8) in;\nshared vec4 cache[8];\nlayout(std430, binding = 0) buffer Data { float values[]; };\nvoid main() { cache[0] = vec4(values[0]); }\n";
    let m = MemorySources::new().with("main.csh", src);
    let mut pp = Preprocessor::new(&m);
    let out = pp.preprocess("main.csh", &PreprocessOptions::default());
    assert!(!out.code.contains("sb_kw_"), "{}", out.code);
    let glsl = out.to_glsl();
    if let Err(e) = TranslationUnit::parse(glsl.as_str()) {
        panic!("does not parse: {e}\n{glsl}");
    }
}

/// Keeps `GLSLANG_REJECTS_AT_450` (and therefore the escape lists) honest by
/// measuring it with `glslangValidator -V` (Vulkan, `#version 450`) when it is
/// installed: every escaped word must be rejected as an identifier, and
/// `packed` (deliberately not escaped) must be accepted.
#[test]
fn escape_list_matches_glslang() {
    use std::process::Command;
    let available = Command::new("glslangValidator")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !available {
        eprintln!("glslangValidator not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("sb-preprocess-kw-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let accepts = |w: &str| {
        let path = dir.join(format!("{w}.frag"));
        let src = format!("#version 450\nvoid main() {{ float {w} = 1.0; {w} += 1.0; }}\n");
        std::fs::write(&path, src).expect("write shader");
        Command::new("glslangValidator")
            .arg("-V")
            .arg(&path)
            .arg("-o")
            .arg(dir.join(format!("{w}.spv")))
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let wrongly_accepted: Vec<&str> = all_words().into_iter().filter(|w| accepts(w)).collect();
    let packed_ok = accepts("packed");
    let control_ok = accepts("color");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(control_ok, "control shader must compile");
    assert!(
        wrongly_accepted.is_empty(),
        "glslang accepts these escaped words as identifiers: {wrongly_accepted:?}"
    );
    assert!(packed_ok, "glslang now rejects 'packed'; add it to ALWAYS_RESERVED");
    for w in all_words() {
        assert!(GLSLANG_REJECTS_AT_450.contains(&w), "{w} missing from GLSLANG_REJECTS_AT_450");
    }
}
