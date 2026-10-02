//! Differential tests against Iris's real preprocessor.
//!
//! `tests/jcpp/IrisHarness.java` reproduces Iris 1.11's preprocessing on top
//! of the JCPP 1.4.14 library that Iris ships: include expansion
//! (IncludeGraph/FileNode/IncludeProcessor), `JcppProcessor.glslPreprocessSource`
//! (including the `#version`/`#extension` hoisting hack) and
//! `PropertiesPreprocessor`. These tests compare sb-preprocess against it on
//! hand-written edge cases and on every program and `.properties` file of the
//! corpus. Every intentional deviation is listed with its reason.
//!
//! Requirements (the tests pass with a notice when any is missing):
//! * `java` and `javac` on the `PATH`;
//! * the JCPP jar: `SB_JCPP_JAR`, default
//!   `<scratchpad>/mc/x_iris/META-INF/jars/jcpp-1.4.14.jar` (extracted from the Iris jar);
//! * the slf4j-api jar: `SB_SLF4J_JAR`, default the first `slf4j-api-*.jar`
//!   in `/opt/*/lib`.

mod common;

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use common::{CORPUS_DEFINES, DirSources, corpus_options, corpus_root, program_files, shader_roots};
use indexmap::IndexMap;
use sb_core::MemorySources;
use sb_preprocess::{PreprocessOptions, Preprocessed, Preprocessor, preprocess_properties};

const DEFAULT_JCPP_JAR: &str = "/tmp/claude-0/-home-user-super-adventure/14a9b258-2170-5e12-9e2c-1c1a40e3de07/scratchpad/mc/x_iris/META-INF/jars/jcpp-1.4.14.jar";

struct Harness {
    classpath: String,
    dir: PathBuf,
}

fn find_slf4j() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SB_SLF4J_JAR") {
        return Some(PathBuf::from(p));
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir("/opt")
        .ok()?
        .flatten()
        .filter_map(|e| std::fs::read_dir(e.path().join("lib")).ok())
        .flat_map(|rd| rd.flatten().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("slf4j-api-") && n.ends_with(".jar"))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

fn build_harness() -> Result<Harness, String> {
    let jcpp = std::env::var_os("SB_JCPP_JAR")
        .map_or_else(|| PathBuf::from(DEFAULT_JCPP_JAR), PathBuf::from);
    if !jcpp.is_file() {
        return Err(format!("JCPP jar not found at {}", jcpp.display()));
    }
    let slf4j = find_slf4j().ok_or("slf4j-api jar not found")?;
    let javac_ok = Command::new("javac")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !javac_ok {
        return Err("javac not found".into());
    }
    let dir = std::env::temp_dir().join(format!("sb-preprocess-jcpp-{}", std::process::id()));
    let classes = dir.join("classes");
    std::fs::create_dir_all(&classes).map_err(|e| e.to_string())?;
    let jars = format!("{}:{}", jcpp.display(), slf4j.display());
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/jcpp/IrisHarness.java");
    let out = Command::new("javac")
        .args(["-nowarn", "-cp", &jars, "-d"])
        .arg(&classes)
        .arg(&src)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "javac failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(Harness {
        classpath: format!("{}:{jars}", classes.display()),
        dir,
    })
}

fn harness() -> Option<&'static Harness> {
    static H: OnceLock<Result<Harness, String>> = OnceLock::new();
    match H.get_or_init(build_harness) {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("JCPP differential test skipped: {e}");
            None
        }
    }
}

