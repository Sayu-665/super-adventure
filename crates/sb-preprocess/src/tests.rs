//! Engine unit tests: every directive, macro edge cases (including the
//! examples from C99 6.10.3.5), line mapping, escaping and robustness.

use super::*;
use pretty_assertions::assert_eq;
use sb_core::{MemorySources, Severity};

fn check_invariants(out: &Preprocessed) {
    assert_eq!(
        out.line_map.len(),
        out.code.lines().count(),
        "line_map must have one entry per output line"
    );
    assert!(out.code.is_empty() || out.code.ends_with('\n'));
}

fn run_files(files: &[(&str, &str)], entry: &str, opts: &PreprocessOptions) -> Preprocessed {
    let mut m = MemorySources::new();
    for (p, t) in files {
        m.insert(p, t);
    }
    let mut pp = Preprocessor::new(&m);
    let out = pp.preprocess(entry, opts);
    check_invariants(&out);
    out
}

fn run_opts(src: &str, opts: &PreprocessOptions) -> Preprocessed {
    run_files(&[("main.fsh", src)], "main.fsh", opts)
}

fn run(src: &str) -> Preprocessed {
    run_opts(src, &PreprocessOptions::default())
}

/// Non-empty lines with whitespace runs collapsed.
fn norm(code: &str) -> Vec<String> {
    code.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect()
}

/// All whitespace removed, non-empty lines.
fn nows(code: &str) -> Vec<String> {
    code.lines()
        .map(|l| l.split_whitespace().collect::<String>())
        .filter(|l| !l.is_empty())
        .collect()
}

fn codes(out: &Preprocessed) -> Vec<&str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn clean(src: &str) -> Vec<String> {
    let out = run(src);
    assert!(
        out.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        out.diagnostics
    );
    norm(&out.code)
}

// ----------------------------------------------------------------------
// Includes
// ----------------------------------------------------------------------

#[test]
fn include_expands_and_maps_lines() {
    let out = run_files(
        &[
            (
                "world0/composite.fsh",
                "#version 330\n#include \"/lib/a.glsl\"\nvoid main() { A(); }\n",
            ),
            ("lib/a.glsl", "// lib a\n#include \"b.glsl\"\nvoid A() {}\n"),
            ("lib/b.glsl", "int b;\n"),
        ],
        "world0/composite.fsh",
        &PreprocessOptions::default(),
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(
        out.code,
        "\n// lib a\nint b;\nvoid A() {}\nvoid main() { A(); }\n"
    );
    let map: Vec<(String, u32)> = out
        .line_map
        .iter()
        .map(|l| (l.file.clone(), l.line))
        .collect();
    assert_eq!(
        map,
        [
            ("world0/composite.fsh".to_string(), 1),
            ("lib/a.glsl".into(), 1),
            ("lib/b.glsl".into(), 1),
            ("lib/a.glsl".into(), 3),
            ("world0/composite.fsh".into(), 3)
        ]
    );
    assert_eq!(
        out.files,
        ["world0/composite.fsh", "lib/a.glsl", "lib/b.glsl"]
    );
    assert_eq!(out.location(3).map(|l| l.file.as_str()), Some("lib/b.glsl"));
}

#[test]
fn include_is_unconditional() {
    // Included even inside `#if 0` (its content is inactive there) ...
    let out = run_files(
        &[
            ("main.fsh", "#if 0\n#include \"x.glsl\"\n#endif\nX\n"),
            ("x.glsl", "#define X 1\n"),
        ],
        "main.fsh",
        &PreprocessOptions::default(),
    );
    assert_eq!(norm(&out.code), ["X"]);
    assert_eq!(out.files, ["main.fsh", "x.glsl"]);
    // ... and a missing file in an inactive region is still an error (as in Iris).
    let out = run("#if 0\n#include \"missing.glsl\"\n#endif\n");
    assert_eq!(codes(&out), ["pp.include-missing"]);
    // Inside a block comment the included text becomes part of the comment.
    let out = run_files(
        &[
            ("main.fsh", "/*\n#include \"x.glsl\"\n*/\nX\n"),
            ("x.glsl", "#define X 1\n"),
        ],
        "main.fsh",
        &PreprocessOptions::default(),
    );
    assert_eq!(out.code, "/*\n#define X 1\n*/\nX\n");
}

#[test]
fn missing_include_reports_and_continues() {
    let out = run("a\n#include \"/lib/nope.glsl\"\nb\n");
    assert_eq!(norm(&out.code), ["a", "b"]);
    let d = out.diagnostics.iter().next().expect("diagnostic");
    assert_eq!(d.code, "pp.include-missing");
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.location, Some(SourceLocation::new("main.fsh", 2)));
}

#[test]
fn include_cycles_are_skipped_with_warning() {
    let out = run_files(
        &[
            ("a.glsl", "A1\n#include \"b.glsl\"\nA2\n"),
            ("b.glsl", "B1\n#include \"a.glsl\"\nB2\n"),
        ],
        "a.glsl",
        &PreprocessOptions::default(),
    );
    assert_eq!(norm(&out.code), ["A1", "B1", "B2", "A2"]);
    assert_eq!(codes(&out), ["pp.include-cycle"]);
    assert!(!out.diagnostics.has_errors());
    let out = run("#include \"main.fsh\"\nx\n");
    assert_eq!(codes(&out), ["pp.include-cycle"]);
    assert_eq!(norm(&out.code), ["x"]);
}

#[test]
fn repeated_includes_with_guards() {
    let out = run_files(
        &[
            ("main.fsh", "#include \"g.glsl\"\n#include \"g.glsl\"\nG\n"),
            ("g.glsl", "#ifndef G_GLSL\n#define G_GLSL\nint g;\n#endif\n"),
        ],
        "main.fsh",
        &PreprocessOptions::default(),
    );
    assert!(out.diagnostics.is_empty());
    assert_eq!(norm(&out.code), ["int g;", "G"]);
    assert_eq!(out.files, ["main.fsh", "g.glsl"]);
    assert_eq!(out.code.lines().count(), 1 + 4 + 4);
}

#[test]
fn include_depth_limit() {
    let mut files: Vec<(String, String)> = (0..10)
        .map(|i| {
            (
                format!("f{i}.glsl"),
                format!("#include \"f{}.glsl\"\nL{i}\n", i + 1),
            )
        })
        .collect();
    files.push(("f10.glsl".into(), "END\n".into()));
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let opts = PreprocessOptions {
        max_include_depth: 4,
        ..Default::default()
    };
    let out = run_files(&refs, "f0.glsl", &opts);
    assert_eq!(codes(&out), ["pp.include-depth"]);
    assert_eq!(norm(&out.code), ["L4", "L3", "L2", "L1", "L0"]);
    let out = run_files(&refs, "f0.glsl", &PreprocessOptions::default());
    assert!(out.diagnostics.is_empty());
    assert_eq!(norm(&out.code)[0], "END");
}

#[test]
fn include_forms() {
    let files = [
        (
            "shaders.fsh",
            "#include <lib/x.glsl>\n  #include \"lib/x.glsl\" // trailing comment\n#include lib/x.glsl\n",
        ),
        ("lib/x.glsl", "x\n"),
    ];
    let out = run_files(&files, "shaders.fsh", &PreprocessOptions::default());
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(norm(&out.code), ["x", "x", "x"]);
    // `..` above the root is clamped like Iris (warning); a bare `#include` is malformed.
    let out = run_files(
        &[("world0/main.fsh", "#include \"../../lib/x.glsl\"\n#include\n"), ("lib/x.glsl", "x\n")],
        "world0/main.fsh",
        &PreprocessOptions::default(),
    );
    assert_eq!(codes(&out), ["pp.include-clamped", "pp.include-malformed"]);
    assert_eq!(norm(&out.code), ["x"]);
    assert_eq!(out.files, ["world0/main.fsh", "lib/x.glsl"]);
    // Every line starting with `#include` is an include line for Iris.
    let out = run("#include_next \"x.glsl\"\nok\n");
    assert_eq!(codes(&out), ["pp.include-malformed"]);
    assert_eq!(norm(&out.code), ["ok"]);
    // `# include` is not resolved by Iris; we warn and drop it.
    let out = run("# include \"x.glsl\"\nx\n");
    assert_eq!(codes(&out), ["pp.include-unsupported"]);
    assert_eq!(norm(&out.code), ["x"]);
}

