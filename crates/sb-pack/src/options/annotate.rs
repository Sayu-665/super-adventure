//! Per-line option detection, a port of Iris's `OptionAnnotatedSource` and
//! `ParsedString` that additionally records byte spans so values can be edited in
//! place.

use std::ops::Range;

/// `const` names that may be options (Iris `VALID_CONST_OPTION_NAMES`).
pub const CONST_OPTION_NAMES: [&str; 25] = [
    "shadowMapResolution",
    "shadowDistance",
    "voxelDistance",
    "shadowDistanceRenderMul",
    "entityShadowDistanceMul",
    "shadowIntervalSize",
    "generateShadowMipmap",
    "generateShadowColorMipmap",
    "shadowHardwareFiltering",
    "shadowtex0Mipmap",
    "shadowtexMipmap",
    "shadowtex1Mipmap",
    "shadowtex0Nearest",
    "shadowtexNearest",
    "shadow0MinMagNearest",
    "shadowtex1Nearest",
    "shadow1MinMagNearest",
    "wetnessHalflife",
    "drynessHalflife",
    "eyeBrightnessHalflife",
    "centerDepthHalflife",
    "sunPathRotation",
    "ambientOcclusionLevel",
    "superSamplingLevel",
    "noiseTextureResolution",
];

/// Whether `name` is a configurable `const` (including the per-index
/// `shadowcolorN*` / `shadowColorN*` / `shadowHardwareFilteringN` names, N < 8).
pub fn is_const_option_name(name: &str) -> bool {
    if CONST_OPTION_NAMES.contains(&name) {
        return true;
    }
    let indexed = |prefix: &str, suffixes: &[&str]| {
        name.strip_prefix(prefix).is_some_and(|rest| {
            let mut chars = rest.chars();
            matches!(chars.next(), Some('0'..='7')) && suffixes.contains(&chars.as_str())
        })
    };
    indexed("shadowcolor", &["Mipmap", "Nearest", "MinMagNearest"])
        || indexed("shadowColor", &["Mipmap", "Nearest", "MinMagNearest"])
        || indexed("shadowHardwareFiltering", &[""])
}

/// What a line declares or references.
#[derive(Debug, Clone, PartialEq)]
pub enum LineAnnotation {
    /// `#ifdef NAME` / `#ifndef NAME` (exactly; trailing comments disqualify).
    Reference(String),
    /// `[//]#define NAME [// comment]`.
    BoolDefine {
        name: String,
        default: bool,
        comment: Option<String>,
    },
    /// `#define NAME VALUE // [a b c] comment`.
    ValueDefine {
        name: String,
        value: String,
        value_span: Range<usize>,
        allowed: Vec<String>,
        comment: Option<String>,
    },
    /// `const bool NAME = true|false; [// comment]` with a whitelisted name.
    ConstBool {
        name: String,
        default: bool,
        value_span: Range<usize>,
        comment: Option<String>,
    },
    /// `const int|float NAME = VALUE; // [a b c]` with a whitelisted name.
    ConstValue {
        name: String,
        value: String,
        value_span: Range<usize>,
        allowed: Vec<String>,
        comment: Option<String>,
    },
}

// ---------------------------------------------------------------------------------
// Java character classes. Iris's `ParsedString` works on UTF-16 `char`s, so
// supplementary characters (surrogate pairs) are never letters, digits or whitespace.
// The tables below were generated from OpenJDK 21 and checked with a differential
// test against Iris's own `ParsedString`/`OptionAnnotatedSource` code.
// ---------------------------------------------------------------------------------

