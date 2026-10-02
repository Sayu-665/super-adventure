//! Line-oriented preprocessing-token lexer.
//!
//! Lexing works one (logical) line at a time with an explicit [`LexState`]
//! carried between lines, so block comments may span lines while every line
//! still has its own token list. That keeps the output 1:1 with input lines
//! and allows per-file token caching.

use crate::intern::{Interner, Sym, known};

/// Lexer state at a line boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LexState {
    /// Ordinary code.
    #[default]
    Normal,
    /// Inside a `/* ... */` comment that started on an earlier line.
    InComment,
}

/// Token kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Kind {
    /// Identifier (incl. keywords and `defined`).
    Ident,
    /// Preprocessing number (`1`, `0x1F`, `1.0e-5f`, `2u`, ...).
    Number,
    /// Punctuator (`+`, `<<=`, `#`, `##`, ...).
    Punct,
    /// String or character literal (possibly unterminated at end of line).
    Str,
    /// A run of horizontal whitespace.
    Space,
    /// A complete comment on this line (`// ...` or `/* ... */`).
    Comment,
    /// `/* ...` running to the end of the line (comment continues).
    CommentStart,
    /// A whole line inside a block comment.
    CommentCont,
    /// `... */` closing a comment started on an earlier line.
    CommentEnd,
    /// Any other character (stray `\`, `@`, `` ` ``, control characters, ...).
    Other,
    /// Placeholder produced by `##` with an empty operand (removed before output).
    Placemarker,
    /// Line break between physical lines pulled into a macro invocation. The
    /// token's `sym` holds the index of the physical line that follows.
    Newline,
}

impl Kind {
    /// Whitespace-like for macro and directive purposes.
    #[inline]
    pub(crate) fn is_white(self) -> bool {
        matches!(
            self,
            Kind::Space
                | Kind::Comment
                | Kind::CommentStart
                | Kind::CommentCont
                | Kind::CommentEnd
                | Kind::Newline
        )
    }
}

/// Token produced by macro expansion (used to avoid accidental token pasting in output).
pub(crate) const F_MACRO: u8 = 1;
/// Never expand this identifier again (set after an unterminated invocation
/// consumed the rest of the input, to keep error recovery linear).
pub(crate) const F_NOEXPAND: u8 = 2;

/// A preprocessing token. `Copy`, 12 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tok {
    pub sym: Sym,
    /// Hide-set id (see `macros::HideSets`); 0 = empty.
    pub hs: u32,
    pub kind: Kind,
    pub flags: u8,
}

impl Tok {
    #[inline]
    pub(crate) fn new(kind: Kind, sym: Sym) -> Self {
        Tok {
            sym,
            hs: 0,
            kind,
            flags: 0,
        }
    }
    #[inline]
    pub(crate) fn space() -> Self {
        Tok::new(Kind::Space, known::SPACE)
    }
    #[inline]
    pub(crate) fn is_punct(&self, sym: Sym) -> bool {
        self.kind == Kind::Punct && self.sym == sym
    }
}

const PUNCT3: [&str; 3] = ["<<=", ">>=", "..."];
const PUNCT2: [&str; 21] = [
    "##", "++", "--", "->", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "^^", "+=", "-=", "*=",
    "/=", "%=", "&=", "|=", "^=",
];
const PUNCT1: &[u8] = b"{}[]()#;:,.?~!+-*/%<>=&|^";

#[inline]
fn is_ident_start_ascii(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

#[inline]
fn is_ident_cont_ascii(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

#[inline]
fn is_hspace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | 0x0B | 0x0C | b'\r')
}

/// Length in bytes of the identifier starting at `i` (which must be an identifier start).
fn ident_len(s: &str, i: usize) -> usize {
    let mut j = i;
    let b = s.as_bytes();
    while j < b.len() {
        let c = b[j];
        if c < 0x80 {
            if is_ident_cont_ascii(c) {
                j += 1;
            } else {
                break;
            }
        } else {
            match s[j..].chars().next() {
                Some(ch) if ch.is_alphanumeric() => j += ch.len_utf8(),
                _ => break,
            }
        }
    }
    j - i
}

/// Length of the preprocessing number starting at `i`.
fn number_len(b: &[u8], i: usize) -> usize {
    let hex = b.len() > i + 1 && b[i] == b'0' && (b[i + 1] == b'x' || b[i + 1] == b'X');
    let mut j = i + 1;
    while j < b.len() {
        let c = b[j];
        if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' {
            j += 1;
            let exp = if hex {
                c == b'p' || c == b'P'
            } else {
                c == b'e' || c == b'E'
            };
            if exp && j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                j += 1;
            }
        } else {
            break;
        }
    }
    j - i
}

/// Length of the string/char literal starting at `i` (quote at `i`); runs to
/// the end of the line when unterminated.
fn string_len(b: &[u8], i: usize) -> usize {
    let q = b[i];
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            c if c == q => return j + 1 - i,
            _ => j += 1,
        }
    }
    b.len() - i
}