#[test]
fn include_closure_lists_reachable_files() {
    let m = MemorySources::new()
        .with(
            "world0/gbuffers_terrain.fsh",
            "#if 0\n#include \"/lib/a.glsl\"\n#endif\n#include \"/lib/b.glsl\"\n",
        )
        .with(
            "lib/a.glsl",
            "#include \"c.glsl\"\n#include \"missing.glsl\"\n",
        )
        .with("lib/b.glsl", "#include \"c.glsl\"\n#include \"a.glsl\"\n")
        .with("lib/c.glsl", "#include \"b.glsl\"\n");
    let mut pp = Preprocessor::new(&m);
    let (files, diags) = pp.include_closure("world0/gbuffers_terrain.fsh");
    assert_eq!(
        files,
        [
            "world0/gbuffers_terrain.fsh",
            "lib/a.glsl",
            "lib/c.glsl",
            "lib/b.glsl"
        ]
    );
    let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes,
        ["pp.include-cycle", "pp.include-cycle", "pp.include-missing"]
    );
    let (files, diags) = pp.include_closure("nope.fsh");
    assert!(files.is_empty());
    assert!(diags.has_errors());
}

#[test]
fn missing_entry_and_in_memory_source() {
    let m = MemorySources::new().with("lib/u.glsl", "uniform float u;\n");
    let mut pp = Preprocessor::new(&m);
    let out = pp.preprocess("composite.fsh", &PreprocessOptions::default());
    assert_eq!(
        out.diagnostics.iter().next().map(|d| d.code.as_str()),
        Some("pp.file-missing")
    );
    assert!(out.code.is_empty() && out.line_map.is_empty());
    let out = pp.preprocess_source(
        "world0/dh_terrain.fsh",
        "#version 400\n#include \"/lib/u.glsl\"\nU\n",
        &PreprocessOptions::default().with_define("U", "u"),
    );
    check_invariants(&out);
    assert!(out.diagnostics.is_empty());
    assert_eq!(norm(&out.code), ["uniform float u;", "u"]);
    assert_eq!(out.line_map[2].file, "world0/dh_terrain.fsh");
    // The in-memory file is not cached under its path.
    let out = pp.preprocess("world0/dh_terrain.fsh", &PreprocessOptions::default());
    assert!(out.diagnostics.has_errors());
}

#[test]
fn cache_is_reused_across_entries() {
    let m = MemorySources::new()
        .with("lib/s.glsl", "#ifdef A\nint a;\n#else\nint b;\n#endif\n")
        .with("one.fsh", "#include \"lib/s.glsl\"\n")
        .with("two.fsh", "#define A\n#include \"lib/s.glsl\"\n");
    let mut pp = Preprocessor::new(&m);
    let one = pp.preprocess("one.fsh", &PreprocessOptions::default());
    let two = pp.preprocess("two.fsh", &PreprocessOptions::default());
    assert_eq!(norm(&one.code), ["int b;"]);
    assert_eq!(norm(&two.code), ["int a;"]);
    // Macros do not leak between runs.
    let one_again = pp.preprocess("one.fsh", &PreprocessOptions::default());
    assert_eq!(one_again.code, one.code);
}

// ----------------------------------------------------------------------
// Conditionals
// ----------------------------------------------------------------------

#[test]
fn ifdef_ifndef_else() {
    assert_eq!(
        clean("#define A\n#ifdef A\na\n#else\nb\n#endif\n#ifndef A\nc\n#else\nd\n#endif\n"),
        ["a", "d"]
    );
    assert_eq!(clean("#ifdef B\na\n#endif\n#ifndef B\nb\n#endif\n"), ["b"]);
}

#[test]
fn if_elif_chains() {
    let src = "#if X == 1\none\n#elif X == 2\ntwo\n#elif X == 3\nthree\n#else\nother\n#endif\n";
    for (x, want) in [("1", "one"), ("2", "two"), ("3", "three"), ("9", "other")] {
        let out = run_opts(src, &PreprocessOptions::default().with_define("X", x));
        assert_eq!(norm(&out.code), [want]);
    }
    // First true branch wins even if later ones are also true.
    assert_eq!(clean("#if 1\na\n#elif 1\nb\n#else\nc\n#endif\n"), ["a"]);
}

#[test]
fn defined_operator_forms() {
    let src =
        "#define A\n#if defined A && defined(A) && !defined B && !defined ( B )\nyes\n#endif\n";
    assert_eq!(clean(src), ["yes"]);
    // `defined` produced by macro expansion (GCC/JCPP behaviour).
    assert_eq!(
        clean("#define HAS(x) defined(x)\n#if !HAS(G)\nok\n#endif\n"),
        ["ok"]
    );
    // As in GCC/JCPP the argument is pre-expanded, so a defined (empty) macro breaks HAS().
    assert_eq!(
        codes(&run(
            "#define HAS(x) defined(x)\n#define F\n#if HAS(F)\n#endif\n"
        )),
        ["pp.bad-expression"]
    );
    assert_eq!(
        clean("#define ISDEF defined(F)\n#define F\n#if ISDEF\nok\n#endif\n"),
        ["ok"]
    );
    // The operand of `defined` is not macro-expanded.
    assert_eq!(
        clean("#define A B\n#if defined(A) && !defined(B)\nok\n#endif\n"),
        ["ok"]
    );
    let out = run("#if defined\nx\n#endif\n");
    assert_eq!(codes(&out), ["pp.bad-expression"]);
    let out = run("#if defined(A\nx\n#endif\n");
    assert_eq!(codes(&out), ["pp.bad-expression"]);
}

#[test]
fn if_expressions() {
    assert_eq!(
        clean("#if (2 + 3) * 4 == 20 && (7 % 4) == 3 && (1 << 3) == 8\na\n#endif\n"),
        ["a"]
    );
    assert_eq!(clean("#if UNKNOWN == 0 && !UNKNOWN\na\n#endif\n"), ["a"]);
    assert_eq!(
        clean(
            "#define V 3\n#if V > 2 ? 1 : 0\na\n#endif\n#if (V < 2 ? 10 : 20) == 20\nb\n#endif\n"
        ),
        ["a", "b"]
    );
    assert_eq!(
        clean("#if 0x10 == 16 && 010 == 8 && 'a' == 97\na\n#endif\n"),
        ["a"]
    );
    assert_eq!(
        clean(
            "#define MC_VERSION 260300\n#if MC_VERSION >= 11700 && MC_VERSION < 300000\nnew\n#endif\n"
        ),
        ["new"]
    );
    assert_eq!(
        clean("#define F(x) ((x) * 2)\n#if F(3) == 6\na\n#endif\n"),
        ["a"]
    );
    // `true` is not special (C/JCPP semantics).
    assert_eq!(clean("#define T true\n#if T\na\n#else\nb\n#endif\n"), ["b"]);
}

#[test]
fn if_expression_errors() {
    let out = run("#if\na\n#endif\nb\n");
    assert_eq!(codes(&out), ["pp.bad-expression"]);
    assert_eq!(norm(&out.code), ["b"]);
    let out = run("#if 1 +\na\n#endif\n");
    assert_eq!(codes(&out), ["pp.bad-expression"]);
    let out = run("#if 1 / 0\na\n#endif\n");
    assert_eq!(codes(&out), ["pp.bad-expression"]);
    assert!(norm(&out.code).is_empty());
    let out = run("#if 0 && 1 / 0\na\n#endif\n");
    assert!(out.diagnostics.is_empty());
    let out = run("#if 1 2\na\n#endif\n");
    assert_eq!(codes(&out), ["pp.extra-tokens"]);
    assert_eq!(norm(&out.code), ["a"]);
}