/// BMP ranges of Unicode category Nd (`Character.isDigit(char)`).
const JAVA_BMP_DIGITS: [(char, char); 37] = [
    ('\u{30}', '\u{39}'),
    ('\u{660}', '\u{669}'),
    ('\u{6F0}', '\u{6F9}'),
    ('\u{7C0}', '\u{7C9}'),
    ('\u{966}', '\u{96F}'),
    ('\u{9E6}', '\u{9EF}'),
    ('\u{A66}', '\u{A6F}'),
    ('\u{AE6}', '\u{AEF}'),
    ('\u{B66}', '\u{B6F}'),
    ('\u{BE6}', '\u{BEF}'),
    ('\u{C66}', '\u{C6F}'),
    ('\u{CE6}', '\u{CEF}'),
    ('\u{D66}', '\u{D6F}'),
    ('\u{DE6}', '\u{DEF}'),
    ('\u{E50}', '\u{E59}'),
    ('\u{ED0}', '\u{ED9}'),
    ('\u{F20}', '\u{F29}'),
    ('\u{1040}', '\u{1049}'),
    ('\u{1090}', '\u{1099}'),
    ('\u{17E0}', '\u{17E9}'),
    ('\u{1810}', '\u{1819}'),
    ('\u{1946}', '\u{194F}'),
    ('\u{19D0}', '\u{19D9}'),
    ('\u{1A80}', '\u{1A89}'),
    ('\u{1A90}', '\u{1A99}'),
    ('\u{1B50}', '\u{1B59}'),
    ('\u{1BB0}', '\u{1BB9}'),
    ('\u{1C40}', '\u{1C49}'),
    ('\u{1C50}', '\u{1C59}'),
    ('\u{A620}', '\u{A629}'),
    ('\u{A8D0}', '\u{A8D9}'),
    ('\u{A900}', '\u{A909}'),
    ('\u{A9D0}', '\u{A9D9}'),
    ('\u{A9F0}', '\u{A9F9}'),
    ('\u{AA50}', '\u{AA59}'),
    ('\u{ABF0}', '\u{ABF9}'),
    ('\u{FF10}', '\u{FF19}'),
];

/// `Character.isDigit(char)`.
fn java_is_digit(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_digit();
    }
    JAVA_BMP_DIGITS
        .binary_search_by(|&(lo, hi)| {
            if hi < c {
                std::cmp::Ordering::Less
            } else if lo > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// A `ParsedString.takeWord` character: `Character.isDigit || Character.isAlphabetic
/// || '_'` on a UTF-16 unit.
fn java_is_word_char(c: char) -> bool {
    c == '_' || java_is_digit(c) || (u32::from(c) <= 0xFFFF && c.is_alphabetic())
}

/// `Character.isWhitespace(char)`: Unicode space/line/paragraph separators except the
/// non-breaking ones, plus `\t`..`\r` and `\u{1C}`..`\u{1F}`.
fn java_is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t'..='\r'
            | '\u{1C}'..='\u{20}'
            | '\u{1680}'
            | '\u{2000}'..='\u{2006}'
            | '\u{2008}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

/// Characters removed by `String.trim()` (every code point up to U+0020).
fn is_java_trim(c: char) -> bool {
    c <= ' '
}

/// `String.trim()`.
fn java_trim(s: &str) -> &str {
    s.trim_matches(is_java_trim)
}

/// A cursor over a line mirroring Iris's `ParsedString` (positions are byte offsets
/// into the original line).
struct Cursor<'a> {
    s: &'a str,
    pos: usize,
    end: usize,
    /// Also accept a trailing `.` after digits (`5.`), as OptiFine does.
    optifine_numbers: bool,
}

impl<'a> Cursor<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.pos..self.end]
    }
    fn is_end(&self) -> bool {
        self.pos >= self.end
    }
    fn take_literal(&mut self, lit: &str) -> bool {
        if self.rest().starts_with(lit) {
            self.pos += lit.len();
            true
        } else {
            false
        }
    }
    /// Iris `takeSomeWhitespace`: requires the next character to be Java whitespace,
    /// then applies `String.trim()` (which only removes characters up to U+0020, so a
    /// leading U+2003 is "taken" without being consumed). The line end was already
    /// trimmed the same way.
    fn take_some_whitespace(&mut self) -> bool {
        match self.rest().chars().next() {
            Some(c) if java_is_whitespace(c) => {
                let rest = self.rest();
                let skipped = rest.len() - rest.trim_start_matches(is_java_trim).len();
                self.pos += skipped;
                true
            }
            _ => false,
        }
    }
    /// `//` followed by any number of extra `/`.
    fn take_comments(&mut self) -> bool {
        if !self.take_literal("//") {
            return false;
        }
        while self.take_literal("/") {}
        true
    }
    fn take_word(&mut self) -> Option<Range<usize>> {
        let start = self.pos;
        let len: usize = self
            .rest()
            .chars()
            .take_while(|&c| java_is_word_char(c))
            .map(char::len_utf8)
            .sum();
        if len == 0 {
            return None;
        }
        self.pos += len;
        Some(start..self.pos)
    }
    /// Iris `takeNumber`: consume while the current or the next character is a digit,
    /// plus an `f`/`F` suffix if more text follows; must parse as a Java float.
    fn take_number(&mut self) -> Option<Range<usize>> {
        let chars: Vec<(usize, char)> = self.rest().char_indices().collect();
        let mut i = 0;
        while i < chars.len() {
            let cur = java_is_digit(chars[i].1);
            if u32::from(chars[i].1) > 0xFFFF {
                // Java sees a surrogate pair here: neither unit is a digit.
                break;
            }
            if i + 1 < chars.len() {
                if !cur && !java_is_digit(chars[i + 1].1) {
                    break;
                }
            } else if !cur {
                break;
            }
            i += 1;
        }
        if self.optifine_numbers
            && i > 0
            && chars[i - 1].1.is_ascii_digit()
            && chars.get(i).is_some_and(|c| c.1 == '.')
            && !chars.get(i + 1).is_some_and(|c| java_is_word_char(c.1))
        {
            i += 1;
        }
        if i > 0 && i + 1 < chars.len() && matches!(chars[i].1, 'f' | 'F') {
            i += 1;
        }
        let byte_len = chars.get(i).map(|(b, _)| *b).unwrap_or(self.rest().len());
        if !java_parse_float_ok(&self.rest()[..byte_len]) {
            return None;
        }
        let start = self.pos;
        self.pos += byte_len;
        Some(start..self.pos)
    }
    fn take_word_or_number(&mut self) -> Option<Range<usize>> {
        self.take_number().or_else(|| self.take_word())
    }
}

