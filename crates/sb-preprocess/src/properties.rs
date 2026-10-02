//! `.properties` preprocessing, mirroring Iris's `PropertiesPreprocessor`
//! (Iris 1.11, `shaderpack/preprocessor/PropertiesPreprocessor.java`).

use indexmap::IndexMap;
use sb_core::{Diagnostic, Diagnostics};

use crate::engine::{Engine, EngineConfig, PhysLine};
use crate::intern::Interner;
use crate::source::{FileData, java_trim};

/// Marker Iris substitutes for backslashes during properties preprocessing.
pub(crate) const BACKSLASH_MARKER: &str = "IRIS_PASSTHROUGHBACKSLASH";

/// JCPP `PreprocessorCommand` names; a `#` line is a directive if it starts with `#` + one of these.
const DIRECTIVE_PREFIXES: &[&str] = &[
    "define",
    "elif",
    "else",
    "endif",
    "error",
    "if",
    "ifdef",
    "ifndef",
    "include",
    "line",
    "pragma",
    "undef",
    "warning",
    "include_next",
    "import",
];

/// Split on Java's `\R` (`\r\n` or any of `\n \x0B \x0C \r \u{85} \u{2028} \u{2029}`).
fn split_java_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let brk = matches!(
            c,
            '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
        );
        if brk {
            out.push(&text[start..i]);
            let mut next = i + c.len_utf8();
            if c == '\r' && it.peek().is_some_and(|&(_, d)| d == '\n') {
                it.next();
                next += 1;
            }
            start = next;
        }
    }
    out.push(&text[start..]);
    out
}

/// Java's `Character.isWhitespace` (used by `String.isBlank`).
fn java_is_whitespace(c: char) -> bool {
    matches!(c, '\u{1C}'..='\u{1F}')
        || (c.is_whitespace() && !matches!(c, '\u{85}' | '\u{A0}' | '\u{2007}' | '\u{202F}'))
}

/// Apply Iris's line filter. Returns the joined text and the original
/// 1-based line number of every output line.
pub(crate) fn iris_prefilter(text: &str) -> (String, Vec<u32>) {
    let mut joined = String::with_capacity(text.len());
    let mut numbers = Vec::new();
    for (idx, raw) in split_java_lines(text).into_iter().enumerate() {
        let t = java_trim(raw);
        if t.chars().all(java_is_whitespace) {
            continue;
        }
        if let Some(after) = t.strip_prefix('#') {
            if DIRECTIVE_PREFIXES.iter().any(|d| after.starts_with(d)) {
                joined.push_str(t);
            }
        } else if t.contains('#') {
            joined.push_str(&t.replace('#', ""));
        } else {
            joined.push_str(t);
        }
        joined.push('\n');
        numbers.push(u32::try_from(idx + 1).unwrap_or(u32::MAX));
    }
    (joined, numbers)
}