#[test]
fn float_conditions_truncate_like_iris() {
    let out = run_opts(
        "#if SHADOW_STRENGTH > 0.5\nstrong\n#else\nweak\n#endif\n",
        &PreprocessOptions::default().with_define("SHADOW_STRENGTH", "0.75"),
    );
    // 0.75 -> 0 and 0.5 -> 0, so `0 > 0` is false (as on Iris).
    assert_eq!(norm(&out.code), ["weak"]);
    assert_eq!(codes(&out), ["pp.float-in-condition"]);
}

#[test]
fn nested_conditionals_in_inactive_code() {
    let src = "#if 0\n#if garbage (((\n#elif 1/0\n#else\n#error nope\n#endif\n#ifdef\n#endif\n#elif 1\nx\n#else\ny\n#endif\nz\n";
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(norm(&out.code), ["x", "z"]);
    // Unknown directives in inactive code are ignored silently.
    assert!(run("#if 0\n#frobnicate\n#endif\n").diagnostics.is_empty());
    // Inactive #elif is not evaluated once a branch was taken.
    assert!(run("#if 1\n#elif (((\n#endif\n").diagnostics.is_empty());
}

#[test]
fn conditional_structure_errors() {
    assert_eq!(codes(&run("#endif\n")), ["pp.unmatched-conditional"]);
    assert_eq!(codes(&run("#else\n")), ["pp.unmatched-conditional"]);
    assert_eq!(codes(&run("#elif 1\n")), ["pp.unmatched-conditional"]);
    let out = run("#if 1\na\n#else\nb\n#else\nc\n#endif\n");
    assert_eq!(codes(&out), ["pp.unmatched-conditional"]);
    assert_eq!(norm(&out.code), ["a"]);
    // `#elif` after `#else` is reported and ignored (state unchanged, as in JCPP).
    let out = run("#if 0\n#else\na\n#elif 1\nb\n#endif\n");
    assert_eq!(codes(&out), ["pp.unmatched-conditional"]);
    assert_eq!(norm(&out.code), ["a", "b"]);
    // A stray #endif is only a warning: JCPP ignores it and Iris loads the pack.
    let out = run("a\n#endif\nb\n");
    assert_eq!(codes(&out), ["pp.unmatched-conditional"]);
    assert!(!out.diagnostics.has_errors());
    assert_eq!(norm(&out.code), ["a", "b"]);
    let out = run("#ifdef X\na\n");
    assert_eq!(codes(&out), ["pp.unterminated-conditional"]);
    assert_eq!(
        out.diagnostics
            .iter()
            .next()
            .and_then(|d| d.location.clone()),
        Some(SourceLocation::new("main.fsh", 1))
    );
    // Invalid #ifdef/#ifndef operands are errors; the group counts as true (JCPP).
    let out = run("#ifdef 3\nx\n#endif\n");
    assert_eq!(codes(&out), ["pp.bad-directive"]);
    assert_eq!(norm(&out.code), ["x"]);
    let out = run("#ifndef\ny\n#endif\n");
    assert_eq!(codes(&out), ["pp.bad-directive"]);
    assert_eq!(norm(&out.code), ["y"]);
    assert_eq!(codes(&run("#ifdef A B\n#endif\n")), ["pp.extra-tokens"]);
    // `#endif FOO` / `#else // x` are accepted silently.
    assert!(
        run("#if 1\n#else junk\n#endif junk\n")
            .diagnostics
            .is_empty()
    );
}

// ----------------------------------------------------------------------
// Macros
// ----------------------------------------------------------------------

#[test]
fn object_and_function_like_macros() {
    assert_eq!(
        clean("#define PI 3.14\nfloat x = PI;\n"),
        ["float x = 3.14;"]
    );
    assert_eq!(
        clean("#define SQ(x) ((x)*(x))\nSQ(a+1)\n"),
        ["((a+1)*(a+1))"]
    );
    assert_eq!(
        clean("#define F(a, b) a + b\nF((1, 2), (3, 4))\n"),
        ["(1, 2) + (3, 4)"]
    );
    // Brackets and braces do not protect commas (C semantics).
    assert_eq!(
        codes(&run("#define F(a, b) a + b\nF([1, 2], 3)\n")),
        ["pp.macro-args"]
    );
    assert_eq!(clean("#define F(x) <x>\nF (1) F\t( 2 )\n"), ["<1> <2>"]);
    // Function-like macro name without arguments is left alone.
    assert_eq!(clean("#define F(x) x\nint F; F + 1\n"), ["int F; F + 1"]);
    // Object-like macro whose body starts with a parenthesis.
    assert_eq!(clean("#define O (x)\nO(1)\n"), ["(x)(1)"]);
    assert_eq!(clean("#define E()  empty\nE() E( )\n"), ["empty empty"]);
    assert_eq!(clean("#define ONE(x) [x]\nONE()\n"), ["[]"]);
    assert_eq!(clean("#define EMPTY\na EMPTY b\n"), ["a b"]);
}

#[test]
fn undef_and_redefinition() {
    assert_eq!(clean("#define A 1\n#undef A\nA\n#undef NEVER\n"), ["A"]);
    let out = run("#define A 1\n#define A 1\n#define A  1 \n");
    assert!(
        out.diagnostics.is_empty(),
        "identical redefinitions are fine: {:?}",
        out.diagnostics
    );
    let out = run("#define A 1\n#define A 2\nA\n");
    assert_eq!(codes(&out), ["pp.macro-redefined"]);
    assert_eq!(norm(&out.code), ["2"]);
    assert_eq!(codes(&run("#undef\n")), ["pp.bad-directive"]);
    assert_eq!(
        codes(&run(
            "#define\n#define 1x\n#define defined\n#define F(a,a)\n"
        )),
        ["pp.bad-define"; 4]
    );
}

#[test]
fn recursion_is_blocked_by_hide_sets() {
    assert_eq!(clean("#define foo foo\nfoo\n"), ["foo"]);
    assert_eq!(clean("#define a b\n#define b a\na b\n"), ["a b"]);
    assert_eq!(clean("#define f(x) f(x + 1)\nf(0)\n"), ["f(0 + 1)"]);
    assert_eq!(clean("#define a a b\n#define b a\na\n"), ["a a"]);
    // Classic: f(f)(1)
    assert_eq!(nows(&run("#define f(x) x\nf(f)(1)\n").code), ["f(1)"]);
    assert_eq!(
        nows(&run("#define f(a) a*g\n#define g(a) f(a)\nf(2)(9)\n").code),
        ["2*9*g"]
    );
}

#[test]
fn stringify() {
    assert_eq!(clean("#define S(x) #x\nS(a  +   b)\n"), ["\"a + b\""]);
    assert_eq!(
        run("#define S(x) #x\nS(\"q\\n\" 'c')\n").code.trim(),
        r#""\"q\\n\" 'c'""#
    );
    assert_eq!(clean("#define S(x) # x\nS()\n"), ["\"\""]);
    assert_eq!(clean("#define S(x) #x\n#define X 1\nS(X)\n"), ["\"X\""]);
    assert_eq!(
        clean("#define S(x) #x\n#define XS(x) S(x)\n#define X 1\nXS(X)\n"),
        ["\"1\""]
    );
    // `#` in an object-like macro is an ordinary token, but a `#` reaching
    // program text is an error (JCPP throws "Bad token", so Iris rejects it).
    let out = run("#define H # x\nH\n");
    assert_eq!(norm(&out.code), ["# x"]);
    assert_eq!(codes(&out), ["pp.stray-hash"]);
}