impl Harness {
    fn run(&self, args: &[&Path], stdin: Option<&str>) {
        let mut cmd = Command::new("java");
        cmd.arg("-cp").arg(&self.classpath).arg("IrisHarness");
        cmd.args(args);
        cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn java");
        let mut pipe = child.stdin.take().expect("stdin");
        if let Some(s) = stdin {
            pipe.write_all(s.as_bytes()).expect("write stdin");
        }
        drop(pipe);
        let out = child.wait_with_output().expect("java");
        assert!(
            out.status.success(),
            "IrisHarness failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn scratch(&self, name: &str) -> PathBuf {
        let d = self.dir.join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("scratch dir");
        d
    }
}

fn write_defines(path: &Path, defines: &[(&str, &str)]) {
    let text: String = defines
        .iter()
        .map(|(k, v)| {
            if v.is_empty() {
                format!("{k}\n")
            } else {
                format!("{k}={v}\n")
            }
        })
        .collect();
    std::fs::write(path, text).expect("write defines");
}

/// `<base>.<ext>` (appended, unlike `Path::with_extension`: program names
/// already have an extension).
fn sibling(base: &Path, ext: &str) -> PathBuf {
    PathBuf::from(format!("{}.{ext}", base.display()))
}

fn strip_ws(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Remove `//` and `/* */` comments (string-literal aware).
fn strip_comments(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
            out.push(' ');
        } else if b[i] == b'"' {
            let st = i;
            i += 1;
            while i < b.len() && b[i] != b'"' && b[i] != b'\n' {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            out.push_str(&String::from_utf8_lossy(&b[st..i]));
        } else {
            let len = s[i..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&s[i..i + len]);
            i += len;
        }
    }
    out
}

fn first_diff(ours: &str, theirs: &str) -> String {
    let a: Vec<char> = ours.chars().collect();
    let b: Vec<char> = theirs.chars().collect();
    let i = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let st = i.saturating_sub(60);
    let snip = |v: &[char]| v[st..(i + 60).min(v.len())].iter().collect::<String>();
    format!("\n    ours: ...{}\n    iris: ...{}", snip(&a), snip(&b))
}

/// Iris's output body with the `#version`/`#extension` markers that leak into
/// comments turned back into the original text.
fn iris_body(base: &Path) -> Option<String> {
    let body = std::fs::read_to_string(sibling(base, "body")).ok()?;
    Some(
        body.replace("#warning IRIS_JCPP_GLSL_VERSION", "#version")
            .replace("#warning IRIS_JCPP_GLSL_EXTENSION", "#extension"),
    )
}

/// Hoisted lines (`#version`/`#extension`) without whitespace and comments.
fn iris_header(base: &Path) -> Vec<String> {
    std::fs::read_to_string(sibling(base, "hdr"))
        .unwrap_or_default()
        .lines()
        .map(|l| strip_ws(&strip_comments(l)))
        .collect()
}

fn our_header(out: &Preprocessed) -> Vec<String> {
    out.header().lines().map(strip_ws).collect()
}

fn non_blank_lines(s: &str) -> Vec<String> {
    s.lines().map(strip_ws).filter(|l| !l.is_empty()).collect()
}

/// Compare one GLSL result. `None` when identical (ignoring whitespace,
/// with and without comments, plus the order of non-blank lines).
fn compare_glsl(out: &Preprocessed, base: &Path) -> Option<String> {
    let Some(theirs) = iris_body(base) else {
        let why = std::fs::read_to_string(sibling(base, "fail")).unwrap_or_default();
        return Some(format!("Iris cannot load it: {why}"));
    };
    let (hdr_o, hdr_t) = (our_header(out), iris_header(base));
    if hdr_o.first() != hdr_t.iter().find(|l| l.starts_with("#version"))
        || hdr_o.iter().filter(|l| l.starts_with("#extension")).collect::<Vec<_>>()
            != hdr_t.iter().filter(|l| l.starts_with("#extension")).collect::<Vec<_>>()
    {
        return Some(format!("header: ours {hdr_o:?}, iris {hdr_t:?}"));
    }
    let (a, b) = (strip_ws(&out.code), strip_ws(&theirs));
    if a != b {
        return Some(format!("text differs:{}", first_diff(&a, &b)));
    }
    let (a, b) = (
        strip_ws(&strip_comments(&out.code)),
        strip_ws(&strip_comments(&theirs)),
    );
    if a != b {
        return Some(format!("code differs:{}", first_diff(&a, &b)));
    }
    if non_blank_lines(&out.code) != non_blank_lines(&theirs) {
        return Some("line structure differs".into());
    }
    None
}

// ----------------------------------------------------------------------
// Edge cases
// ----------------------------------------------------------------------

/// Expected relation to Iris for one snippet.
#[derive(Clone, Copy)]
enum Expect {
    /// Same output and hoisted directives as Iris.
    Same,
    /// Intentional deviation (reason); asserted to still differ so that the
    /// list stays accurate.
    Differs(&'static str),
    /// JCPP throws, so Iris refuses the program; `true` when we report an error too.
    IrisRejects(bool, &'static str),
}

use Expect::{Differs, IrisRejects, Same};

const CASES: &[(&str, &str, Expect)] = &[
    // --- conditionals ---
    ("if_empty_define", "#define E\n#if E\na\n#else\nb\n#endif\nend\n", Same),
    ("if_noexpr", "#if\na\n#else\nb\n#endif\nend\n", Same),
    ("ifdef_num", "#ifdef 3\na\n#else\nb\n#endif\nend\n", Same),
    ("ifdef_noarg", "#ifdef\na\n#else\nb\n#endif\nend\n",
        Differs("JCPP consumes the newline as the operand and swallows the next line; we report an error and treat the group as true")),
    ("ifndef_noarg", "#ifndef\na\n#else\nb\n#endif\nend\n",
        Differs("as ifdef_noarg")),
    ("elif_after_else", "#if 0\na\n#else\nb\n#elif 1\nc\n#endif\nend\n", Same),
    ("elif_after_else2", "#if 1\na\n#else\nb\n#elif 1\nc\n#endif\nend\n", Same),
    ("else_after_else", "#if 0\na\n#else\nb\n#else\nc\n#endif\nend\n", Same),
    ("else_after_else2", "#if 1\na\n#else\nb\n#else\nc\n#endif\nend\n", Same),
    ("nested_inactive", "#if 0\n#if garbage (((\n#elif 1/0\n#else\n#error nope\n#endif\n#ifdef\n#endif\n#elif 1\nx\n#else\ny\n#endif\nz\n", Same),
    ("elif_garbage_taken", "#if 1\na\n#elif (((\nb\n#endif\nend\n", Same),
    ("elif_garbage_untaken", "#if 0\na\n#elif (((\nb\n#else\nc\n#endif\nend\n", Same),
    ("dir_junk", "#define X\n#ifdef X junk\na\n#else junk\nb\n#endif junk\nend\n", Same),
    ("elifdef", "#if 0\n#elifdef X\na\n#endif\nend\n", Same),
    // --- #if expressions ---
    ("float_if", "#if 1.5 > 1\na\n#endif\n#if 0.75\nb\n#endif\n#if 1e3 == 1000\nc\n#endif\n#if 2.0f == 2\nd\n#endif\n#if .5\ne\n#endif\n#if 1.5e1 == 15\nf\n#endif\n#if 1.5e1 == 10\ng\n#endif\nend\n", Same),
    ("true_false", "#if true\na\n#endif\n#if !false\nb\n#endif\nend\n", Same),
    ("divzero", "#if 1/0\na\n#else\nb\n#endif\n#if 0 && 1/0\nc\n#else\nd\n#endif\nend\n", Same),
    ("trailing_tokens", "#if 1 2\na\n#else\nb\n#endif\nend\n", Same),
    ("defined_forms", "#define A\n#if defined A && defined(A) && !defined B\na\n#endif\n#define D defined(A)\n#if D\nb\n#endif\n#define DX defined X\n#if DX\nc\n#else\nd\n#endif\nend\n", Same),
    ("defined_noarg", "#if defined\na\n#else\nb\n#endif\nend\n", Same),
    ("defined_missing_paren", "#define A\n#if defined(A\na\n#else\nb\n#endif\nend\n", Same),
    ("defined_bad_operand", "#if defined(3) || 1\na\n#endif\nend\n", Same),
    ("charlit", "#if 'a' == 97\na\n#endif\n#if 'ab'\nb\n#endif\n#if '\\n' == 10\nc\n#endif\nend\n", Same),
    ("ternary", "#if 1 ? 2 : (1/0)\na\n#endif\n#if 0 ? 1 : 0 ? 2 : 3\nb\n#endif\n#if (1 ? 0 : 1) + 1 == 1\nc\n#endif\nend\n", Same),
    ("shifts", "#if (1 << 63) < 0\na\n#endif\n#if (-8 >> 1) == -4\nb\n#endif\n#if (1 << 64) == 1\nc\n#endif\n#if (1 << -1) < 0\nd\n#endif\nend\n", Same),
    ("unsigned_suffix", "#if 10u + 2UL == 12\na\n#endif\n#if -1 > 0u\nb\n#endif\nend\n", Same),
    ("octal", "#if 010 == 8\na\n#endif\n#if 08 == 8\nb\n#endif\nend\n", Same),
    ("binary", "#if 0b101 == 5\na\n#endif\nend\n", Same),
    ("hexfloat", "#if 0x1p3 == 8\na\n#endif\nend\n", Same),
    ("bigint", "#if 99999999999999999999 > 0\na\n#endif\n", IrisRejects(true, "Long.parseLong overflow")),
    ("if_string", "#if \"abc\"\na\n#else\nb\n#endif\nend\n", Same),
    ("if_assign", "#if 1 = 1\na\n#else\nb\n#endif\nend\n", Same),
    ("if_comma", "#if (1, 0)\na\n#else\nb\n#endif\nend\n", Same),
    ("neg_mod", "#if (-7 % 3) == -1 && (-7 / 2) == -3\na\n#endif\nend\n", Same),
    ("minus_minus", "#if - - 3 == 3 && ~0 == -1 && !0 == 1\na\n#endif\nend\n", Same),
    ("if_macro_fn_partial", "#define F(x) x\n#if F\na\n#else\nb\n#endif\nend\n", Same),
    ("if_macro_fn_args", "#define F(x) ((x)*2)\n#if F(3) == 6\na\n#endif\n#if F(\n3) == 6\nb\n#endif\nend\n",
        Differs("JCPP collects macro arguments across the end of an #if line; we report an unterminated call")),
    ("ifdef_keyword", "#ifdef defined\na\n#else\nb\n#endif\nend\n", Same),
    // --- macros ---
    ("comment_in_define", "#define A 1 /* c */ + 2\nx = A;\n#define B 3 // trailing\ny = B;\n", Same),
    ("comment_in_args", "#define F(x) [x]\nF(/*c*/1) F(1 /* d */) F(\n// line\n2)\nend\n", Same),
    ("comment_before_paren", "#define F(x) [x]\nF /* c */ (1)\nend\n", Same),
    ("stringify_comment", "#define S(x) #x\nS(a /* c */ b) S(  a  \n  b  )\nend\n", Same),
    ("stringify_quotes", "#define S(x) #x\nS(\"a\\\"b\" '\\'' \\n)\nend\n",
        Differs("C stringification escapes backslashes only inside string/char literals; JCPP escapes them everywhere")),
    ("paste_invalid", "#define C(a,b) a##b\nC(+,-) C(.,.) C(1,.5) C(x,1.5)\nend\n", Same),
    ("paste_invalid_comment", "#define C(a,b) a##b\nC(/,/) end\n",
        Differs("an invalid paste keeps both tokens apart ('/ /', GCC); JCPP re-lexes '//' into a comment that swallows the rest of the line")),
    ("paste_empty", "#define C(a,b) a##b\n[C(,)] [C(a,)] [C(,b)]\nend\n", Same),
    ("va_args", "#define V(...) f(__VA_ARGS__)\n#define W(a,...) g(a, ## __VA_ARGS__)\n#define S(...) #__VA_ARGS__\nV() V(1,2) W(1) W(1,2) S(a, b)\nend\n", Same),
    ("va_named", "#define N(args...) h(args)\nN(x, y)\nend\n", Same),
    ("macro_args_count", "#define F(a,b) a b\nF(1)\nF(1,2,3)\nnext\n",
        Differs("both report an error; JCPP drops the arguments, we keep the invocation text")),
    ("va_count", "#define V(a, b, ...) x\nV(1)\nV(1,2)\nend\n", Differs("as macro_args_count")),
    ("macro_eof", "#define F(x) [x]\nF", Same),
    ("macro_eof_args", "#define F(x) [x]\nF(1, 2\nmore\n",
        Differs("both report an error; JCPP drops the unterminated arguments, we keep them")),
    ("multiline_args", "#define F(a,b) (a+b)\nint x = F(1,\n  2) + 3;\nint y;\n", Same),
    ("directive_in_args", "#define F(a) [a]\nF(1\n#ifdef NOPE\n+ 2\n#else\n+ 3\n#endif\n)\nend\n",
        Differs("directives inside macro arguments are processed in place like GCC (with a warning); JCPP produces garbage")),
    ("fn_name_then_directive", "#define F(a) [a]\nF\n#define G\n(1)\nend\n", Same),
    ("recursion", "#define foo foo\nfoo\n#define a b\n#define b a\na b\n#define f(x) f(x + 1)\nf(0)\n#define g(x) x\ng(g)(1)\n", Same),
    ("hidden_set_fn", "#define f(a) a*g\n#define g(a) f(a)\nf(2)(9)\n", Same),
    ("c99_ex3", "#define x 3\n#define f(a) f(x * (a))\n#undef x\n#define x 2\n#define g f\n#define z z[0]\n#define h g(~\n#define m(a) a(w)\n#define w 0,1\n#define t(a) a\n#define p() int\n#define q(x) x\n#define r(x,y) x ## y\n#define str(x) # x\nf(y+1) + f(f(z)) % t(t(g)(0) + t)(1);\ng(x+(3,4)-w) | h 5) & m\n(f)^m(m);\np() i[q()] = { q(1), r(2,3), r(4,), r(,5), r(,) };\nchar c[2][6] = { str(hello), str() };\n",
        Differs("JCPP's hide sets are incomplete (z[0][0][0], 2 *(0)); we follow C99 6.10.3.5 EXAMPLE 3")),
    ("c99_ex4", "#define str(s) # s\n#define xstr(s) str(s)\n#define debug(s, t) printf(\"x\" # s \"= %d, x\" # t \"= %s\", \\\n x ## s, x ## t)\n#define INCFILE(n) vers ## n\n#define glue(a, b) a ## b\n#define xglue(a, b) glue(a, b)\n#define HIGHLOW \"hello\"\n#define LOW LOW \", world\"\ndebug(1, 2);\nfputs(str(strncmp(\"abc\\0d\", \"abc\", '\\4') // this goes away\n == 0) str(: @\\n), s);\nxstr(INCFILE(2).h)\nglue(HIGH, LOW);\nxglue(HIGH, LOW)\n",
        Differs("stringification of a backslash outside literals, as stringify_quotes")),
    ("paste_ident_to_macro", "#define CAT(a,b) a##b\n#define xy 42\nCAT(x, y)\n#define XCAT(a,b) CAT(a,b)\n#define A x\n#define B y\nCAT(A,B) XCAT(A,B)\n", Same),
    ("deep_args_paren", "#define F(a,b) a+b\nF((1,2),(3,4)) F(\")\", ',')\n", Same),
    ("space_paste_avoid", "#define NEG -1\nx-NEG\n#define P +\na+P+b\n#define ID(x) x\nID(a)ID(b)\n#define ONE 1\nONE.5\n", Same),
    ("paste_avoid_comment", "#define E\nx/E/y\n",
        Differs("a space keeps macro-separated tokens apart ('x/ /y'); JCPP emits 'x//y', which starts a comment")),
    ("define_redefine", "#define A 1\n#define A 2\nA\n#define F(x) x\n#define F(y) y\nF(3)\n", Same),
    ("define_bad", "#define\n#define 1x\n#define F(a,a) a\n#define G(a\n#define H(a,) a\nend\n", Same),
    ("undef_noarg", "#undef\n#undef 3\nend\n", Same),
    ("dollar", "#define $x 5\nint a = $x;\n", Same),
    ("fn_obj_paren", "#define O (x)\nO(1)\n#define E() empty\nE() E( )\n#define ONE(x) [x]\nONE()\n", Same),
    ("fn_macro_space_before_paren_in_def", "#define F (x) x\nF(1)\n", Same),
    ("counter", "int c = __COUNTER__;\nint d = __COUNTER__;\n", Same),
    ("undef_builtin", "#undef __LINE__\nint a = __LINE__;\n#define __FILE__ 5\nint b = __FILE__;\n", Same),
    ("builtins", "int a = __LINE__;\n#define L __LINE__\nint b = L;\nint c = __FILE__;\n",
        Differs("__FILE__ is 0 (glslang, spec) instead of JCPP's \"<no file>\"; __LINE__ inside a macro is the line of use (JCPP: line of the #define)")),
    ("line_dir", "#line 100\nint a = __LINE__;\n#line 7 \"foo\"\nint b = __LINE__;\n",
        Differs("#line is honoured (spec); JCPP ignores it")),
    // --- lexing, comments, splices ---
    ("splice_comment", "// comment \\\nint hidden;\nint shown;\n// sp \\ \nint shown2;\n", Same),
    ("splice_ident", "#define FOOBAR 1\nFOO\\\nBAR\n", Same),
    ("splice_define", "#define LONG(a) \\\n  a + \\\n  a\nLONG(1)\n", Same),
    ("unterminated_comment", "int a;\n/* never closed\nstill\n",
        Differs("an unterminated comment is closed in the output (with a warning)")),
    ("unterminated_string", "int a = \"abc;\nint b;\n#define Q \"x\nQ\n", Same),
    ("apostrophe", "// don't\nint x; // it's\ndon't stop\n#define A 1\nA\n", Same),
    ("null_dirs", "#\n#   \n# 42 \"x\"\n#!bang\n#frobnicate\nok\n", Same),
    ("hash_mid", "x # define Y\nY\n", IrisRejects(true, "JCPP throws on '#' outside directives")),
    ("comment_then_dir", "int a; /* open\nclose */ #define Q 5\nQ\n", IrisRejects(true, "as hash_mid")),
    ("comment_then_dir_bol", "/* open\nclose */ #define R 6\nR\n", Same),
    ("obj_hash", "#define H # x\nH\n", IrisRejects(true, "as hash_mid")),
    ("hash_hash_in_object", "#define X a ## ## b\nX\n", IrisRejects(false, "JCPP throws on the second '##'; we paste across both")),
    ("include_like", "#import \"y\"\nend\n", IrisRejects(true, "JCPP throws on #import")),
    ("nul_bom", "\u{feff}#define A 1\nA\n", IrisRejects(false, "a UTF-8 BOM is an invalid token for JCPP; the spec says to strip it")),
    ("crlf", "#define A 1\r\nA\r\n#if 1\r\nb\r\n#endif\r\n", Same),
    ("cr_only", "#define A 1\rA\r", Same),
    ("java_line_breaks", "#define A 1\u{0C}A\u{0B}// c\u{2028}A\n", Same),
    ("ws_vtab", "#define A 1\u{0B}A\n\u{0C}#define B 2\nB\n", Same),
    ("define_noname", "#define\nA 1\nend\n",
        Differs("JCPP consumes the newline as the macro name and swallows the next line; we warn and keep it")),
    // --- version / extension / pragma ---
    ("version_in_comment", "// needs #version 130\n#version 330 compatibility\n/* #extension GL_X : enable */\nint a;\n", Same),
    // Iris hoists every #version; we keep the first (spec) and warn, which is
    // what the GL compiler effectively uses, so the comparison only checks the first.
    ("multi_version", "#version 120\nint a;\n#version 330 core\nint b;\n", Same),
    ("ext_inactive", "#version 330\n#ifdef NOPE\n#extension GL_A : enable\n#endif\n#extension GL_B : require\n#ifdef GL_B\nb\n#endif\nint a;\n",
        Differs("enabled extensions define a macro (spec, like a GL compiler); JCPP leaves it undefined")),
    ("version_macro", "#define V 330\n#version V\nint a;\n",
        Differs("both fail: Iris hoists the unexpanded '#version V', we report pp.bad-version")),
    ("pragma", "#pragma optimize(on)\n#pragma once\nint a;\n", Same),
    ("error_warning", "#error \"msg here\" // c\n#warning careful\nint a;\n", Same),
];

#[test]
fn snippets_match_iris() {
    let Some(h) = harness() else { return };
    let input = h.scratch("cases_in");
    let output = h.scratch("cases_out");
    for (name, src, _) in CASES {
        std::fs::write(input.join(format!("{name}.glsl")), src).expect("write case");
    }
    let defines = h.dir.join("no_defines.txt");
    write_defines(&defines, &[]);
    h.run(&[Path::new("cases"), &input, &output, &defines], None);

    let mut problems = Vec::new();
    for (name, src, expect) in CASES {
        let m = MemorySources::new();
        let mut pp = Preprocessor::new(&m);
        let opts = PreprocessOptions {
            escape_reserved_words: false,
            ..Default::default()
        };
        let out = pp.preprocess_source("main.fsh", src, &opts);
        let base = output.join(name);
        let rejected = sibling(&base, "fail").exists();
        let diff = compare_glsl(&out, &base);
        match *expect {
            Same if rejected || diff.is_some() => {
                problems.push(format!("{name}: expected Iris's output, {}", diff.unwrap_or_default()));
            }
            Differs(why) if rejected || diff.is_none() => {
                problems.push(format!("{name}: documented deviation no longer differs ({why}); update the table"));
            }
            IrisRejects(_, why) if !rejected => {
                problems.push(format!("{name}: Iris accepts it now ({why}); update the table"));
            }
            IrisRejects(true, why) if !out.diagnostics.has_errors() => {
                problems.push(format!("{name}: Iris rejects it ({why}) but we report no error"));
            }
            _ => {}
        }
    }
    let _ = std::fs::remove_dir_all(&input);
    let _ = std::fs::remove_dir_all(&output);
    assert!(problems.is_empty(), "differences from Iris:\n{}", problems.join("\n"));
}

// ----------------------------------------------------------------------
// Corpus
// ----------------------------------------------------------------------

#[test]
fn corpus_programs_match_iris() {
    let Some(corpus) = corpus_root() else { return };
    let Some(h) = harness() else { return };
    let defines = h.dir.join("corpus_defines.txt");
    write_defines(&defines, CORPUS_DEFINES);
    let mut opts = corpus_options();
    opts.escape_reserved_words = false;
    let (mut total, mut per_pack) = (0usize, BTreeMap::new());
    let mut mismatches = Vec::new();
    for (pack, shaders) in shader_roots(&corpus) {
        let programs = program_files(&shaders);
        let out_dir = h.scratch(&format!("corpus_{}", pack.replace(['/', ' '], "_")));
        h.run(&[Path::new("glsl"), &shaders, &out_dir, &defines], Some(&programs.join("\n")));
        let sources = DirSources { root: shaders.clone() };
        let mut pp = Preprocessor::new(&sources);
        for program in &programs {
            let out = pp.preprocess(program, &opts);
            total += 1;
            *per_pack.entry(pack.clone()).or_insert(0usize) += 1;
            if let Some(d) = compare_glsl(&out, &out_dir.join(program)) {
                mismatches.push(format!("{pack} {program}: {d}"));
            }
        }
        let _ = std::fs::remove_dir_all(&out_dir);
    }
    println!("compared {total} programs with Iris/JCPP: {per_pack:?}");
    assert!(total > 100, "expected a real corpus");
    assert!(
        mismatches.is_empty(),
        "{} of {total} programs differ from Iris:\n{}",
        mismatches.len(),
        mismatches.iter().take(30).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn corpus_properties_match_iris() {
    let Some(corpus) = corpus_root() else { return };
    let Some(h) = harness() else { return };
    let defines_file = h.dir.join("props_defines.txt");
    write_defines(&defines_file, CORPUS_DEFINES);
    let defines: IndexMap<String, Option<String>> = CORPUS_DEFINES
        .iter()
        .map(|(k, v)| (k.to_string(), (!v.is_empty()).then(|| v.to_string())))
        .collect();
    let mut files: Vec<PathBuf> = Vec::new();
    for (_, shaders) in shader_roots(&corpus) {
        for e in walkdir::WalkDir::new(&shaders).max_depth(2).sort_by_file_name() {
            let Ok(e) = e else { continue };
            let p = e.path();
            if p.extension().is_some_and(|x| x == "properties")
                && !p.components().any(|c| c.as_os_str() == "lang")
            {
                files.push(p.to_path_buf());
            }
        }
    }
    let dimension = |p: &Path| p.file_name().is_some_and(|n| n == "dimension.properties");
    let list: String = files
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{i}|{}|{}\n", u8::from(dimension(p)), p.display()))
        .collect();
    let list_file = h.dir.join("props_list.txt");
    std::fs::write(&list_file, list).expect("write list");
    let out_dir = h.scratch("props_out");
    h.run(&[Path::new("props"), &list_file, &out_dir, &defines_file], None);
    let mut mismatches = Vec::new();
    for (i, p) in files.iter().enumerate() {
        // Iris reads .properties files as ISO-8859-1.
        let text: String = std::fs::read(p).expect("read").iter().map(|&b| b as char).collect();
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let (ours, _) = preprocess_properties(&text, &name, &defines);
        let theirs: String = std::fs::read(out_dir.join(format!("{i}.out")))
            .expect("iris output")
            .iter()
            .map(|&b| b as char)
            .collect();
        if ours != theirs {
            let line = ours.lines().zip(theirs.lines()).position(|(a, b)| a != b);
            mismatches.push(format!("{}: first differing line {line:?}", p.display()));
        }
    }
    let _ = std::fs::remove_dir_all(&out_dir);
    println!("compared {} .properties files with Iris", files.len());
    assert!(files.len() > 10);
    assert!(mismatches.is_empty(), "differences from Iris:\n{}", mismatches.join("\n"));
}
