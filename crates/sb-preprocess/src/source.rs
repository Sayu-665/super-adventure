//! Loaded pack files: text normalization, line indexing, `#include` line
//! detection (Iris semantics) and the per-file token cache.

use crate::intern::Interner;
use crate::lexer::{LexState, Tok, lex_line};

/// Characters other than `\n` that end a line for Iris: its include graph
/// splits every source file with Java's `\R` (`\r\n` or any of
/// `\n \x0B \x0C \r \u{85} \u{2028} \u{2029}`) and re-joins the lines with `\n`.
const JAVA_LINE_BREAKS: [char; 6] = ['\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}'];

/// Normalize raw file text: strip a UTF-8 BOM, drop NUL characters (Iris does
/// the same, some packs contain stray NULs) and convert every Java `\R` line
/// break (CRLF, lone CR, vertical tab, form feed, NEL, U+2028, U+2029) to LF,
/// exactly as Iris's line splitting does before preprocessing.
pub(crate) fn normalize_text(src: &str) -> String {
    let s = src.strip_prefix('\u{FEFF}').unwrap_or(src);
    if !s.contains(|c: char| c == '\0' || JAVA_LINE_BREAKS.contains(&c)) {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\0' => {}
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            c if JAVA_LINE_BREAKS.contains(&c) => out.push('\n'),
            c => out.push(c),
        }
    }
    out
}

/// Java's `String.trim()`: strips every char `<= ' '` from both ends.
pub(crate) fn java_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// Result of parsing an `#include` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IncludeTarget {
    /// Normalized pack path of the included file.
    Path(String),
    /// The path climbs above the `shaders/` root with `..`, which
    /// [`sb_core::normalize_pack_path`] rejects. Iris (`AbsolutePackPath`)
    /// silently clamps such `..` segments at the root; this is that clamped
    /// path (included, with a warning).
    Clamped(String),
    /// The line is an include but the target is malformed.
    Invalid(String),
}