#[test]
fn token_pasting() {
    assert_eq!(
        clean("#define CAT(a, b) a ## b\nCAT(x, 1) CAT(x,) CAT(,y) [CAT(,)]\n"),
        ["x1 x y []"]
    );
    assert_eq!(
        clean("#define CAT(a,b) a##b\n#define xy 42\nCAT(x, y)\n"),
        ["42"]
    );
    // Operands of ## are not expanded; an extra level forces expansion.
    assert_eq!(
        clean(
            "#define CAT(a,b) a##b\n#define XCAT(a,b) CAT(a,b)\n#define A x\n#define B y\nCAT(A,B) XCAT(A,B)\n"
        ),
        ["AB xy"]
    );
    assert_eq!(clean("#define OP(a,b) a ## = ## b\nOP(+,)\n"), ["+="]);
    assert_eq!(clean("#define V(n) vec ## n\nV(3) v;\n"), ["vec3 v;"]);
    assert_eq!(clean("#define AB(a) a ## a ## a\nAB(1)\n"), ["111"]);
    // Object-like macro with ##.
    assert_eq!(clean("#define XY x ## y\nXY\n"), ["xy"]);
    let out = run("#define CAT(a,b) a##b\nCAT(+,-)\n");
    assert_eq!(codes(&out), ["pp.invalid-paste"]);
    assert_eq!(nows(&out.code), ["+-"]);
}

#[test]
fn variadic_macros() {
    assert_eq!(
        clean("#define V(...) f(__VA_ARGS__)\nV(1, 2, 3) V()\n"),
        ["f(1, 2, 3) f()"]
    );
    assert_eq!(
        clean("#define V(a, ...) g(a: __VA_ARGS__)\nV(1) V(1, 2, (3, 4))\n"),
        ["g(1: ) g(1: 2, (3, 4))"]
    );
    assert_eq!(clean("#define N(args...) h(args)\nN(x, y)\n"), ["h(x, y)"]);
    assert_eq!(
        clean("#define G(fmt, ...) p(fmt, ## __VA_ARGS__)\nG(a) G(a, b)\n"),
        ["p(a) p(a,b)"]
    );
    assert_eq!(
        clean("#define S(...) #__VA_ARGS__\nS(a,  b ,c)\n"),
        ["\"a, b ,c\""]
    );
}

#[test]
fn argument_count_errors() {
    let out = run("#define F(a, b) a b\nF(1)\nnext\n");
    assert_eq!(codes(&out), ["pp.macro-args"]);
    assert_eq!(nows(&out.code), ["F(1)", "next"]);
    let out = run("#define F() x\nF(1)\n");
    assert_eq!(codes(&out), ["pp.macro-args"]);
    let out = run("#define V(a, b, ...) x\nV(1)\n");
    assert_eq!(codes(&out), ["pp.macro-args"]);
    let out = run("#define F(x) x\nF(1, 2)\n");
    assert_eq!(codes(&out), ["pp.macro-args"]);
}

#[test]
fn c99_example_3() {
    let src = r#"#define x 3
#define f(a) f(x * (a))
#undef x
#define x 2
#define g f
#define z z[0]
#define h g(~
#define m(a) a(w)
#define w 0,1
#define t(a) a
#define p() int
#define q(x) x
#define r(x,y) x ## y
#define str(x) # x
f(y+1) + f(f(z)) % t(t(g)(0) + t)(1);
g(x+(3,4)-w) | h 5) & m
(f)^m(m);
p() i[q()] = { q(1), r(2,3), r(4,), r(,5), r(,) };
char c[2][6] = { str(hello), str() };
"#;
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(
        nows(&out.code),
        [
            "f(2*(y+1))+f(2*(f(2*(z[0]))))%f(2*(0))+t(1);",
            "f(2*(2+(3,4)-0,1))|f(2*(~5))&f(2*(0,1))^m(0,1);",
            "inti[]={1,23,4,5,};",
            "charc[2][6]={\"hello\",\"\"};",
        ]
    );
    // The invocation of `m` that spans two lines is emitted on its first line;
    // the second line is blank and the line map stays aligned.
    let lines: Vec<&str> = out.code.lines().collect();
    assert_eq!(lines.len(), 19);
    assert!(
        lines[15].contains("m(0,1);")
            || lines[15].contains("m(0, 1);")
            || lines[15].replace(' ', "").contains("m(0,1);")
    );
    assert_eq!(lines[16], "");
    assert_eq!(out.line_map[16].line, 17);
    assert_eq!(out.line_map[17].line, 18);
}

#[test]
fn c99_example_4() {
    let src = "#define str(s) # s\n#define xstr(s) str(s)\n#define debug(s, t) printf(\"x\" # s \"= %d, x\" # t \"= %s\", \\\n x ## s, x ## t)\n#define INCFILE(n) vers ## n\n#define glue(a, b) a ## b\n#define xglue(a, b) glue(a, b)\n#define HIGHLOW \"hello\"\n#define LOW LOW \", world\"\ndebug(1, 2);\nfputs(str(strncmp(\"abc\\0d\", \"abc\", '\\4') // this goes away\n == 0) str(: @\\n), s);\nxstr(INCFILE(2).h)\nglue(HIGH, LOW);\nxglue(HIGH, LOW)\n";
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(
        norm(&out.code),
        [
            r#"printf("x" "1" "= %d, x" "2" "= %s", x1, x2);"#,
            r#"fputs("strncmp(\"abc\\0d\", \"abc\", '\\4') == 0" ": @\n", s);"#,
            r#""vers2.h""#,
            r#""hello";"#,
            r#""hello" ", world""#,
        ]
    );
}

#[test]
fn c99_example_5_placemarkers() {
    let src = "#define t(x,y,z) x ## y ## z\nint j[] = { t(1,2,3), t(,4,5), t(6,,7), t(8,9,),\n t(10,,), t(,11,), t(,,12), t(,,) };\n";
    assert_eq!(
        nows(&run(src).code),
        ["intj[]={123,45,67,89,", "10,11,12,};"]
    );
}

#[test]
fn c99_example_7_variadic() {
    let src = "#define debug(...) fprintf(stderr, __VA_ARGS__)\n#define showlist(...) puts(#__VA_ARGS__)\n#define report(test, ...) ((test)?puts(#test): printf(__VA_ARGS__))\ndebug(\"Flag\");\ndebug(\"X = %d\\n\", x);\nshowlist(The first, second, and third items.);\nreport(x>y, \"x is %d but y is %d\", x, y);\n";
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(
        nows(&out.code),
        [
            r#"fprintf(stderr,"Flag");"#,
            r#"fprintf(stderr,"X=%d\n",x);"#,
            r#"puts("Thefirst,second,andthirditems.");"#,
            r#"((x>y)?puts("x>y"):printf("xis%dbutyis%d",x,y));"#,
        ]
    );
}