fn punct_len(b: &[u8], i: usize) -> usize {
    let rest = &b[i..];
    if rest.len() >= 3 && PUNCT3.iter().any(|p| rest.starts_with(p.as_bytes())) {
        return 3;
    }
    if rest.len() >= 2 && PUNCT2.iter().any(|p| rest.starts_with(p.as_bytes())) {
        return 2;
    }
    if PUNCT1.contains(&rest[0]) { 1 } else { 0 }
}

fn find_comment_end(b: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    while j + 1 < b.len() {
        if b[j] == b'*' && b[j + 1] == b'/' {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// Lex one logical line (already spliced, no `\n`) starting in `state`;
/// appends tokens to `out` and returns the state at the end of the line.
pub(crate) fn lex_line(
    text: &str,
    state: LexState,
    int: &mut Interner,
    out: &mut Vec<Tok>,
) -> LexState {
    let b = text.as_bytes();
    let mut i = 0usize;
    if state == LexState::InComment {
        match find_comment_end(b, 0) {
            Some(j) => {
                out.push(Tok::new(Kind::CommentEnd, int.intern(&text[..j + 2])));
                i = j + 2;
            }
            None => {
                if !text.is_empty() {
                    out.push(Tok::new(Kind::CommentCont, int.intern(text)));
                }
                return LexState::InComment;
            }
        }
    }
    while i < b.len() {
        let c = b[i];
        let start = i;
        let kind;
        if is_hspace(c) {
            while i < b.len() && is_hspace(b[i]) {
                i += 1;
            }
            kind = Kind::Space;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            out.push(Tok::new(Kind::Comment, int.intern(&text[i..])));
            return LexState::Normal;
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            match find_comment_end(b, i + 2) {
                Some(j) => {
                    i = j + 2;
                    kind = Kind::Comment;
                }
                None => {
                    out.push(Tok::new(Kind::CommentStart, int.intern(&text[i..])));
                    return LexState::InComment;
                }
            }
        } else if c == b'"' || c == b'\'' {
            i += string_len(b, i);
            kind = Kind::Str;
        } else if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            i += number_len(b, i);
            kind = Kind::Number;
        } else if is_ident_start_ascii(c) {
            i += ident_len(text, i);
            kind = Kind::Ident;
        } else if c >= 0x80 {
            let ch = text[i..].chars().next().unwrap_or('\u{FFFD}');
            if ch.is_alphabetic() {
                i += ident_len(text, i).max(ch.len_utf8());
                kind = Kind::Ident;
            } else if ch.is_whitespace() {
                i += ch.len_utf8();
                kind = Kind::Space;
            } else {
                i += ch.len_utf8();
                kind = Kind::Other;
            }
        } else {
            let n = punct_len(b, i);
            if n > 0 {
                i += n;
                kind = Kind::Punct;
            } else {
                i += 1;
                kind = Kind::Other;
            }
        }
        out.push(Tok::new(kind, int.intern(&text[start..i])));
    }
    LexState::Normal
}

/// Lex a complete string that must form exactly one token (used for `##`).
/// Returns `None` when the text is empty or lexes to several tokens.
pub(crate) fn lex_single(text: &str, int: &mut Interner) -> Option<Tok> {
    let mut v = Vec::with_capacity(2);
    let st = lex_line(text, LexState::Normal, int, &mut v);
    if st != LexState::Normal || v.len() != 1 {
        return None;
    }
    let t = v[0];
    match t.kind {
        Kind::Ident | Kind::Number | Kind::Punct | Kind::Str | Kind::Other => Some(t),
        _ => None,
    }
}

/// Whether emitting `next` directly after `prev` (no whitespace) could lex as
/// a different token sequence. Conservative.
pub(crate) fn would_paste(prev: &str, prev_kind: Kind, next: &str, next_kind: Kind) -> bool {
    let (Some(a), Some(z)) = (prev.chars().last(), next.chars().next()) else {
        return false;
    };
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    match (prev_kind, next_kind) {
        (Kind::Ident | Kind::Number, Kind::Ident | Kind::Number) => true,
        (Kind::Number, Kind::Punct) => {
            z == '.' || ((z == '+' || z == '-') && matches!(a, 'e' | 'E' | 'p' | 'P'))
        }
        (Kind::Punct, Kind::Number) => a == '.',
        (Kind::Ident | Kind::Number, Kind::Str) | (Kind::Str, Kind::Ident | Kind::Number) => {
            word(a) && word(z)
        }
        (Kind::Punct, Kind::Punct) => matches!(
            (a, z),
            ('+', '+' | '=')
                | ('-', '-' | '=' | '>')
                | ('<', '<' | '=')
                | ('>', '>' | '=')
                | ('=', '=')
                | ('!', '=')
                | ('&', '&' | '=')
                | ('|', '|' | '=')
                | ('^', '^' | '=')
                | ('*', '=' | '/')
                | ('/', '=' | '/' | '*')
                | ('%', '=')
                | ('#', '#')
                | ('.', '.')
        ),
        (Kind::Punct, Kind::Other) | (Kind::Other, Kind::Punct) | (Kind::Other, Kind::Other) => {
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(s: &str) -> Vec<(Kind, String)> {
        let mut int = Interner::new();
        let mut v = Vec::new();
        lex_line(s, LexState::Normal, &mut int, &mut v);
        v.iter()
            .map(|t| (t.kind, int.get(t.sym).to_string()))
            .collect()
    }

    fn texts(s: &str) -> Vec<String> {
        lex(s)
            .into_iter()
            .filter(|(k, _)| *k != Kind::Space)
            .map(|(_, t)| t)
            .collect()
    }

    #[test]
    fn identifiers_and_numbers() {
        assert_eq!(texts("foo _bar $x b2"), ["foo", "_bar", "$x", "b2"]);
        assert_eq!(
            texts("1 1.5 .5 1e-5 1.0f 0x1Fu 2u 1e+3x"),
            ["1", "1.5", ".5", "1e-5", "1.0f", "0x1Fu", "2u", "1e+3x"]
        );
        // A hex number does not swallow `+` after an `e` digit.
        assert_eq!(texts("0x1e+1"), ["0x1e", "+", "1"]);
        assert_eq!(texts("0x1p-3"), ["0x1p-3"]);
        // pp-number swallows identifier characters.
        assert_eq!(texts("2FOO"), ["2FOO"]);
        assert_eq!(texts("éclair x"), ["éclair", "x"]);
    }

    #[test]
    fn punctuators_longest_match() {
        assert_eq!(
            texts("a<<=b>>=c...d##e^^f"),
            [
                "a", "<<=", "b", ">>=", "c", "...", "d", "##", "e", "^^", "f"
            ]
        );
        assert_eq!(texts("a+++b"), ["a", "++", "+", "b"]);
        assert_eq!(texts("x->y"), ["x", "->", "y"]);
        assert_eq!(texts("\\ @ `"), ["\\", "@", "`"]);
    }

    #[test]
    fn strings_and_chars() {
        assert_eq!(
            texts(r#"a "b \" c" 'x' d"#),
            ["a", r#""b \" c""#, "'x'", "d"]
        );
        // Unterminated literal runs to end of line.
        assert_eq!(texts("don't stop"), ["don", "'t stop"]);
    }

    #[test]
    fn comments() {
        assert_eq!(
            lex("a // c d"),
            [
                (Kind::Ident, "a".into()),
                (Kind::Space, " ".into()),
                (Kind::Comment, "// c d".into())
            ]
        );
        assert_eq!(texts("a /* b */ c"), ["a", "/* b */", "c"]);
        let mut int = Interner::new();
        let mut v = Vec::new();
        assert_eq!(
            lex_line("x /* open", LexState::Normal, &mut int, &mut v),
            LexState::InComment
        );
        assert_eq!(v.last().map(|t| t.kind), Some(Kind::CommentStart));
        v.clear();
        assert_eq!(
            lex_line("still inside", LexState::InComment, &mut int, &mut v),
            LexState::InComment
        );
        assert_eq!(v[0].kind, Kind::CommentCont);
        v.clear();
        assert_eq!(
            lex_line(" end */ y", LexState::InComment, &mut int, &mut v),
            LexState::Normal
        );
        assert_eq!(v[0].kind, Kind::CommentEnd);
        assert_eq!(int.get(v[0].sym), " end */");
        assert_eq!(int.get(v.last().map(|t| t.sym).unwrap_or_default()), "y");
        // `/*/` does not close itself.
        v.clear();
        assert_eq!(
            lex_line("/*/ x", LexState::Normal, &mut int, &mut v),
            LexState::InComment
        );
        v.clear();
        assert_eq!(
            lex_line("", LexState::InComment, &mut int, &mut v),
            LexState::InComment
        );
        assert!(v.is_empty());
    }

    #[test]
    fn single_token_lexing() {
        let mut int = Interner::new();
        assert!(lex_single("foo", &mut int).is_some());
        assert!(lex_single("+=", &mut int).is_some());
        assert!(lex_single("+-", &mut int).is_none());
        assert!(lex_single("", &mut int).is_none());
        assert!(lex_single("/*", &mut int).is_none());
    }

    #[test]
    fn paste_avoidance() {
        assert!(!would_paste("-", Kind::Punct, "1", Kind::Number));
        assert!(would_paste("-", Kind::Punct, "-", Kind::Punct));
        assert!(would_paste("a", Kind::Ident, "b", Kind::Ident));
        assert!(would_paste("1e", Kind::Number, "+", Kind::Punct));
        assert!(would_paste("/", Kind::Punct, "*", Kind::Punct));
        assert!(!would_paste("(", Kind::Punct, "a", Kind::Ident));
        assert!(!would_paste("a", Kind::Ident, ")", Kind::Punct));
    }
}
