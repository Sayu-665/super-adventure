//! A faithful parser for the `java.util.Properties` text format, used by every pack
//! `.properties` file, `.lang` files and the per-pack settings file.
//!
//! Rules implemented (identical to `Properties.load(Reader)`):
//!
//! * Lines end with `\n`, `\r` or `\r\n`. Leading whitespace (space, tab, form feed)
//!   is skipped and blank lines are ignored.
//! * A line whose first non-whitespace character is `#` or `!` is a comment. Comment
//!   lines never continue onto the next line.
//! * A line ending in an odd number of backslashes continues on the next line; the
//!   leading whitespace of the continuation line is dropped. A blank line after a
//!   continuation ends the logical line. A continuation line starting with `#` is
//!   *not* a comment.
//! * The key ends at the first unescaped `=`, `:` or whitespace. Whitespace around the
//!   separator is skipped (one `=`/`:` may follow whitespace). The value is the rest of
//!   the logical line, trailing whitespace included.
//! * Escapes in keys and values: `\t`, `\n`, `\r`, `\f`, `\uXXXX`; any other escaped
//!   character stands for itself (so `\=`, `\:`, `\ `, `\#`, `\\` work).
//!
//! Duplicate keys: the value of the last occurrence wins, but the entry keeps the
//! position of the first occurrence (Iris's `OrderBackedProperties`).
//!
//! Malformed `\u` escapes (which make Java throw) are kept literally and reported by
//! [`parse_with_diagnostics`].

use indexmap::IndexMap;
use sb_core::{Diagnostic, Diagnostics, SourceLocation};
use serde::{Deserialize, Serialize};

/// One `key=value` entry. `line` is the 1-based line on which the entry starts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PropEntry {
    pub key: String,
    pub value: String,
    pub line: u32,
}

impl PropEntry {
    pub fn new(key: impl Into<String>, value: impl Into<String>, line: u32) -> Self {
        Self { key: key.into(), value: value.into(), line }
    }
}

/// Parse properties text. See the module docs for the exact syntax.
pub fn parse(text: &str) -> Vec<PropEntry> {
    parse_inner(text, &mut |_, _| {})
}

/// Parse properties text, reporting malformed `\uXXXX` escapes as warnings located in
/// `file`.
pub fn parse_with_diagnostics(text: &str, file: &str) -> (Vec<PropEntry>, Diagnostics) {
    let mut diags = Diagnostics::new();
    let entries = parse_inner(text, &mut |line, msg| {
        diags.push(Diagnostic::warning("props.bad-escape", msg).at(SourceLocation::new(file, line)));
    });
    (entries, diags)
}

/// Parse into an ordered map (key -> value).
pub fn parse_map(text: &str) -> IndexMap<String, String> {
    parse(text).into_iter().map(|e| (e.key, e.value)).collect()
}

fn parse_inner(text: &str, on_error: &mut dyn FnMut(u32, String)) -> Vec<PropEntry> {
    let mut map: IndexMap<String, (String, u32)> = IndexMap::new();
    for (line, logical) in logical_lines(text) {
        let (key, value) = split_key_value(&logical);
        let key = unescape(key, line, on_error);
        let value = unescape(value, line, on_error);
        match map.get_mut(&key) {
            Some(slot) => *slot = (value, line),
            None => {
                map.insert(key, (value, line));
            }
        }
    }
    map.into_iter().map(|(key, (value, line))| PropEntry { key, value, line }).collect()
}

fn is_ws(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\u{c}'
}

/// Join natural lines into logical lines (comments and blank lines removed, escapes
/// still present). Returns `(first line number, logical line)`.
pub(crate) fn logical_lines(text: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut line_no: u32 = 1;
    let mut buf = String::new();
    let mut start_line = 1;
    let mut skip_ws = true;
    let mut appended_line_begin = false;
    let mut preceding_backslash = false;

    while i < chars.len() {
        let c = chars[i];
        i += 1;
        let is_nl = c == '\n' || c == '\r';
        if is_nl {
            // Count a "\r\n" pair as one line break.
            if c == '\r' && chars.get(i) == Some(&'\n') {
                i += 1;
            }
        }

        if skip_ws {
            if is_ws(c) {
                continue;
            }
            if !appended_line_begin && is_nl {
                line_no += 1;
                continue;
            }
            skip_ws = false;
            appended_line_begin = false;
        }

        if buf.is_empty() && !is_nl {
            start_line = line_no;
            if c == '#' || c == '!' {
                // Comment: consume the rest of the natural line.
                while i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
                    i += 1;
                }
                skip_ws = true;
                continue;
            }
        }

        if !is_nl {
            buf.push(c);
            preceding_backslash = if c == '\\' { !preceding_backslash } else { false };
            continue;
        }

        // End of a natural line.
        line_no += 1;
        if buf.is_empty() {
            skip_ws = true;
            continue;
        }
        if preceding_backslash {
            buf.pop();
            skip_ws = true;
            appended_line_begin = true;
            preceding_backslash = false;
        } else {
            out.push((start_line, std::mem::take(&mut buf)));
            skip_ws = true;
        }
    }
    if !buf.is_empty() {
        if preceding_backslash {
            buf.pop();
        }
        out.push((start_line, buf));
    }
    out
}