#[test]
fn multiline_invocations_keep_line_map_aligned() {
    let src = "#define F(a, b) (a + b)\nint x = F(1,\n          2) + 3;\nint y;\n";
    let out = run(src);
    assert!(out.diagnostics.is_empty());
    assert_eq!(out.code, "\nint x = (1 + 2) + 3;\n\nint y;\n");
    assert_eq!(
        out.line_map.iter().map(|l| l.line).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    // Name at the end of a line, '(' on the next one.
    let out = run("#define F(a) [a]\nF\n(1) tail\nnext\n");
    assert_eq!(out.code, "\n[1] tail\n\nnext\n");
    // Name at the end of a line followed by something else: no invocation and
    // the line structure (and a trailing // comment) is preserved.
    let out = run("#define F(a) [a]\nint F; // comment\nint z;\n");
    assert_eq!(out.code, "\nint F; // comment\nint z;\n");
    // Peeking stops at directives.
    let out = run("#define F(a) [a]\nF\n#define G\n(1)\n");
    assert_eq!(out.code, "\nF\n\n(1)\n");
    // Comments inside arguments are dropped.
    let out = run("#define F(a, b) a b\nF(x, // c1\n  /* c2\n */ y) z\n");
    assert_eq!(out.code, "\nx y z\n\n\n");
}

#[test]
fn directives_inside_macro_arguments() {
    let out = run("#define F(a) [a]\nF(1\n#ifdef NOPE\n+ 2\n#else\n+ 3\n#endif\n)\nend\n");
    assert_eq!(codes(&out), ["pp.directive-in-macro-args"; 3]);
    assert_eq!(nows(&out.code), ["[1+3]", "end"]);
    assert_eq!(out.code.lines().count(), 9);
}

#[test]
fn unterminated_invocation() {
    let out = run("#define F(a) a\nF(1, 2\nmore\n");
    assert_eq!(codes(&out), ["pp.unterminated-macro-call"]);
    // Nothing is silently lost.
    assert_eq!(nows(&out.code), ["F(1,2", "more"]);
}

#[test]
fn builtin_macros() {
    let out = run(
        "#version 330 compatibility\nint a = __LINE__;\nint b = __FILE__;\nint c = __VERSION__;\n#if __VERSION__ == 330 && defined GL_compatibility_profile && !defined GL_core_profile\nok\n#endif\n",
    );
    assert_eq!(
        norm(&out.code),
        ["int a = 2;", "int b = 0;", "int c = 330;", "ok"]
    );
    let out = run("int v = __VERSION__;\n#ifdef GL_core_profile\ncore\n#endif\n");
    assert_eq!(norm(&out.code), ["int v = 110;"]);
    let out = run("#version 450\n#ifdef GL_core_profile\ncore\n#endif\n");
    assert_eq!(norm(&out.code), ["core"]);
    let out = run("#version 300 es\n#if defined GL_ES && defined GL_es_profile\nes\n#endif\n");
    assert_eq!(norm(&out.code), ["es"]);
    assert_eq!(clean("#define L __LINE__\n\nL\n"), ["3"]);
}

#[test]
fn environment_defines() {
    let opts = PreprocessOptions::default()
        .with_define("MC_VERSION", "260300")
        .with_flag("IS_IRIS")
        .with_define("EXPR", "(1 + 2)");
    let out = run_opts(
        "#ifdef IS_IRIS\niris IS_IRIS end\n#endif\nint v = MC_VERSION * EXPR;\n",
        &opts,
    );
    assert!(out.diagnostics.is_empty());
    assert_eq!(norm(&out.code), ["iris end", "int v = 260300 * (1 + 2);"]);
    // Empty-valued define in #if is an error (JCPP: bad token), like on Iris.
    let out = run_opts("#if IS_IRIS\nx\n#endif\n", &opts);
    assert_eq!(codes(&out), ["pp.bad-expression"]);
    let out = run_opts(
        "x\n",
        &PreprocessOptions::default()
            .with_define("1bad", "1")
            .with_define("A", "line\nbreak"),
    );
    assert_eq!(codes(&out), ["pp.bad-define"]);
    let out = run_opts(
        "A\n",
        &PreprocessOptions::default().with_define("A", "line\nbreak"),
    );
    assert_eq!(out.code, "line break\n");
}

#[test]
fn avoid_accidental_token_pasting() {
    assert_eq!(run("#define NEG -1\nx-NEG\n").code, "\nx- -1\n");
    assert_eq!(run("#define P +\na+P+b\n").code, "\na+ + +b\n");
    assert_eq!(
        run("#define E\nx/E/y\n").code,
        "\nx//y\n".replace("//", "/ /")
    );
    assert_eq!(run("#define ID(x) x\nID(a)ID(b)\n").code, "\na b\n");
    assert_eq!(run("#define ONE 1\nONE.5\n").code, "\n1 .5\n");
    // Source text that is already adjacent is untouched.
    assert_eq!(run("a+-b\n").code, "a+-b\n");
}

// ----------------------------------------------------------------------
// Other directives
// ----------------------------------------------------------------------

#[test]
fn error_and_warning_directives() {
    let out = run("#error \"This program should be disabled\" // c\n#warning careful\n");
    let d: Vec<(Severity, &str, &str)> = out
        .diagnostics
        .iter()
        .map(|d| (d.severity, d.code.as_str(), d.message.as_str()))
        .collect();
    assert_eq!(
        d,
        [
            (
                Severity::Error,
                "pp.error",
                "\"This program should be disabled\""
            ),
            (Severity::Warning, "pp.warning", "careful")
        ]
    );
    assert!(
        run("#ifdef NOPE\n#error inactive\n#endif\n")
            .diagnostics
            .is_empty()
    );
    assert_eq!(
        run("#error\n")
            .diagnostics
            .iter()
            .next()
            .map(|d| d.message.clone()),
        Some("#error".into())
    );
}

#[test]
fn version_extension_pragma_are_hoisted() {
    let src = "// header\n#version 330 compatibility\n#extension GL_ARB_gpu_shader5 : enable\n#pragma optimize(on)\n#ifdef NOPE\n#extension GL_EXT_nope : require\n#version 460\n#endif\n#ifdef GL_ARB_gpu_shader5\nfive\n#endif\nvoid main() {}\n";
    let out = run(src);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(
        out.version,
        Some(VersionDirective {
            number: 330,
            profile: Some(Profile::Compatibility)
        })
    );
    assert_eq!(
        out.extensions,
        [ExtensionDirective {
            name: "GL_ARB_gpu_shader5".into(),
            behavior: "enable".into()
        }]
    );
    assert_eq!(out.pragmas, ["optimize(on)"]);
    assert_eq!(norm(&out.code), ["// header", "five", "void main() {}"]);
    assert_eq!(
        out.header(),
        "#version 330 compatibility\n#extension GL_ARB_gpu_shader5 : enable\n"
    );
    assert!(
        out.to_glsl()
            .starts_with("#version 330 compatibility\n#extension")
    );
    assert!(
        !out.code.contains("#version")
            && !out.code.contains("#extension")
            && !out.code.contains("#pragma")
    );
}

#[test]
fn multiple_versions() {
    let out = run("#version 120\n#version 120\n");
    assert!(out.diagnostics.is_empty());
    let out = run("#version 120\n#version 330 core\n");
    assert_eq!(codes(&out), ["pp.version-mismatch"]);
    assert_eq!(out.version_number(), 120);
    assert_eq!(codes(&run("#version\n")), ["pp.bad-version"]);
    assert_eq!(codes(&run("#version abc\n")), ["pp.bad-version"]);
    let out = run("#version 330 banana\n");
    assert_eq!(codes(&out), ["pp.bad-version"]);
    assert_eq!(
        out.version,
        Some(VersionDirective {
            number: 330,
            profile: None
        })
    );
    assert_eq!(run("#version 100\n").version_number(), 100);
}

#[test]
fn extension_edge_cases() {
    let out = run(
        "#extension GL_A : disable\n#extension all : warn\n#extension bad\n#extension GL_B : sometimes\n#if defined GL_A || defined all\nx\n#endif\n",
    );
    assert_eq!(codes(&out), ["pp.bad-extension", "pp.bad-extension"]);
    assert_eq!(out.extensions.len(), 2);
    assert!(norm(&out.code).is_empty());
}

#[test]
fn line_directive_adjusts_line_map() {
    let out = run("a\n#line 100\nb\nc\n#define X 7\n#line X\nd\n#line\n");
    assert_eq!(
        out.line_map.iter().map(|l| l.line).collect::<Vec<_>>(),
        [1, 2, 100, 101, 102, 103, 7, 8]
    );
    assert_eq!(codes(&out), ["pp.bad-line"]);
    let out = run("#line 10 2\nint l = __LINE__;\n");
    assert_eq!(norm(&out.code), ["int l = 10;"]);
}

#[test]
fn unknown_and_null_directives() {
    let out = run("#\n#   \n#frobnicate stuff\n# 42 \"x\"\nok\n");
    assert_eq!(
        codes(&out),
        ["pp.unknown-directive", "pp.unknown-directive"]
    );
    assert_eq!(norm(&out.code), ["ok"]);
    // C23 #elifdef is unknown to JCPP as well.
    assert_eq!(
        codes(&run("#if 1\n#elifdef X\n#endif\n")),
        ["pp.unknown-directive"]
    );
}

#[test]
fn directive_hash_rules() {
    // A '#' that is not first on the line is not a directive.
    assert_eq!(run("x # define Y\n").code, "x # define Y\n");
    // Whitespace and comments before '#' are fine; whitespace after '#' too.
    assert_eq!(clean("  /* c */ #  define Y 1\nY\n"), ["1"]);
}

// ----------------------------------------------------------------------
// Comments, splices, line structure
// ----------------------------------------------------------------------

#[test]
fn comments_are_kept_in_active_code_only() {
    let src = "/* DRAWBUFFERS:01 */\n// c1\n#if 0\n/* dead */\n// dead\n#endif\nx; /* multi\nline */ y;\n";
    let out = run(src);
    assert_eq!(
        out.code,
        "/* DRAWBUFFERS:01 */\n// c1\n\n\n\n\nx; /* multi\nline */ y;\n"
    );
    let out = run_opts(
        src,
        &PreprocessOptions {
            keep_comments: false,
            ..Default::default()
        },
    );
    assert_eq!(norm(&out.code), ["x;", "y;"]);
    assert_eq!(out.code.lines().count(), 8);
}

#[test]
fn comments_hide_directives_and_continue_directives() {
    // `#endif` inside a comment is not a directive.
    let out = run("#if 0\n/*\n#endif\n*/\n#endif\nafter\n");
    assert!(out.diagnostics.is_empty());
    assert_eq!(norm(&out.code), ["after"]);
    // A block comment opened on a directive line continues the directive.
    assert_eq!(clean("#define X 1 /* start\nend */ + 2\nX\n"), ["1 + 2"]);
    // A directive right after the end of a multi-line comment opened in active code.
    // Per C (and JCPP) a '#' after a multi-line comment is at the start of a
    // line only if nothing but whitespace/comments precedes it on the logical line.
    let out = run("/* open\nclose */ #define R 6\nR\n");
    assert_eq!(out.code, "/* open\nclose */\n6\n");
    let out = run("int a; /* open\nclose */ #define Q 5\nQ\n");
    assert_eq!(out.code, "int a; /* open\nclose */ #define Q 5\nQ\n");
}

#[test]
fn unterminated_comment_is_closed() {
    let out = run("int a;\n/* never closed\nstill\n");
    assert_eq!(codes(&out), ["pp.unterminated-comment"]);
    assert!(out.code.ends_with("still */\n"));
    check_invariants(&out);
}

#[test]
fn line_splicing() {
    let src = "#define LONG(a) \\\n  a + \\\n  a\nLONG(1)\nint x = 1 + \\\n2;\nend\n";
    let out = run(src);
    assert!(out.diagnostics.is_empty());
    assert_eq!(out.code, "\n\n\n1 + 1\nint x = 1 + 2;\n\nend\n");
    assert_eq!(
        out.line_map.iter().map(|l| l.line).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6, 7]
    );
    // Splice inside an identifier.
    assert_eq!(clean("#define FOOBAR 1\nFOO\\\nBAR\n"), ["1"]);
    // A `//` comment ending in a backslash swallows the next line (C/JCPP semantics).
    assert_eq!(
        clean("// comment \\\nint hidden;\nint shown;\n"),
        ["// comment int hidden;", "int shown;"]
    );
    // Backslash followed by spaces is not a splice (JCPP).
    assert_eq!(
        clean("// comment \\ \nint shown;\n"),
        ["// comment \\", "int shown;"]
    );
}