/// Whether Java's `Float.parseFloat` accepts `s` (for the strings `take_number` can
/// produce: optional sign, digits, `.`, exponent, `f`/`F`/`d`/`D` suffix).
fn java_parse_float_ok(s: &str) -> bool {
    let s = java_trim(s);
    let body = s.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(s);
    if body.is_empty() || body.ends_with(['e', 'E', '+', '-']) {
        return false;
    }
    // Rust accepts "inf"/"nan" spellings Java rejects; they cannot contain digits, but
    // be explicit anyway.
    if body
        .chars()
        .any(|c| c.is_ascii_alphabetic() && !matches!(c, 'e' | 'E'))
    {
        return false;
    }
    body.parse::<f64>().is_ok()
}

/// Split `[a b c]` out of a comment: returns (allowed values, remaining comment).
/// `None` if there is no `[...]` list. The default is appended if missing.
fn allowed_values(comment: &str, default: &str) -> Option<(Vec<String>, Option<String>)> {
    let open = comment.find('[')?;
    let close = open + comment[open..].find(']')?;
    let mut allowed: Vec<String> = comment[open + 1..close]
        .split(' ')
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .collect();
    if !allowed.iter().any(|v| v == default) {
        allowed.push(default.to_string());
    }
    let remaining = format!("{}{}", &comment[..open], &comment[close + 1..]);
    let remaining = java_trim(&remaining);
    Some((
        allowed,
        (!remaining.is_empty()).then(|| remaining.to_string()),
    ))
}

fn opt_comment(s: &str) -> Option<String> {
    let t = java_trim(s);
    (!t.is_empty()).then(|| t.to_string())
}

/// Annotate one source line with Iris's rules. Returns `None` for lines without
/// option meaning.
pub fn annotate_line(line: &str) -> Option<LineAnnotation> {
    annotate_line_with(line, false)
}

