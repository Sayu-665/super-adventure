//! Differential test against glslang's preprocessor (`glslangValidator -E`)
//! on the subset of the C preprocessor that glslang supports (it rejects `?:`
//! in `#if`, pasting of numbers and empty macro arguments, all of which this
//! crate supports). Output is compared with all whitespace removed, because the
//! two place multi-line macro expansions on different lines. Skipped when
//! `glslangValidator` is not installed.

use std::process::Command;

use sb_core::MemorySources;
use sb_preprocess::{PreprocessOptions, Preprocessor};

const CASES: &[&str] = &[
    // Object- and function-like macros, nesting, rescanning.
    "#define PI 3.14159\n#define SQ(x) ((x) * (x))\n#define AREA(r) (PI * SQ(r))\nfloat a = AREA(2.0 + 1.0);\n",
    "#define A B\n#define B C\n#define C 42\nint v = A;\n",
    // Self-reference and mutual recursion are blocked.
    "#define foo foo + 1\n#define a b\n#define b a\nint x = foo; int y = a; int z = b;\n",
    "#define f(x) f(x + 1)\nint g = f(0);\n",
    // C99 6.10.3.5 EXAMPLE 3 (the parts glslang supports).
    "#define x 3\n#define f(a) f(x * (a))\n#undef x\n#define x 2\n#define g f\n#define z z[0]\n#define h g(~\n#define m(a) a(w)\n#define w 0,1\n#define t(a) a\nf(y+1) + f(f(z)) % t(t(g)(0) + t)(1);\ng(x+(3,4)-w) | h 5) & m\n(f)^m(m);\n",
    // Identifier pasting.
    "#define CAT(a, b) a ## b\n#define XCAT(a, b) CAT(a, b)\n#define N 3\nvec3 CAT(my, Var); XCAT(vec, N) w;\n",
    // Conditionals and expressions.
    "#define LEVEL 3\n#if LEVEL > 2 && defined(LEVEL) && !defined(NOPE)\nint hi;\n#elif LEVEL > 1\nint mid;\n#else\nint lo;\n#endif\n",
    "#if (1 << 4) == 16 && (17 % 5) == 2 && (-3 / 2) == -1 && (6 & 3) == 2 && (6 | 1) == 7 && (6 ^ 2) == 4\nint ok;\n#endif\n",
    "#ifdef A\nint a;\n#else\n#ifndef B\nint b;\n#endif\n#endif\n#define B\n#ifdef B\nint b2;\n#endif\n#undef B\n#ifndef B\nint b3;\n#endif\n",
    "#if 0\n#if garbage\n#error inner\n#endif\n#else\nint taken;\n#endif\n",
    // Multi-line invocation and comments inside arguments.
    "#define F(a, b) a + b\nint s = F(1, /* comment */\n  2) + F(3,\n// line comment\n  4);\nint after;\n",
    // Built-ins.
    "int l = __LINE__;\nint v = __VERSION__;\n#ifdef GL_core_profile\nint core;\n#endif\n",
    // Function-like macro name without arguments.
    "#define F(x) [x]\nint F;\nint G = F (1);\n",
];

fn glslang_available() -> bool {
    Command::new("glslangValidator")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn strip_ws(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn matches_glslang_preprocessor_on_common_subset() {
    if !glslang_available() {
        eprintln!("glslangValidator not found; skipping differential test");
        return;
    }
    let dir = std::env::temp_dir().join(format!("sb-preprocess-diff-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let mut mismatches = Vec::new();
    for (i, body) in CASES.iter().enumerate() {
        let src = format!("#version 450\n{body}");
        let path = dir.join(format!("case{i}.frag"));
        std::fs::write(&path, &src).expect("write temp file");
        let out = Command::new("glslangValidator")
            .arg("-E")
            .arg(&path)
            .output()
            .expect("run glslang");
        assert!(
            out.status.success(),
            "glslang failed on case {i}:\n{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let theirs: String = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim_start().starts_with("#version"))
            .collect::<Vec<_>>()
            .join("\n");

        let m = MemorySources::new().with("case.frag", &src);
        let mut pp = Preprocessor::new(&m);
        let opts = PreprocessOptions {
            keep_comments: false,
            ..Default::default()
        };
        let ours = pp.preprocess("case.frag", &opts);
        assert!(
            !ours.diagnostics.has_errors(),
            "case {i}: {:?}",
            ours.diagnostics
        );
        if strip_ws(&theirs) != strip_ws(&ours.code) {
            mismatches.push(format!(
                "case {i}:\n  glslang: {}\n  ours:    {}",
                strip_ws(&theirs),
                strip_ws(&ours.code)
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        mismatches.is_empty(),
        "differences from glslang:\n{}",
        mismatches.join("\n")
    );
}