#[test]
fn robustness_inputs() {
    // BOM, CRLF and NUL characters.
    let out = run("\u{FEFF}#version 120\r\nint\0 a;\r\n#define X 1\rX\r\n");
    assert!(out.diagnostics.is_empty());
    assert_eq!(out.version_number(), 120);
    assert_eq!(out.code, "\nint a;\n\n1\n");
    // Empty file.
    let out = run("");
    assert!(out.code.is_empty());
    // No trailing newline.
    assert_eq!(run("a\nb").code, "a\nb\n");
    // Unterminated string/char literal.
    assert_eq!(run("don't \"stop\nnext\n").code, "don't \"stop\nnext\n");
    // Stray characters.
    assert_eq!(run("@ ` \\ $\n").code, "@ ` \\ $\n");
}

#[test]
fn exponential_macros_are_capped() {
    let mut src = String::from("#define A0 x\n");
    for i in 1..40 {
        src.push_str(&format!("#define A{i} A{p} A{p}\n", p = i - 1));
    }
    src.push_str("A39\n");
    let out = run(&src);
    assert!(codes(&out).contains(&"pp.expansion-limit"));
    let mut src = String::from("#define F0(x) x x\n");
    for i in 1..40 {
        src.push_str(&format!("#define F{i}(x) F{p}(F{p}(x))\n", p = i - 1));
    }
    src.push_str("F39(y)\n");
    let out = run(&src);
    assert!(codes(&out).contains(&"pp.expansion-limit"));
}

#[test]
fn deep_nesting_does_not_overflow() {
    let src = format!(
        "#define F(x) x\n{}1{}\n",
        "F(".repeat(5000),
        ")".repeat(5000)
    );
    let out = run(&src);
    assert!(out.diagnostics.has_errors());
    let src = format!(
        "#if {}1{}\n#endif\n",
        "(".repeat(100_000),
        ")".repeat(100_000)
    );
    let out = run(&src);
    assert_eq!(codes(&out), ["pp.bad-expression"]);
    let src = format!("{}x\n", "#if 1\n".repeat(10_000)) + &"#endif\n".repeat(10_000);
    let out = run(&src);
    assert!(out.diagnostics.is_empty());
    assert_eq!(norm(&out.code), ["x"]);
}

// ----------------------------------------------------------------------
// Reserved words
// ----------------------------------------------------------------------

#[test]
fn reserved_words_are_escaped() {
    let src = "#version 120\nuniform sampler2D input; // input in comment\nvec3 sample = vec3(filter);\nstruct S { float output; };\nfloat f(S s) { return s.output; }\n";
    let out = run(src);
    assert_eq!(
        norm(&out.code),
        [
            "uniform sampler2D sb_kw_input; // input in comment",
            "vec3 sb_kw_sample = vec3(sb_kw_filter);",
            "struct S { float sb_kw_output; };",
            "float f(S s) { return s.sb_kw_output; }",
        ]
    );
}

#[test]
fn reserved_words_respect_versions_extensions_and_layout() {
    let out = run("#version 400 core\nsample in vec4 c;\nfloat shared;\n");
    assert_eq!(
        norm(&out.code),
        ["sample in vec4 c;", "float sb_kw_shared;"]
    );
    let out = run("#version 430\nshared vec4 cache[64];\nlayout(std430) buffer B { float x; };\n");
    assert_eq!(
        norm(&out.code),
        [
            "shared vec4 cache[64];",
            "layout(std430) buffer B { float x; };"
        ]
    );
    let out = run("#version 330\n#extension GL_ARB_compute_shader : enable\nshared float s;\n");
    assert_eq!(norm(&out.code), ["shared float s;"]);
    // Layout qualifier identifiers are left alone.
    let out =
        run("#version 330\nlayout(shared, packed) uniform U { float shared; float packed; };\n");
    assert_eq!(
        norm(&out.code),
        ["layout(shared, packed) uniform U { float sb_kw_shared; float packed; };"]
    );
    // The version is taken from the (first) #version, even if it comes later.
    let out = run("float sample;\n#version 450\n");
    assert_eq!(norm(&out.code), ["float sample;"]);
    // Macro-produced identifiers are escaped too; macros named like keywords are expanded first.
    let out = run("#define IN input\n#define half float\nhalf IN;\n");
    assert_eq!(norm(&out.code), ["float sb_kw_input;"]);
    // Disabled.
    let out = run_opts(
        "float input;\n",
        &PreprocessOptions {
            escape_reserved_words: false,
            ..Default::default()
        },
    );
    assert_eq!(norm(&out.code), ["float input;"]);
    // Vulkan keyword used as a parameter name (spectrum).
    assert_eq!(
        norm(&run("vec4 f(sampler2D sampler) { return texture(sampler, vec2(0)); }\n").code),
        ["vec4 f(sampler2D sb_kw_sampler) { return texture(sb_kw_sampler, vec2(0)); }"]
    );
}

