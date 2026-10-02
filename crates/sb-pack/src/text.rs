//! Text decoding and line-splitting helpers shared by the whole crate.
//!
//! * GLSL sources and `.lang` files are UTF-8 (decoded lossily, BOM stripped).
//! * `.properties` files are ISO-8859-1 (Java `Properties` / Iris convention).
//!
//! Line splitting treats `\r\n`, `\n` and `\r` as line terminators, which is what
//! every other ShaderBridge component (preprocessor, diagnostics) assumes, so line
//! numbers stay consistent across crates.

/// The UTF-8 byte order mark.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Decode bytes as UTF-8, replacing invalid sequences with U+FFFD and stripping a
/// leading byte order mark.
pub fn decode_utf8(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// Decode bytes as ISO-8859-1 (every byte maps to the code point of the same value).
///
/// A leading UTF-8 byte order mark is stripped first: some packs save their
/// `.properties` files from editors that add one, and decoding it as Latin-1 would
/// corrupt the first key.
pub fn decode_latin1(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// A line of a text: its content (without terminator) and byte range in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineSpan<'a> {
    /// Line content without the terminator.
    pub text: &'a str,
    /// Byte offset of the first character of the line.
    pub start: usize,
    /// Byte offset just past the line content (start of the terminator, if any).
    pub end: usize,
    /// Length in bytes of the terminator (`0`, `1` or `2`).
    pub terminator_len: usize,
}

/// Split `text` into lines (terminators `\r\n`, `\n`, `\r`).
///
/// A trailing terminator does not produce an extra empty line, so `"a\nb\n"` and
/// `"a\nb"` both yield two lines. An empty text yields no lines.
pub fn line_spans(text: &str) -> Vec<LineSpan<'_>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                out.push(LineSpan { text: &text[start..i], start, end: i, terminator_len: 1 });
                i += 1;
                start = i;
            }
            b'\r' => {
                let len = if bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                out.push(LineSpan { text: &text[start..i], start, end: i, terminator_len: len });
                i += len;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < bytes.len() {
        out.push(LineSpan { text: &text[start..], start, end: bytes.len(), terminator_len: 0 });
    }
    out
}

/// Split `text` into line contents (see [`line_spans`]).
pub fn lines(text: &str) -> Vec<&str> {
    line_spans(text).into_iter().map(|l| l.text).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_bom_is_stripped() {
        assert_eq!(decode_utf8(b"\xEF\xBB\xBF#version 120"), "#version 120");
        assert_eq!(decode_utf8(b"plain"), "plain");
    }

    #[test]
    fn utf8_invalid_is_lossy() {
        assert_eq!(decode_utf8(b"a\xFFb"), "a\u{FFFD}b");
    }

    #[test]
    fn latin1_maps_bytes_to_code_points() {
        assert_eq!(decode_latin1(b"caf\xE9"), "caf\u{e9}");
        assert_eq!(decode_latin1(b"\xEF\xBB\xBFkey=v"), "key=v");
    }

    #[test]
    fn line_splitting_handles_all_terminators() {
        assert_eq!(lines("a\nb\r\nc\rd"), vec!["a", "b", "c", "d"]);
        assert_eq!(lines("a\n"), vec!["a"]);
        assert_eq!(lines("a\n\n"), vec!["a", ""]);
        assert!(lines("").is_empty());
        let spans = line_spans("ab\r\ncd");
        assert_eq!(spans[0].start, 0);
        assert_eq!(spans[0].end, 2);
        assert_eq!(spans[0].terminator_len, 2);
        assert_eq!(spans[1].start, 4);
        assert_eq!(spans[1].terminator_len, 0);
    }
}
