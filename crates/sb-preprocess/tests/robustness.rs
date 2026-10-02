//! Randomized robustness test: arbitrary combinations of preprocessor
//! fragments must never panic or hang, and the output invariants must hold.

use indexmap::IndexMap;
use sb_core::MemorySources;
use sb_preprocess::{PreprocessOptions, Preprocessor, preprocess_properties};

/// xorshift64* — deterministic, dependency-free.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const FRAGMENTS: &[&str] = &[
    "#define ",
    "#undef ",
    "#if ",
    "#ifdef ",
    "#ifndef ",
    "#elif ",
    "#else",
    "#endif",
    "#include \"a.glsl\"",
    "#include \"/b.glsl\"",
    "#include \"missing.glsl\"",
    "#line ",
    "#pragma x",
    "#version 330",
    "#version 120 compatibility",
    "#extension GL_X : enable",
    "#error e",
    "#warning w",
    "# ",
    "A",
    "B",
    "F",
    "G",
    "(",
    ")",
    ",",
    "##",
    "#",
    "...",
    "__VA_ARGS__",
    "__LINE__",
    "defined",
    "\\",
    "\n",
    "\n",
    "\n",
    "\n",
    " ",
    "\t",
    "/*",
    "*/",
    "//",
    "\"",
    "'",
    "1",
    "0x1F",
    "1.5",
    "09",
    "+",
    "-",
    "*",
    "/",
    "?",
    ":",
    "<<",
    "!",
    "input",
    "layout",
    "sample",
    "\r\n",
    "\r",
    "\u{FEFF}",
    "\0",
    "é",
    "F(",
    "A B",
    "\n#define F(x) x x\n",
    "\n#define A A B\n",
    "\n#define B A\n",
    "\n#define G(x, ...) x ## __VA_ARGS__ #x\n",
    "\n#define H(a) F(a)(a)\n",
    "\n#if 1\n",
    "\n#endif\n",
];

fn random_text(rng: &mut Rng, max: usize) -> String {
    let n = rng.below(max);
    let mut s = String::new();
    for _ in 0..n {
        s.push_str(FRAGMENTS[rng.below(FRAGMENTS.len())]);
    }
    s
}

#[test]
fn random_inputs_never_panic_and_keep_invariants() {
    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    let opts = PreprocessOptions::default()
        .with_define("A", "1")
        .with_flag("FLAG");
    let mut defines = IndexMap::new();
    defines.insert("FLAG".to_string(), None);
    defines.insert("V".to_string(), Some("2".to_string()));
    for _ in 0..1500 {
        let main = random_text(&mut rng, 120);
        let a = random_text(&mut rng, 60);
        let b = random_text(&mut rng, 60);
        let m = MemorySources::new()
            .with("main.fsh", &main)
            .with("a.glsl", &a)
            .with("b.glsl", &b);
        let mut pp = Preprocessor::new(&m);
        for entry in ["main.fsh", "a.glsl"] {
            let out = pp.preprocess(entry, &opts);
            assert_eq!(
                out.line_map.len(),
                out.code.lines().count(),
                "line map mismatch for input {main:?}"
            );
            assert!(out.code.is_empty() || out.code.ends_with('\n'));
            let (files, _) = pp.include_closure(entry);
            for f in &out.files {
                assert!(files.contains(f));
            }
        }
        let (props, _) = preprocess_properties(&main, "shaders.properties", &defines);
        assert!(props.is_empty() || props.ends_with('\n'));
    }
}

#[test]
fn pathological_inputs_finish() {
    let cases = [
        "\\".repeat(10_000),
        "/*".repeat(10_000),
        "#if 1\n".repeat(5_000),
        "#endif\n".repeat(5_000),
        format!("#define F(x) x\n{}", "F(".repeat(20_000)),
        format!("#define F(x) F\n{}", "F(F)".repeat(20_000)),
        format!(
            "#define F(a,b) a\n{}{}",
            "F(".repeat(5_000),
            ")".repeat(5_000)
        ),
        format!(
            "#define F(a) a\n{}1{}",
            "F(".repeat(5_000),
            ")".repeat(5_000)
        ),
        "\"".repeat(10_000),
        "#define A B\n#define B C\n#define C A\n".to_string() + &"A ".repeat(50_000),
        "\0\0\0\u{FEFF}\r\r\r".repeat(1_000),
    ];
    for src in &cases {
        let m = MemorySources::new().with("main.fsh", src);
        let mut pp = Preprocessor::new(&m);
        let out = pp.preprocess("main.fsh", &PreprocessOptions::default());
        assert_eq!(out.line_map.len(), out.code.lines().count());
        let _ = preprocess_properties(src, "block.properties", &IndexMap::new());
    }
}

/// Regression (review): a long chain of macros (`M_i` -> `M_{i-1} + i`) made
/// hide sets grow quadratically and was killed for running out of memory
/// (200k macros). Hide-set storage is now bounded and expansion stops with an
/// error diagnostic.
#[test]
fn long_macro_chains_are_bounded() {
    let mut src = String::new();
    for i in 1..30_000u32 {
        src.push_str(&format!("#define M{i} M{} + {i}\n", i - 1));
    }
    src.push_str("int x = M29999;\nint y = M10;\n");
    let m = MemorySources::new().with("main.fsh", &src);
    let mut pp = Preprocessor::new(&m);
    let out = pp.preprocess("main.fsh", &PreprocessOptions::default());
    assert_eq!(out.line_map.len(), out.code.lines().count());
    assert!(
        out.diagnostics.iter().any(|d| d.code == "pp.expansion-limit" && d.is_error()),
        "{:?}",
        out.diagnostics
    );
    // A realistic chain depth (a few hundred) expands fully.
    let mut src = String::new();
    for i in 1..500u32 {
        src.push_str(&format!("#define M{i} M{} + 1\n", i - 1));
    }
    src.push_str("#define M0 0\nint x = M499;\n");
    let m = MemorySources::new().with("main.fsh", &src);
    let mut pp = Preprocessor::new(&m);
    let out = pp.preprocess("main.fsh", &PreprocessOptions::default());
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(out.code.matches("+ 1").count(), 499);
}

#[test]
fn preprocessor_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Preprocessor<'static>>();
    assert_send_sync::<sb_preprocess::Preprocessed>();
}