/// Split a logical line into (raw key, raw value), still escaped.
fn split_key_value(line: &str) -> (&str, &str) {
    let bytes: Vec<(usize, char)> = line.char_indices().collect();
    let mut key_end = line.len();
    let mut value_start = line.len();
    let mut has_sep = false;
    let mut preceding_backslash = false;
    let mut idx = 0;
    while idx < bytes.len() {
        let (pos, c) = bytes[idx];
        if (c == '=' || c == ':') && !preceding_backslash {
            key_end = pos;
            value_start = pos + c.len_utf8();
            has_sep = true;
            break;
        } else if is_ws(c) && !preceding_backslash {
            key_end = pos;
            value_start = pos + c.len_utf8();
            break;
        }
        preceding_backslash = if c == '\\' { !preceding_backslash } else { false };
        idx += 1;
    }
    let rest = &line[value_start..];
    let mut offset = 0;
    for c in rest.chars() {
        if !is_ws(c) {
            if !has_sep && (c == '=' || c == ':') {
                has_sep = true;
            } else {
                break;
            }
        }
        offset += c.len_utf8();
    }
    (&line[..key_end], &rest[offset..])
}

/// Resolve Java properties escapes.
fn unescape(s: &str, line: u32, on_error: &mut dyn FnMut(u32, String)) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(e) = it.next() else {
            out.push('\\');
            break;
        };
        match e {
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            'f' => out.push('\u{c}'),
            'u' => {
                let hex: String = it.clone().take(4).collect();
                let Some(unit) = parse_hex4(&hex) else {
                    on_error(line, format!("malformed \\uXXXX escape `\\u{hex}`, kept literally"));
                    out.push('u');
                    continue;
                };
                for _ in 0..4 {
                    it.next();
                }
                if (0xD800..0xDC00).contains(&unit) {
                    // High surrogate: combine with a following `\uDC00`-`\uDFFF` escape.
                    let mut look = it.clone();
                    if look.next() == Some('\\') && look.next() == Some('u') {
                        let low_hex: String = look.take(4).collect();
                        if let Some(low) = parse_hex4(&low_hex).filter(|l| (0xDC00..0xE000).contains(l)) {
                            let cp = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                            out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                            for _ in 0..6 {
                                it.next();
                            }
                            continue;
                        }
                    }
                    out.push('\u{FFFD}');
                } else {
                    // Lone low surrogates cannot be represented in a Rust string.
                    out.push(char::from_u32(unit).unwrap_or('\u{FFFD}'));
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn parse_hex4(s: &str) -> Option<u32> {
    if s.len() == 4 && s.chars().all(|h| h.is_ascii_hexdigit()) { u32::from_str_radix(s, 16).ok() } else { None }
}

/// Escape a key for writing (`Properties.store` conventions).
pub fn escape_key(key: &str) -> String {
    escape(key, true)
}

/// Escape a value for writing (`Properties.store` conventions).
pub fn escape_value(value: &str) -> String {
    escape(value, false)
}

fn escape(s: &str, is_key: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        match c {
            ' ' if i == 0 || is_key => out.push_str("\\ "),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '=' | ':' | '#' | '!' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                // Non-printable or non-ASCII: \uXXXX (UTF-16 code units).
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{:04X}", u));
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Write entries in `.properties` syntax (one `key=value` per line).
pub fn write<'a>(entries: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut out = String::new();
    for (k, v) in entries {
        out.push_str(&escape_key(k));
        out.push('=');
        out.push_str(&escape_value(v));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn kv(text: &str) -> Vec<(String, String)> {
        parse(text).into_iter().map(|e| (e.key, e.value)).collect()
    }

    fn pair(k: &str, v: &str) -> (String, String) {
        (k.to_string(), v.to_string())
    }

    #[test]
    fn separators() {
        assert_eq!(kv("a=1\nb:2\nc 3\nd\t4\ne = 5\nf : 6\ng   =   7"), vec![
            pair("a", "1"),
            pair("b", "2"),
            pair("c", "3"),
            pair("d", "4"),
            pair("e", "5"),
            pair("f", "6"),
            pair("g", "7"),
        ]);
        // Only one separator character is consumed after whitespace.
        assert_eq!(kv("a = = b"), vec![pair("a", "= b")]);
        assert_eq!(kv("a==b"), vec![pair("a", "=b")]);
        // Key only.
        assert_eq!(kv("lonely"), vec![pair("lonely", "")]);
        assert_eq!(kv("empty="), vec![pair("empty", "")]);
    }

    #[test]
    fn comments_and_blank_lines() {
        assert_eq!(kv("# comment\n! also\n\n   \n  a=1\n   # indented comment\nb=2 # not a comment"), vec![
            pair("a", "1"),
            pair("b", "2 # not a comment"),
        ]);
        // A comment line ending in a backslash does not continue.
        assert_eq!(kv("# comment \\\na=1"), vec![pair("a", "1")]);
    }

    #[test]
    fn continuation_lines() {
        assert_eq!(kv("a=one \\\n    two \\\n\tthree"), vec![pair("a", "one two three")]);
        // A continuation line starting with # is not a comment.
        assert_eq!(kv("a=x \\\n  #y"), vec![pair("a", "x #y")]);
        // An even number of backslashes does not continue.
        assert_eq!(kv("a=x\\\\\nb=y"), vec![pair("a", "x\\"), pair("b", "y")]);
        // A blank line after a continuation ends the logical line.
        assert_eq!(kv("a=x \\\n\nb=y"), vec![pair("a", "x "), pair("b", "y")]);
        // Continuation at EOF.
        assert_eq!(kv("a=x\\"), vec![pair("a", "x")]);
        // CRLF and CR line endings.
        assert_eq!(kv("a=1 \\\r\n  2\r\nb=3\rc=4"), vec![pair("a", "1 2"), pair("b", "3"), pair("c", "4")]);
    }

    #[test]
    fn escapes() {
        assert_eq!(kv(r"a=tab\there"), vec![pair("a", "tab\there")]);
        assert_eq!(kv(r"a=\n\r\f"), vec![pair("a", "\n\r\u{c}")]);
        assert_eq!(kv(r"a=\u0041\u00e9"), vec![pair("a", "A\u{e9}")]);
        assert_eq!(kv(r"key\=with\:seps\ and\ space=v"), vec![pair("key=with:seps and space", "v")]);
        assert_eq!(kv(r"a=\q\#\\"), vec![pair("a", "q#\\")]);
        assert_eq!(kv(r"\#notcomment=1"), vec![pair("#notcomment", "1")]);
    }

    #[test]
    fn malformed_unicode_escape_is_reported() {
        let (entries, diags) = parse_with_diagnostics("a=\\u12G4\nb=ok", "x.properties");
        assert_eq!(entries[0].value, "u12G4");
        assert_eq!(entries[1].value, "ok");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags.iter().next().unwrap().location.as_ref().unwrap().line, 1);
    }

    #[test]
    fn trailing_whitespace_is_kept_in_values() {
        assert_eq!(kv("a=1  "), vec![pair("a", "1  ")]);
    }

    #[test]
    fn duplicates_keep_first_position_and_last_value() {
        let entries = parse("a=1\nb=2\na=3");
        assert_eq!(entries, vec![PropEntry::new("a", "3", 3), PropEntry::new("b", "2", 2)]);
    }

    #[test]
    fn line_numbers_are_first_physical_line() {
        let entries = parse("\n# c\nx=1 \\\n  2\n\ny=3");
        assert_eq!(entries, vec![PropEntry::new("x", "1 2", 3), PropEntry::new("y", "3", 6)]);
    }

    #[test]
    fn unicode_text() {
        assert_eq!(kv("option.X=Rendu \u{e9}l\u{e9}gant"), vec![pair("option.X", "Rendu \u{e9}l\u{e9}gant")]);
    }

    #[test]
    fn write_roundtrip() {
        let entries = [("a b", " lead"), ("c=d", "x:y#z!"), ("uni", "\u{e9}\u{1F600}"), ("t", "a\tb\\")];
        let text = write(entries.iter().map(|(k, v)| (*k, *v)));
        let back = kv(&text);
        let expect: Vec<_> = entries.iter().map(|(k, v)| pair(k, v)).collect();
        assert_eq!(back, expect);
    }
}