/// Context-sensitive reserved words used as identifiers in old versions are
/// escaped (every occurrence, including ones whose context alone is ambiguous).
#[test]
fn contextual_words_used_as_identifiers_are_escaped() {
    let cases: &[(&str, &[&str])] = &[
        (
            "float smooth = 0.5;\nfloat y = smooth * 2.0;\n",
            &["float sb_kw_smooth = 0.5;", "float y = sb_kw_smooth * 2.0;"],
        ),
        (
            "vec3 flat;\nvoid main() { flat.x = 1.0; }\n",
            &["vec3 sb_kw_flat;", "void main() { sb_kw_flat.x = 1.0; }"],
        ),
        (
            "float f(float noperspective) { return noperspective; }\n",
            &["float f(float sb_kw_noperspective) { return sb_kw_noperspective; }"],
        ),
        ("int coherent = 1;\n", &["int sb_kw_coherent = 1;"]),
        (
            "bool volatile, restrict;\n",
            &["bool sb_kw_volatile, sb_kw_restrict;"],
        ),
        (
            "struct S { int readonly; int writeonly[2]; };\nint g(S s) { return s.readonly + s.writeonly[1]; }\n",
            &[
                "struct S { int sb_kw_readonly; int sb_kw_writeonly[2]; };",
                "int g(S s) { return s.sb_kw_readonly + s.sb_kw_writeonly[1]; }",
            ],
        ),
        (
            "float layout = 1.0;\nvoid main() { layout += 2.0; }\n",
            &[
                "float sb_kw_layout = 1.0;",
                "void main() { sb_kw_layout += 2.0; }",
            ],
        ),
        (
            "uniform float precision;\nfloat p = precision;\n",
            &[
                "uniform float sb_kw_precision;",
                "float p = sb_kw_precision;",
            ],
        ),
        (
            "const float lowp = 0.1, mediump = 0.5, highp = 1.0;\nfloat q = mix(lowp, highp, mediump);\n",
            &[
                "const float sb_kw_lowp = 0.1, sb_kw_mediump = 0.5, sb_kw_highp = 1.0;",
                "float q = mix(sb_kw_lowp, sb_kw_highp, sb_kw_mediump);",
            ],
        ),
        (
            "bool switch = true;\nvoid main() { if (switch) { switch = !switch; } }\n",
            &[
                "bool sb_kw_switch = true;",
                "void main() { if (sb_kw_switch) { sb_kw_switch = !sb_kw_switch; } }",
            ],
        ),
        // `case - 1` alone is ambiguous (`case -1:`); the declaration decides.
        (
            "int case = 2;\nvoid main() { int y; case - 1; y = case; }\n",
            &[
                "int sb_kw_case = 2;",
                "void main() { int y; sb_kw_case - 1; y = sb_kw_case; }",
            ],
        ),
        (
            "vec3 default = vec3(1.0);\nvec3 pick(bool c) { return c ? default : vec3(0.0); }\n",
            &[
                "vec3 sb_kw_default = vec3(1.0);",
                "vec3 pick(bool c) { return c ? sb_kw_default : vec3(0.0); }",
            ],
        ),
        // `sqrt(double)` alone is ambiguous (an unnamed parameter); the declaration decides.
        (
            "float double = 4.0;\nfloat r = sqrt(double);\n",
            &["float sb_kw_double = 4.0;", "float r = sqrt(sb_kw_double);"],
        ),
        // A function named `layout`: `layout(` alone is ambiguous, the call after `=` is not.
        (
            "vec2 layout(vec2 p) { return p; }\nvec2 q = layout(vec2(0.0));\n",
            &[
                "vec2 sb_kw_layout(vec2 p) { return p; }",
                "vec2 q = sb_kw_layout(vec2(0.0));",
            ],
        ),
        // Macro-produced, and context across lines and comments.
        (
            "#define NAME smooth\nfloat NAME /* c */\n  = 1.0;\n",
            &["float sb_kw_smooth /* c */", "= 1.0;"],
        ),
    ];
    for (body, expect) in cases {
        for version in ["", "#version 110\n", "#version 120\n"] {
            let src = format!("{version}{body}");
            let out = run(&src);
            assert!(out.diagnostics.is_empty(), "{src}: {:?}", out.diagnostics);
            assert_eq!(norm(&out.code), *expect, "{src}");
        }
    }
}

/// Keyword uses of the context-sensitive words, which lenient compilers accept
/// in old versions, are never escaped.
#[test]
fn contextual_words_used_as_keywords_are_kept() {
    let cases: &[&str] = &[
        "#version 120\n#extension GL_EXT_gpu_shader4 : enable\nflat varying vec3 n;\nnoperspective varying float d;\n",
        "#version 120\nflat varying vec3 n;\nsmooth varying vec2 uv;\nnoperspective varying float d;\nflat /* c */\n  varying int id;\n",
        "#version 330\nlayout(rgba8) coherent uniform image2D a;\nvoid f(readonly image2D b, writeonly restrict volatile image2D c);\n",
        "#version 130\nlayout(location = 0) out vec4 color;\nlayout(std140) uniform U { vec4 v; };\n",
        "#version 120\nlayout(location = 0) out vec4 color;\n",
        "#version 120\nprecision highp float;\nuniform lowp sampler2D t;\nmediump vec3 v;\nhighp float f(highp float x) { return x; }\n",
        "#version 120\n#define FOO 3\nvoid main() { int x; switch (x) { case 0: x = 1; break; case -1: case (2): case FOO: case +4: case ~5: case !true: break; default: break; } if (x > 0) switch (x) { default : x = 2; } }\n",
        "#version 330\ndouble d = double(1.0);\ndouble a[2];\nvoid f(double, double);\ndouble g(double x) { return x; }\n",
        "#version 120\n#define Q flat\n#define V varying\nQ V vec3 n;\n",
        // Contradicting evidence (a keyword use next to an identifier use): left alone.
        "#version 120\nflat varying vec3 n;\nvoid main() { float y = flat; }\n",
        "#version 120\nvoid main() { int i; switch (i) { default: break; } int default = 1; }\n",
    ];
    for src in cases {
        let out = run(src);
        assert!(!out.code.contains(ESCAPE_PREFIX), "{src}\n{}", out.code);
    }
}