/// Preprocess a shader pack `.properties` file (`shaders.properties`,
/// `block.properties`, `item.properties`, `entity.properties`,
/// `dimension.properties`, ...) exactly like Iris does.
///
/// Iris runs JCPP over a pre-filtered copy of the file:
///
/// 1. The text is split into lines on any Java `\R` line break, every line is
///    trimmed (Java `String.trim()`), and blank lines are **removed**.
/// 2. A line starting with `#` is kept only if it starts with `#` immediately
///    followed by a JCPP directive name (`define`, `elif`, `else`, `endif`,
///    `error`, `if`, `ifdef`, `ifndef`, `include`, `line`, `pragma`, `undef`,
///    `warning`, `include_next`, `import`; a *prefix* match, so `#iffy` is kept
///    and then dropped by the preprocessor as an unknown directive). Every other
///    `#` line is a comment and becomes an empty line. `# if` (with a space) is
///    therefore a comment.
/// 3. On all other lines every `#` character is deleted.
/// 4. Backslashes are replaced by the identifier `IRIS_PASSTHROUGHBACKSLASH`
///    before preprocessing and restored afterwards. Consequently there is no
///    `\`-newline splicing (property-value continuations survive for the
///    Java properties parser), and an identifier written directly before a
///    backslash (`FOO\`) merges with the marker and is **not** macro-expanded.
/// 5. JCPP then evaluates conditionals and **macro-expands ordinary lines**
///    (identifiers outside string/char literals and comments; comments are
///    kept). This crate mirrors that: ordinary lines are macro-expanded. No
///    spaces are inserted between adjacent expanded tokens (JCPP does not).
/// 6. Macros: for `shaders.properties`, `block.properties`, `item.properties`
///    and `entity.properties` Iris defines boolean options and environment
///    defines with an empty value as `1` (JCPP `addMacro(name)`); for
///    `dimension.properties` (environment defines only) empty values stay
///    empty. `preprocess_properties` selects the variant from the file name.
///
/// The output has one line per non-blank input line (directive, inactive and
/// comment lines are empty), exactly like Iris's output.
///
/// `text` is the file decoded as ISO-8859-1 (Iris reads `.properties` files
/// with that charset). `file_name` is used for diagnostics and to pick the macro variant
/// (`dimension.properties` keeps empty define values empty, every other file
/// defines them as `1`). `defines` are the environment macros and the current
/// option values (boolean options that are on map to `None`).
///
/// ```
/// use indexmap::IndexMap;
/// use sb_preprocess::preprocess_properties;
/// let mut defines = IndexMap::new();
/// defines.insert("BLOOM".to_string(), None);
/// defines.insert("BLOOM_RES".to_string(), Some("0.5".to_string()));
/// let src = "# a comment\n#ifdef BLOOM\nprogram.composite.enabled = true\nscale.composite = BLOOM_RES\n#endif\n";
/// let (out, diags) = preprocess_properties(src, "shaders.properties", &defines);
/// assert!(diags.is_empty());
/// assert_eq!(out, "\n\nprogram.composite.enabled = true\nscale.composite = 0.5\n\n");
/// ```
pub fn preprocess_properties(
    text: &str,
    file_name: &str,
    defines: &IndexMap<String, Option<String>>,
) -> (String, Diagnostics) {
    let mut diags = Diagnostics::new();
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    if text.contains(BACKSLASH_MARKER) || text.contains("#warning IRIS_PASSTHROUGH ") {
        diags.push(Diagnostic::error(
            "pp.reserved-marker",
            format!("{file_name} contains an Iris-internal preprocessor marker; Iris refuses to load such files"),
        ));
    }
    let (joined, numbers) = iris_prefilter(text);
    let joined = joined.replace('\0', "").replace('\\', BACKSLASH_MARKER);

    let mut int = Interner::new();
    let mut fd = FileData::new(file_name.to_string(), joined, &mut int, false);
    fd.line_numbers = Some(numbers);
    let files = [fd];
    let lines: Vec<PhysLine> = (0..files[0].line_count() as u32)
        .map(|line| PhysLine {
            file: 0,
            line,
            frame: 0,
        })
        .collect();
    let cfg = EngineConfig {
        keep_comments: true,
        escape: false,
        properties: true,
        avoid_paste: false,
    };
    let mut engine = Engine::new(&files, &mut int, &lines, 1, cfg);
    engine.define_builtins(false);
    let env_only = file_name
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|n| n.eq_ignore_ascii_case("dimension.properties"));
    for (name, value) in defines {
        let v = match value.as_deref() {
            None | Some("") if !env_only => "1",
            None => "",
            Some(v) => v,
        };
        engine.define_user(name, v);
    }
    let out = engine.run();
    diags.extend(out.diagnostics);
    (out.code.replace(BACKSLASH_MARKER, "\\"), diags)
}