/// [`annotate_line`], optionally also accepting OptiFine's number syntax for values
/// (a trailing `.` such as `#define FOG 5. // [1. 5.]`, which Iris rejects).
pub fn annotate_line_with(line: &str, optifine_numbers: bool) -> Option<LineAnnotation> {
    if !line.contains("#define")
        && !line.contains("const")
        && !line.contains("#ifdef")
        && !line.contains("#ifndef")
    {
        return None;
    }
    let lead = line.len() - line.trim_start_matches(is_java_trim).len();
    let end = line.trim_end_matches(is_java_trim).len().max(lead);
    let mut c = Cursor {
        s: line,
        pos: lead,
        end,
        optifine_numbers,
    };

    if c.take_literal("#ifdef") || c.take_literal("#ifndef") {
        if !c.take_some_whitespace() {
            return None;
        }
        let name = c.take_word()?;
        c.take_some_whitespace();
        return c
            .is_end()
            .then(|| LineAnnotation::Reference(line[name].to_string()));
    }
    if c.take_literal("const") {
        return annotate_const(line, c);
    }
    if c.rest().contains("#define") {
        return annotate_define(line, c);
    }
    None
}

fn annotate_const(line: &str, mut c: Cursor<'_>) -> Option<LineAnnotation> {
    if !c.take_some_whitespace() {
        return None;
    }
    let is_bool = if c.take_literal("int") || c.take_literal("float") {
        false
    } else if c.take_literal("bool") {
        true
    } else {
        return None;
    };
    if !c.take_some_whitespace() {
        return None;
    }
    let name = line[c.take_word()?].to_string();
    c.take_some_whitespace();
    if !c.take_literal("=") {
        return None;
    }
    c.take_some_whitespace();
    let value_span = c.take_word_or_number()?;
    let value = line[value_span.clone()].to_string();
    c.take_some_whitespace();
    if !c.take_literal(";") {
        return None;
    }
    c.take_some_whitespace();
    let comment = if c.take_comments() {
        Some(java_trim(c.rest()).to_string())
    } else if !c.is_end() {
        return None;
    } else {
        None
    };
    if !is_const_option_name(&name) {
        return None;
    }
    if is_bool {
        let default = match value.as_str() {
            "true" => true,
            "false" => false,
            _ => return None,
        };
        return Some(LineAnnotation::ConstBool {
            name,
            default,
            value_span,
            comment: comment.as_deref().and_then(opt_comment),
        });
    }
    let (allowed, comment) = allowed_values(comment.as_deref()?, &value)?;
    Some(LineAnnotation::ConstValue {
        name,
        value,
        value_span,
        allowed,
        comment,
    })
}

fn annotate_define(line: &str, mut c: Cursor<'_>) -> Option<LineAnnotation> {
    let has_leading_comment = c.take_comments();
    c.take_some_whitespace();
    if !c.take_literal("#define") {
        return None;
    }
    if !c.take_some_whitespace() {
        return None;
    }
    let name = line[c.take_word()?].to_string();
    let took_ws = c.take_some_whitespace();
    if c.is_end() {
        return Some(LineAnnotation::BoolDefine {
            name,
            default: !has_leading_comment,
            comment: None,
        });
    }
    if c.take_comments() {
        return Some(LineAnnotation::BoolDefine {
            name,
            default: !has_leading_comment,
            comment: opt_comment(c.rest()),
        });
    }
    if !took_ws || has_leading_comment {
        return None;
    }
    let value_span = c.take_word_or_number()?;
    let value = line[value_span.clone()].to_string();
    let took_ws = c.take_some_whitespace();
    if c.is_end() {
        return None;
    }
    if !c.take_comments() {
        return None;
    }
    let _ = took_ws;
    let (allowed, comment) = allowed_values(java_trim(c.rest()), &value)?;
    Some(LineAnnotation::ValueDefine {
        name,
        value,
        value_span,
        allowed,
        comment,
    })
}