/// Identifier uses are left alone from the version (or with the extension)
/// that makes the word a keyword: the pack cannot mean an identifier there.
#[test]
fn contextual_words_respect_versions_and_extensions() {
    let id_uses: String = crate::escape::CONTEXTUAL_RESERVED
        .iter()
        .map(|w| format!("float {} = 0.0;\n", w.word))
        .collect();
    let escaped = |src: &str| {
        let out = run(src);
        let mut words: Vec<String> = out
            .code
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .filter_map(|w| w.strip_prefix(ESCAPE_PREFIX))
            .map(str::to_owned)
            .collect();
        words.sort();
        words
    };
    let sorted = |w: &[&str]| {
        let mut v: Vec<String> = w.iter().map(|s| s.to_string()).collect();
        v.sort();
        v
    };
    let all = sorted(&[
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
    ]);
    assert_eq!(escaped(&format!("#version 120\n{id_uses}")), all);
    assert_eq!(
        escaped(&format!("#version 130\n{id_uses}")),
        sorted(&[
            "coherent",
            "volatile",
            "restrict",
            "readonly",
            "writeonly",
            "layout",
            "double"
        ])
    );
    assert_eq!(
        escaped(&format!("#version 140\n{id_uses}")),
        sorted(&[
            "coherent",
            "volatile",
            "restrict",
            "readonly",
            "writeonly",
            "double"
        ])
    );
    assert_eq!(
        escaped(&format!("#version 400\n{id_uses}")),
        sorted(&["coherent", "volatile", "restrict", "readonly", "writeonly"])
    );
    assert!(escaped(&format!("#version 420\n{id_uses}")).is_empty());
    assert!(escaped(&format!("#version 460\n{id_uses}")).is_empty());
    let with_ext = escaped(&format!(
        "#version 120\n#extension GL_EXT_gpu_shader4 : enable\n#extension GL_ARB_shader_image_load_store : enable\n#extension GL_ARB_explicit_attrib_location : require\n#extension GL_ARB_gpu_shader_fp64 : enable\n{id_uses}"
    ));
    assert_eq!(
        with_ext,
        sorted(&[
            "precision",
            "lowp",
            "mediump",
            "highp",
            "switch",
            "case",
            "default"
        ])
    );
    // A disabled extension does not count.
    assert_eq!(
        escaped("#version 120\n#extension GL_EXT_gpu_shader4 : disable\nfloat flat;\n"),
        ["flat"]
    );
    // Escaping can be turned off.
    let out = run_opts(
        "#version 120\nfloat flat;\n",
        &PreprocessOptions {
            escape_reserved_words: false,
            ..Default::default()
        },
    );
    assert_eq!(norm(&out.code), ["float flat;"]);
    // Inside layout(...) nothing is escaped.
    assert_eq!(
        norm(&run("#version 120\nlayout(flat = 1) out vec4 c; float flat;\n").code),
        ["layout(flat = 1) out vec4 c; float sb_kw_flat;"]
    );
    // Unresolved context at the end of the unit is ambiguous.
    assert_eq!(
        norm(&run("#version 120\nfloat x = 1.0; switch\n").code),
        ["float x = 1.0; switch"]
    );
}

#[test]
fn diagnostics_have_locations_in_included_files() {
    let out = run_files(
        &[
            ("main.fsh", "#include \"lib/x.glsl\"\n"),
            ("lib/x.glsl", "\n\n#error from include\n"),
        ],
        "main.fsh",
        &PreprocessOptions::default(),
    );
    let d = out.diagnostics.iter().next().expect("diag");
    assert_eq!(d.code, "pp.error");
    assert_eq!(d.location, Some(SourceLocation::new("lib/x.glsl", 3)));
}

#[test]
fn serde_round_trip() {
    let out = run("#version 330 core\n#extension GL_X : enable\nint a;\n");
    let json = serde_json::to_string(&out).expect("serialize");
    assert!(json.contains("\"core\""));
    let back: Preprocessed = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, out);
}

// ----------------------------------------------------------------------
// Review regressions: behaviours checked against JCPP 1.4.14 (Iris)
// ----------------------------------------------------------------------

#[test]
fn defined_recovery_matches_jcpp() {
    // A missing `)` is reported but the operand's value is kept (JCPP).
    let out = run("#define A\n#if defined(A\nyes\n#else\nno\n#endif\n");
    assert_eq!(norm(&out.code), ["yes"]);
    assert_eq!(codes(&out), ["pp.bad-expression"]);
    assert!(!out.diagnostics.has_errors());
    let out = run("#define A\n#if defined(A || defined(B)\nyes\n#endif\n");
    assert_eq!(norm(&out.code), ["yes"]);
    // A non-identifier operand is consumed and evaluates to 0; evaluation continues.
    let out = run("#if defined 3 || 1\nyes\n#endif\n#if defined(3) || 1\nyes2\n#endif\n");
    assert_eq!(norm(&out.code), ["yes", "yes2"]);
    assert_eq!(codes(&out), ["pp.bad-expression", "pp.bad-expression"]);
    // The same recovery applies to `defined` produced by macro expansion.
    let out = run("#define A\n#define D defined(A\n#if D\nyes\n#endif\n");
    assert_eq!(norm(&out.code), ["yes"]);
    assert_eq!(codes(&out), ["pp.bad-expression"]);
}

#[test]
fn if_numbers_match_jcpp() {
    let out = run("#if 08 == 8\na\n#endif\n");
    assert_eq!(norm(&out.code), ["a"]);
    assert_eq!(codes(&out), ["pp.bad-number"]);
    // No binary literals (JCPP: invalid token; GLSL has none).
    let out = run("#if 0b101\na\n#else\nb\n#endif\n");
    assert_eq!(norm(&out.code), ["b"]);
    assert_eq!(codes(&out), ["pp.bad-expression"]);
    // Multi-character constants are invalid tokens for JCPP.
    let out = run("#if 'ab'\na\n#else\nb\n#endif\n");
    assert_eq!(norm(&out.code), ["b"]);
    assert_eq!(codes(&out), ["pp.bad-expression"]);
    let out = run("#if 0x1p3 == 8\na\n#endif\n");
    assert_eq!(norm(&out.code), ["a"]);
    assert_eq!(codes(&out), ["pp.float-in-condition"]);
}

#[test]
fn counter_builtin_like_jcpp() {
    let out = run("#ifdef __COUNTER__\nint a = __COUNTER__;\nint b = __COUNTER__;\n#endif\n");
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(norm(&out.code), ["int a = 0;", "int b = 1;"]);
    // Per run, not per preprocessor.
    let m = MemorySources::new().with("a.fsh", "__COUNTER__\n");
    let mut pp = Preprocessor::new(&m);
    for _ in 0..2 {
        assert_eq!(pp.preprocess("a.fsh", &PreprocessOptions::default()).code, "0\n");
    }
}

#[test]
fn stray_hash_in_program_text_is_an_error() {
    // JCPP throws "Bad token" for `#`/`##` outside directives, so Iris rejects these.
    let out = run("x # define Y\nint a; /* c\n*/ # define Z 1\n");
    assert_eq!(codes(&out), ["pp.stray-hash"]);
    assert!(out.diagnostics.has_errors());
    assert_eq!(out.diagnostics.iter().next().and_then(|d| d.location.as_ref()).map(|l| l.line), Some(1));
    // A `#` produced by macro expansion is caught too (reported once per run).
    let out = run("#define P(x) [x] # y\nP(1) P(2)\n");
    assert_eq!(codes(&out), ["pp.stray-hash"]);
    // A `#` in comments, strings and directives is fine.
    let out = run("// # x\n/* ## */ int a; \"#\"\n  #  define Q 1\nQ\n");
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    // Properties mode deletes `#` from values instead.
    let (_, diags) = preprocess_properties("#define H x\na=H\n", "shaders.properties", &IndexMap::new());
    assert!(diags.is_empty());
}

#[test]
fn import_directive_is_an_error_like_iris() {
    let out = run("#import \"x\"\nok\n");
    assert_eq!(codes(&out), ["pp.include-unsupported"]);
    assert!(out.diagnostics.has_errors());
    // `# include` (with a space) is ignored by Iris's include resolver and JCPP
    // drops it after a logged error: a warning, same output.
    let out = run("# include \"x\"\nok\n");
    assert!(!out.diagnostics.has_errors());
}

#[test]
fn java_line_breaks_split_lines_like_iris() {
    // Form feed / vertical tab / U+2028 end lines (Iris splits with Java `\R`),
    // so the code after a `//` comment on the same physical line is live.
    let out = run("// comment\u{0C}int a;\u{0B}#define X 1\u{2028}X\n");
    assert_eq!(norm(&out.code), ["// comment", "int a;", "1"]);
    assert_eq!(
        out.line_map.iter().map(|l| l.line).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
}