/// Iris's path resolution (`AbsolutePackPath.resolve`): like
/// [`sb_core::normalize_pack_path`] but a `..` at the root is ignored instead
/// of failing. `None` when the result is the root itself.
fn iris_clamped_path(including_file: &str, target: &str) -> Option<String> {
    let target = target.replace('\\', "/");
    let base = including_file.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    if !target.starts_with('/') {
        let dir = base.rfind('/').map_or("", |i| &base[..i]);
        parts.extend(dir.split('/').filter(|s| !s.is_empty() && *s != "."));
    }
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// One `#include` line of a file.
#[derive(Debug, Clone)]
pub(crate) struct IncludeLine {
    /// 0-based line index.
    pub line: u32,
    pub target: IncludeTarget,
}

/// Parse an Iris-style include line: any line whose trimmed text starts with
/// `#include` (Iris `FileNode.findIncludes`). Returns `None` for other lines.
///
/// The target is `"path"`, `<path>` or a bare path. Lines such as
/// `#include_next "x"` are include lines for Iris too, which then fails to
/// resolve them; they are reported as malformed.
pub(crate) fn parse_include_line(including_file: &str, line: &str) -> Option<IncludeTarget> {
    let t = java_trim(line);
    let rest = t.strip_prefix("#include")?;
    match rest.chars().next() {
        Some(c) if c <= ' ' || c == '"' || c == '<' => {}
        // `#include` alone: an include with no target.
        None => {
            return Some(IncludeTarget::Invalid(
                "#include without a file name".into(),
            ));
        }
        // `#include_next`, `#includes`, ...: Iris treats every line starting with
        // `#include` as an include and cannot resolve these.
        Some(_) => {
            let word = t.split(|c: char| c <= ' ' || c == '"' || c == '<').next().unwrap_or(t);
            return Some(IncludeTarget::Invalid(format!(
                "'{word}' is not supported: Iris treats every line starting with '#include' as an #include and fails to resolve this one"
            )));
        }
    }
    let rest = java_trim(rest);
    let target = if let Some(r) = rest.strip_prefix('"') {
        match r.find('"') {
            Some(j) => &r[..j],
            None => java_trim(r),
        }
    } else if let Some(r) = rest.strip_prefix('<') {
        match r.find('>') {
            Some(j) => &r[..j],
            None => java_trim(r),
        }
    } else {
        let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
        rest[..end].trim_end_matches('"')
    };
    let target = target.trim();
    if target.is_empty() {
        return Some(IncludeTarget::Invalid(
            "#include without a file name".into(),
        ));
    }
    Some(match sb_core::normalize_pack_path(including_file, target) {
        Some(p) => IncludeTarget::Path(p),
        None => match iris_clamped_path(including_file, target) {
            Some(p) => IncludeTarget::Clamped(p),
            None => IncludeTarget::Invalid(format!(
                "include path \"{target}\" resolves to the shader pack root, not a file"
            )),
        },
    })
}

/// Cached lexing of one physical line, assuming the file is lexed on its own
/// from its first line ("natural" lexing).
#[derive(Debug, Clone, Copy)]
pub(crate) struct LineLex {
    pub tok_start: u32,
    pub tok_end: u32,
    pub start: LexState,
    pub end: LexState,
    /// Number of physical lines joined into this logical line by `\`-newline
    /// splices (>= 1), or 0 for a line that continues the previous one.
    pub span: u32,
}

/// A loaded source file.
#[derive(Debug)]
pub(crate) struct FileData {
    /// Normalized pack path (or a virtual name).
    pub path: String,
    pub text: String,
    /// Byte offset of the start of each line, plus a final sentinel.
    line_starts: Vec<u32>,
    pub includes: Vec<IncludeLine>,
    pub toks: Vec<Tok>,
    pub lex: Vec<LineLex>,
    /// Optional original line numbers (1-based) per line, used by the
    /// properties mode where blank lines were removed before preprocessing.
    pub line_numbers: Option<Vec<u32>>,
}

impl FileData {
    /// Build a file from already normalized text.
    pub(crate) fn new(
        path: String,
        text: String,
        int: &mut Interner,
        detect_includes: bool,
    ) -> Self {
        let mut line_starts = Vec::with_capacity(text.len() / 32 + 2);
        if !text.is_empty() {
            line_starts.push(0u32);
            for (i, b) in text.bytes().enumerate() {
                if b == b'\n' && i + 1 < text.len() {
                    line_starts.push(u32::try_from(i + 1).unwrap_or(u32::MAX));
                }
            }
        }
        // Sentinel: one past the terminator of the last line (which may be missing).
        let sentinel = if text.ends_with('\n') {
            text.len()
        } else {
            text.len() + 1
        };
        line_starts.push(u32::try_from(sentinel).unwrap_or(u32::MAX));
        let mut f = FileData {
            path,
            text,
            line_starts,
            includes: Vec::new(),
            toks: Vec::new(),
            lex: Vec::new(),
            line_numbers: None,
        };
        if detect_includes {
            let mut includes = Vec::new();
            for i in 0..f.line_count() {
                let line = f.line(i);
                if line.contains("#include")
                    && let Some(target) = parse_include_line(&f.path, line)
                {
                    includes.push(IncludeLine {
                        line: i as u32,
                        target,
                    });
                }
            }
            f.includes = includes;
        }
        f.lex_all(int);
        f
    }

    /// Number of lines (like `str::lines().count()`).
    pub(crate) fn line_count(&self) -> usize {
        self.line_starts.len() - 1
    }

    /// Text of line `i` without the line terminator.
    pub(crate) fn line(&self, i: usize) -> &str {
        let (Some(&s), Some(&e)) = (self.line_starts.get(i), self.line_starts.get(i + 1)) else {
            return "";
        };
        let s = s as usize;
        let e = (e as usize).saturating_sub(1).min(self.text.len());
        self.text.get(s..e.max(s)).unwrap_or("")
    }

    /// Original 1-based line number of line `i`.
    pub(crate) fn original_line(&self, i: usize) -> u32 {
        match &self.line_numbers {
            Some(v) => v.get(i).copied().unwrap_or(i as u32 + 1),
            None => i as u32 + 1,
        }
    }

    /// Natural lexing of the whole file, line by line, joining `\`-newline splices.
    fn lex_all(&mut self, int: &mut Interner) {
        let n = self.line_count();
        let mut lex = Vec::with_capacity(n);
        let mut toks = Vec::with_capacity(self.text.len() / 3);
        let mut state = LexState::Normal;
        let mut k = 0;
        let mut joined = String::new();
        while k < n {
            let first = self.line(k);
            let start = toks.len() as u32;
            let st0 = state;
            let mut span = 1;
            if first.ends_with('\\') {
                joined.clear();
                let mut j = k;
                loop {
                    let l = self.line(j);
                    match l.strip_suffix('\\') {
                        Some(s) => {
                            joined.push_str(s);
                            if j + 1 >= n {
                                break;
                            }
                            j += 1;
                        }
                        None => {
                            joined.push_str(l);
                            break;
                        }
                    }
                }
                span = (j - k + 1) as u32;
                state = lex_line(&joined, state, int, &mut toks);
            } else {
                state = lex_line(first, state, int, &mut toks);
            }
            let end = toks.len() as u32;
            lex.push(LineLex {
                tok_start: start,
                tok_end: end,
                start: st0,
                end: state,
                span,
            });
            for _ in 1..span {
                lex.push(LineLex {
                    tok_start: end,
                    tok_end: end,
                    start: state,
                    end: state,
                    span: 0,
                });
            }
            k += span as usize;
        }
        self.lex = lex;
        self.toks = toks;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization() {
        assert_eq!(normalize_text("\u{FEFF}a\r\nb\rc\0d\n"), "a\nb\ncd\n");
        assert_eq!(normalize_text("plain\n"), "plain\n");
        assert_eq!(normalize_text("x\r"), "x\n");
    }

    /// Regression (review): Iris splits sources with Java's `\R`, so vertical
    /// tab, form feed, NEL and the Unicode line/paragraph separators end lines.
    #[test]
    fn java_line_breaks_end_lines() {
        assert_eq!(
            normalize_text("a\u{0B}b\u{0C}c\u{85}d\u{2028}e\u{2029}f\r\n"),
            "a\nb\nc\nd\ne\nf\n"
        );
        // A `//` comment ends at a form feed, like on Iris.
        assert_eq!(normalize_text("// c\u{0C}code"), "// c\ncode");
    }

    #[test]
    fn line_indexing() {
        let mut int = Interner::new();
        let f = FileData::new("a.glsl".into(), "one\ntwo\n\nfour".into(), &mut int, false);
        assert_eq!(f.line_count(), 4);
        assert_eq!(f.line(0), "one");
        assert_eq!(f.line(1), "two");
        assert_eq!(f.line(2), "");
        assert_eq!(f.line(3), "four");
        assert_eq!(f.line(4), "");
        let g = FileData::new("b".into(), "x\n".into(), &mut int, false);
        assert_eq!(g.line_count(), 1);
        let e = FileData::new("e".into(), String::new(), &mut int, false);
        assert_eq!(e.line_count(), 0);
        let blank = FileData::new("e".into(), "\n".into(), &mut int, false);
        assert_eq!(blank.line_count(), 1);
        assert_eq!(blank.line(0), "");
    }

    #[test]
    fn include_lines() {
        let p = |l: &str| parse_include_line("world0/composite.fsh", l);
        assert_eq!(
            p("#include \"/lib/a.glsl\""),
            Some(IncludeTarget::Path("lib/a.glsl".into()))
        );
        assert_eq!(
            p("   #include \"lib/a.glsl\" // trailing"),
            Some(IncludeTarget::Path("world0/lib/a.glsl".into()))
        );
        assert_eq!(
            p("#include <lib/a.glsl>"),
            Some(IncludeTarget::Path("world0/lib/a.glsl".into()))
        );
        assert_eq!(
            p("#include\"../x.glsl\""),
            Some(IncludeTarget::Path("x.glsl".into()))
        );
        assert_eq!(
            p("#include bare.glsl"),
            Some(IncludeTarget::Path("world0/bare.glsl".into()))
        );
        assert_eq!(
            p("#include \"unterminated.glsl"),
            Some(IncludeTarget::Path("world0/unterminated.glsl".into()))
        );
        // `..` above the root is clamped like Iris's AbsolutePackPath.
        assert_eq!(
            p("#include \"../../x\""),
            Some(IncludeTarget::Clamped("x".into()))
        );
        assert!(matches!(p("#include"), Some(IncludeTarget::Invalid(_))));
        assert!(matches!(
            p("#include \"\""),
            Some(IncludeTarget::Invalid(_))
        ));
        assert!(matches!(p("#include \"..\""), Some(IncludeTarget::Invalid(_))));
        // Iris treats every line starting with `#include` as an include.
        assert!(matches!(
            p("#include_next \"x\""),
            Some(IncludeTarget::Invalid(m)) if m.contains("#include_next")
        ));
        assert_eq!(p("# include \"x\""), None);
        assert_eq!(p("// #include \"x\""), None);
        assert_eq!(p("int x;"), None);
    }

    /// Regression (review): Iris resolves `..` above the shaders root by
    /// clamping (AbsolutePackPath); sb-core's normalization rejects it.
    #[test]
    fn iris_path_clamping() {
        assert_eq!(iris_clamped_path("composite.fsh", "../lib/a.glsl").as_deref(), Some("lib/a.glsl"));
        assert_eq!(iris_clamped_path("world0/a.fsh", "../../../lib/a.glsl").as_deref(), Some("lib/a.glsl"));
        assert_eq!(iris_clamped_path("world0/a.fsh", "/../lib/./a.glsl").as_deref(), Some("lib/a.glsl"));
        assert_eq!(iris_clamped_path("a.fsh", "../.."), None);
        let p = |l: &str| parse_include_line("composite.fsh", l);
        assert_eq!(
            p("#include \"../lib/common.glsl\""),
            Some(IncludeTarget::Clamped("lib/common.glsl".into()))
        );
        assert_eq!(
            p("#include \"lib/../common.glsl\""),
            Some(IncludeTarget::Path("common.glsl".into()))
        );
    }

    #[test]
    fn natural_lexing_with_splices_and_comments() {
        let mut int = Interner::new();
        let f = FileData::new(
            "a".into(),
            "#define X(a) \\\n  a + \\\n  1\nint y; /* open\nstill\nclose */ z".into(),
            &mut int,
            false,
        );
        assert_eq!(f.lex.len(), 6);
        assert_eq!(f.lex[0].span, 3);
        assert_eq!(f.lex[1].span, 0);
        assert_eq!(f.lex[2].span, 0);
        assert_eq!(f.lex[3].end, LexState::InComment);
        assert_eq!(f.lex[4].start, LexState::InComment);
        assert_eq!(f.lex[5].start, LexState::InComment);
        assert_eq!(f.lex[5].end, LexState::Normal);
    }
}