/// Whether the (trimmed) line starts with `//`.
pub fn has_leading_comment(line: &str) -> bool {
    line.trim_start_matches(is_java_trim).starts_with("//")
}

/// Toggle a boolean `#define` line. Returns `None` if the line already has the
/// requested state.
pub fn set_boolean_define(line: &str, enabled: bool) -> Option<String> {
    let commented = has_leading_comment(line);
    if commented && enabled {
        let indent_len = line.len() - line.trim_start_matches(is_java_trim).len();
        let rest = line[indent_len..].trim_start_matches('/');
        Some(format!("{}{}", &line[..indent_len], rest))
    } else if !commented && !enabled {
        Some(format!("//{line}"))
    } else {
        None
    }
}

/// Replace the byte range `span` of `line` with `value`.
pub fn replace_span(line: &str, span: Range<usize>, value: &str) -> Option<String> {
    let prefix = line.get(..span.start)?;
    let suffix = line.get(span.end..)?;
    Some(format!("{prefix}{value}{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn bool_define(name: &str, default: bool, comment: Option<&str>) -> Option<LineAnnotation> {
        Some(LineAnnotation::BoolDefine {
            name: name.into(),
            default,
            comment: comment.map(str::to_string),
        })
    }

    #[test]
    fn references() {
        assert_eq!(
            annotate_line("#ifdef SHADOWS"),
            Some(LineAnnotation::Reference("SHADOWS".into()))
        );
        assert_eq!(
            annotate_line("   #ifndef   FOO   "),
            Some(LineAnnotation::Reference("FOO".into()))
        );
        assert_eq!(
            annotate_line("#ifdef FOO // trailing comment"),
            None,
            "Iris requires the line to end"
        );
        assert_eq!(annotate_line("#if defined FOO"), None);
        assert_eq!(annotate_line("#ifdef"), None);
        assert_eq!(annotate_line("#ifdefFOO"), None);
    }

    #[test]
    fn boolean_defines() {
        assert_eq!(
            annotate_line("#define SHADOWS"),
            bool_define("SHADOWS", true, None)
        );
        assert_eq!(
            annotate_line("//#define SHADOWS"),
            bool_define("SHADOWS", false, None)
        );
        assert_eq!(
            annotate_line("  ///  #define SHADOWS  // Enables shadows"),
            bool_define("SHADOWS", false, Some("Enables shadows"))
        );
        assert_eq!(
            annotate_line("#define BLOOM //Bloom [not a list]"),
            bool_define("BLOOM", true, Some("Bloom [not a list]"))
        );
        assert_eq!(
            annotate_line("#define\tTABBED"),
            bool_define("TABBED", true, None)
        );
        assert_eq!(annotate_line("#define X(a) a"), None);
        assert_eq!(annotate_line("#defineX"), None);
        assert_eq!(annotate_line("/* #define X */"), None);
    }

    #[test]
    fn value_defines() {
        let a = annotate_line("#define SHADOW_QUALITY 2 // [0 1 2 3] Quality of shadows").unwrap();
        let LineAnnotation::ValueDefine {
            name,
            value,
            value_span,
            allowed,
            comment,
        } = a
        else {
            panic!()
        };
        assert_eq!((name.as_str(), value.as_str()), ("SHADOW_QUALITY", "2"));
        assert_eq!(
            &"#define SHADOW_QUALITY 2 // [0 1 2 3] Quality of shadows"[value_span],
            "2"
        );
        assert_eq!(allowed, vec!["0", "1", "2", "3"]);
        assert_eq!(comment.as_deref(), Some("Quality of shadows"));

        // Default appended when missing from the list; extra spaces ignored.
        let Some(LineAnnotation::ValueDefine { allowed, .. }) =
            annotate_line("#define F 0.5 //[0.25  1.0]")
        else {
            panic!()
        };
        assert_eq!(allowed, vec!["0.25", "1.0", "0.5"]);
        // No space between value and comment.
        let Some(LineAnnotation::ValueDefine { value, .. }) =
            annotate_line("#define N -1//[-1 0 1]")
        else {
            panic!()
        };
        assert_eq!(value, "-1");
        // Word values.
        let Some(LineAnnotation::ValueDefine { value, .. }) =
            annotate_line("#define MODE FAST // [FAST SLOW]")
        else {
            panic!()
        };
        assert_eq!(value, "FAST");
        // Float suffix is part of the number when followed by more text.
        let Some(LineAnnotation::ValueDefine { value, .. }) =
            annotate_line("#define S 1.0f // [1.0f 2.0f]")
        else {
            panic!()
        };
        assert_eq!(value, "1.0f");
        // Not options: no list, leading comment, expression values, no comment.
        assert_eq!(annotate_line("#define PI 3.14159 // pi"), None);
        assert_eq!(annotate_line("//#define Q 2 // [1 2]"), None);
        assert_eq!(annotate_line("#define C vec3(1.0) // [a b]"), None);
        assert_eq!(annotate_line("#define V 2"), None);
        assert_eq!(annotate_line("#define V 2 3 // [2 3]"), None);
    }

    #[test]
    fn optifine_number_syntax_is_opt_in() {
        let line = "#define Fog_Scale 5. // [1. 3. 5. 8.] Scale";
        assert_eq!(annotate_line(line), None, "Iris rejects `5.`");
        let Some(LineAnnotation::ValueDefine {
            value,
            value_span,
            allowed,
            ..
        }) = annotate_line_with(line, true)
        else {
            panic!()
        };
        assert_eq!((value.as_str(), &line[value_span]), ("5.", "5."));
        assert_eq!(allowed, vec!["1.", "3.", "5.", "8."]);
        // A dot followed by a word character is not swallowed.
        assert_eq!(annotate_line_with("#define A 5.x // [1]", true), None);
        // Unchanged behaviour for normal numbers.
        let Some(LineAnnotation::ValueDefine { value, .. }) =
            annotate_line_with("#define B 0.5 // [0.5]", true)
        else {
            panic!()
        };
        assert_eq!(value, "0.5");
    }

    #[test]
    fn const_options() {
        let line = "const int shadowMapResolution = 2048; // [1024 2048 4096]";
        let Some(LineAnnotation::ConstValue {
            name,
            value,
            value_span,
            allowed,
            comment,
        }) = annotate_line(line)
        else {
            panic!()
        };
        assert_eq!(
            (name.as_str(), value.as_str(), &line[value_span]),
            ("shadowMapResolution", "2048", "2048")
        );
        assert_eq!(allowed, vec!["1024", "2048", "4096"]);
        assert_eq!(comment, None);

        let line = "  const float sunPathRotation = -40.0f; //[-40.0f 0.0f 40.0f] Sun angle";
        let Some(LineAnnotation::ConstValue { value, comment, .. }) = annotate_line(line) else {
            panic!()
        };
        assert_eq!(value, "-40.0f");
        assert_eq!(comment.as_deref(), Some("Sun angle"));

        let line = "const bool shadowHardwareFiltering0 = true; // PCF";
        let Some(LineAnnotation::ConstBool {
            name,
            default,
            value_span,
            comment,
        }) = annotate_line(line)
        else {
            panic!()
        };
        assert_eq!(
            (
                name.as_str(),
                default,
                &line[value_span],
                comment.as_deref()
            ),
            ("shadowHardwareFiltering0", true, "true", Some("PCF"))
        );
        assert!(matches!(
            annotate_line("const bool shadowcolor1Nearest = false;"),
            Some(LineAnnotation::ConstBool { default: false, .. })
        ));

        // Not options.
        assert_eq!(
            annotate_line("const int shadowMapResolution = 2048;"),
            None,
            "needs a list"
        );
        assert_eq!(
            annotate_line("const int myConst = 2; // [1 2]"),
            None,
            "not whitelisted"
        );
        assert_eq!(annotate_line("const vec3 sunColor = vec3(1.0);"), None);
        assert_eq!(
            annotate_line("const float sunPathRotation = 1.0 * 2.0; // [1]"),
            None
        );
        assert_eq!(
            annotate_line("const bool shadowHardwareFiltering = maybe;"),
            None
        );
        assert_eq!(
            annotate_line("const int shadowMapResolution = 2048; float x;"),
            None
        );
        assert_eq!(annotate_line("constant"), None);
    }

    #[test]
    fn const_name_whitelist() {
        assert!(is_const_option_name("shadowDistance"));
        assert!(is_const_option_name("shadowcolor7Mipmap"));
        assert!(is_const_option_name("shadowColor0MinMagNearest"));
        assert!(is_const_option_name("shadowHardwareFiltering1"));
        assert!(!is_const_option_name("shadowcolor8Mipmap"));
        assert!(!is_const_option_name("shadowHardwareFiltering12"));
        assert!(!is_const_option_name("colortex0Format"));
    }

    #[test]
    fn java_character_classes_match_iris() {
        // Regressions found by a differential test against Iris's own `ParsedString`
        // code: Java's `trim()` only strips characters up to U+0020, `takeSomeWhitespace`
        // only trims those, and words/numbers use `Character.isDigit` (category Nd).
        assert_eq!(
            annotate_line("\u{a0}#define X"),
            None,
            "NBSP is not trimmed"
        );
        assert_eq!(annotate_line("#define X\u{a0}"), None);
        assert_eq!(annotate_line("  \u{2003}#define X"), None);
        assert_eq!(annotate_line("#define\u{2003}X"), None);
        assert_eq!(annotate_line("#ifdef \u{a0}X"), None);
        assert_eq!(annotate_line("#define Q _5\u{2003} // [5]"), None);
        // `²` is category No: not part of a word.
        assert_eq!(annotate_line("#ifdef A\u{b2}B"), None);
        assert_eq!(annotate_line("#define A\u{b2}B"), None);
        // Non-ASCII decimal digits (Nd) are digits; the value is not a Java float, so
        // it is taken as a word.
        assert!(matches!(
            annotate_line("#define V 1\u{663} // [1]"),
            Some(LineAnnotation::ValueDefine { ref value, .. }) if value == "1\u{663}"
        ));
        // Comments keep a leading NBSP (only `<= U+0020` is trimmed).
        assert!(matches!(
            annotate_line("#define Q 1 // \u{a0}text [1 2]"),
            Some(LineAnnotation::ValueDefine { comment: Some(ref c), .. }) if c == "\u{a0}text"
        ));
        // Plain ASCII whitespace (incl. control characters) still works.
        assert_eq!(
            annotate_line("\u{b}\t#define X \u{1f}"),
            bool_define("X", true, None)
        );
        assert!(java_is_digit('\u{ff15}') && !java_is_digit('\u{b2}'));
        assert!(java_is_whitespace('\u{1c}') && !java_is_whitespace('\u{a0}'));
        assert!(
            !java_is_word_char('\u{1d400}'),
            "supplementary characters are surrogates in Java"
        );
    }

    #[test]
    fn java_float_rules() {
        for ok in ["1", "-1", "0.5", "1e5", "1.0f", "2D", "+3", ".5", "5."] {
            assert!(java_parse_float_ok(ok), "{ok}");
        }
        for bad in ["", "-", "1e", "0x10", "abc", "inf", "1_000", "f"] {
            assert!(!java_parse_float_ok(bad), "{bad}");
        }
    }

    #[test]
    fn editing() {
        assert_eq!(
            set_boolean_define("#define A", false).as_deref(),
            Some("//#define A")
        );
        assert_eq!(
            set_boolean_define("    // #define A // c", true).as_deref(),
            Some("     #define A // c")
        );
        assert_eq!(
            set_boolean_define("///#define A", true).as_deref(),
            Some("#define A")
        );
        assert_eq!(set_boolean_define("#define A", true), None);
        assert_eq!(set_boolean_define("//#define A", false), None);
        assert_eq!(
            replace_span("#define Q 2 // [1 2]", 10..11, "1").as_deref(),
            Some("#define Q 1 // [1 2]")
        );
        assert_eq!(replace_span("x", 5..6, "1"), None);
    }
}