/// Post-processing Iris applies to `block.properties`, `item.properties` and
/// `entity.properties` after [`preprocess_properties`] (Iris `IdMap.loadProperties`):
/// `replaceAll("\\\\\\n\\s*\\n", " ")` followed by `replaceAll("\\S *block\\.", "\nblock.")`.
///
/// The first rule joins a line continuation that is followed by a blank line
/// (left behind by removed directives); the second forces every `block.`
/// key onto its own line. Note that, exactly like Iris, the second rule
/// consumes the non-space character before the spaces (`a b  block.1` becomes
/// `a \nblock.1`). Use this only to reproduce Iris's id-map parsing.
pub fn iris_idmap_fixups(text: &str) -> String {
    // Java regex `\s` without UNICODE_CHARACTER_CLASS.
    let java_ws = |c: char| matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r');

    // Rule 1: `\` `\n` `\s*` `\n` -> " " (greedy `\s*` backtracks to the last newline of the run).
    let mut s1 = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while let Some(j) = text[i..].find("\\\n").map(|j| j + i) {
        let run_start = j + 2;
        let run_len = text[run_start..]
            .find(|c: char| !java_ws(c))
            .unwrap_or(text.len() - run_start);
        match text[run_start..run_start + run_len].rfind('\n') {
            Some(nl) => {
                s1.push_str(&text[copied..j]);
                s1.push(' ');
                copied = run_start + nl + 1;
                i = copied;
            }
            None => i = j + 1,
        }
    }
    s1.push_str(&text[copied..]);

    // Rule 2: `\S *block\.` -> "\nblock." (the `\S` character is consumed, as in Iris).
    let mut out = String::with_capacity(s1.len() + 16);
    let mut copied = 0;
    let mut i = 0;
    while let Some(c) = s1[i..].chars().next() {
        let next = i + c.len_utf8();
        if !java_ws(c) {
            let spaces = s1[next..].bytes().take_while(|&b| b == b' ').count();
            let key = next + spaces;
            if s1[key..].starts_with("block.") {
                out.push_str(&s1[copied..i]);
                out.push_str("\nblock.");
                i = key + "block.".len();
                copied = i;
                continue;
            }
        }
        i = next;
    }
    out.push_str(&s1[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs(list: &[(&str, Option<&str>)]) -> IndexMap<String, Option<String>> {
        list.iter()
            .map(|(k, v)| (k.to_string(), v.map(str::to_string)))
            .collect()
    }

    fn pp(src: &str, d: &[(&str, Option<&str>)]) -> String {
        let (out, diags) = preprocess_properties(src, "shaders.properties", &defs(d));
        assert!(!diags.has_errors(), "{diags:?}");
        out
    }

    #[test]
    fn java_line_splitting() {
        assert_eq!(
            split_java_lines("a\r\nb\rc\nd\u{2028}e"),
            ["a", "b", "c", "d", "e"]
        );
        assert_eq!(split_java_lines("x\n"), ["x", ""]);
    }

    #[test]
    fn java_blank_semantics() {
        // A line holding only a no-break space is not blank for Java.
        let (t, _) = iris_prefilter("a\n\u{A0}\n\u{2003}\nb\n");
        assert_eq!(t, "a\n\u{A0}\nb\n");
    }

    #[test]
    fn prefilter_rules() {
        let (t, n) = iris_prefilter(
            "  a=1  \n\n# comment\n#ifdef X\n# if Y\nb=2 # trailing\n#iffy\n\t\n#define Z\n",
        );
        assert_eq!(t, "a=1\n\n#ifdef X\n\nb=2  trailing\n#iffy\n#define Z\n");
        assert_eq!(n, [1, 3, 4, 5, 6, 7, 9]);
    }

    #[test]
    fn conditionals_and_comments() {
        let src = "#if MC_VERSION >= 11700\nnew=1\n#else\nold=1\n#endif\n# just a comment\nx=y\n";
        assert_eq!(
            pp(src, &[("MC_VERSION", Some("260300"))]),
            "\nnew=1\n\n\n\n\nx=y\n"
        );
        assert_eq!(
            pp(src, &[("MC_VERSION", Some("11202"))]),
            "\n\n\nold=1\n\n\nx=y\n"
        );
    }

    #[test]
    fn ordinary_lines_are_macro_expanded() {
        let src =
            "#define RES 2048\nshadowMapResolution=RES\nsize.buffer.colortex4 = SCALE SCALE\n";
        assert_eq!(
            pp(src, &[("SCALE", Some("0.5"))]),
            "\nshadowMapResolution=2048\nsize.buffer.colortex4 = 0.5 0.5\n"
        );
        // Boolean (empty) defines are 1 in shaders.properties ...
        assert_eq!(pp("v=FLAG\n", &[("FLAG", None)]), "v=1\n");
        // ... but stay empty in dimension.properties.
        let (out, _) =
            preprocess_properties("v=FLAG\n", "dimension.properties", &defs(&[("FLAG", None)]));
        assert_eq!(out, "v=\n");
        // No expansion inside string literals or comments.
        assert_eq!(
            pp("a=\"FLAG\" // FLAG\n", &[("FLAG", Some("x"))]),
            "a=\"FLAG\" // FLAG\n"
        );
    }

    #[test]
    fn backslashes_pass_through() {
        let src = "screen.MAIN = A \\\n   B \\\n   C\n";
        assert_eq!(
            pp(src, &[("B", Some("bee"))]),
            "screen.MAIN = A \\\nbee \\\nC\n"
        );
        // An identifier glued to the backslash is not expanded (it merges with the marker).
        assert_eq!(pp("x = B\\\n", &[("B", Some("bee"))]), "x = B\\\n");
        // No splicing: a continued #define ends at the backslash.
        assert_eq!(pp("#define A 1 \\\nA\n", &[]), "\n1 \\\n");
    }

    #[test]
    fn hash_in_values_is_removed_and_directive_spacing() {
        assert_eq!(pp("color=#FF0000\n", &[]), "color=FF0000\n");
        // `# ifdef` with a space is a comment, so the block is NOT conditional.
        assert_eq!(pp("# ifdef NOPE\na=1\n# endif\n", &[]), "\na=1\n\n");
    }

    #[test]
    fn unknown_directive_prefix_lines_are_dropped_silently() {
        let (out, diags) = preprocess_properties(
            "#iffy stuff\n#defines\na=1\n",
            "block.properties",
            &IndexMap::new(),
        );
        assert_eq!(out, "\n\na=1\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn error_and_unterminated() {
        let (_, diags) = preprocess_properties("#error boom\n", "x.properties", &IndexMap::new());
        assert!(
            diags
                .iter()
                .any(|d| d.code == "pp.error" && d.message.contains("boom"))
        );
        let (_, diags) =
            preprocess_properties("a=1\n\n#ifdef X\nb=2\n", "x.properties", &IndexMap::new());
        let d = diags
            .iter()
            .find(|d| d.code == "pp.unterminated-conditional")
            .expect("diag");
        // Original line number (blank line removed before preprocessing).
        assert_eq!(d.location.as_ref().map(|l| l.line), Some(3));
        let (_, diags) = preprocess_properties(
            "x=IRIS_PASSTHROUGHBACKSLASH\n",
            "x.properties",
            &IndexMap::new(),
        );
        assert!(diags.iter().any(|d| d.code == "pp.reserved-marker"));
    }

    #[test]
    fn function_like_macros_and_no_paste_avoidance() {
        let src = "#define NEG(x) -x\nv=-NEG(1)\n";
        // JCPP does not insert spaces between tokens.
        assert_eq!(pp(src, &[]), "\nv=--1\n");
    }

    #[test]
    fn idmap_fixups() {
        assert_eq!(
            iris_idmap_fixups("block.1=a b \\\n\n\nblock.2=c\n"),
            "block.1=a \nblock.2=c\n"
        );
        assert_eq!(
            iris_idmap_fixups("block.1=a \\\n  b\n"),
            "block.1=a \\\n  b\n"
        );
        assert_eq!(iris_idmap_fixups("x\nblock.2=c\n"), "x\nblock.2=c\n");
    }
}
