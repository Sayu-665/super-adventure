//! Parsing of glslang info logs.
//!
//! glslang reports one message per line:
//!
//! ```text
//! ERROR: 0:123: 'foo' : undeclared identifier
//! ERROR: 0:123:7: 'foo' : undeclared identifier        (with column display)
//! WARNING: 0:3: '#extension' : extension not supported: GL_foo
//! ERROR: #version: Desktop shaders for Vulkan SPIR-V require version 140 or higher
//! ERROR: Linking fragment stage: Missing entry point: Each stage requires one entry point
//! ERROR: 1 compilation errors.  No code generated.     (summary, dropped)
//! ```
//!
//! The location is `<string>:<line>[:<column>]`. `<string>` is `0` for the
//! single source string ShaderBridge passes, unless the source changes it with a
//! `#line` directive (then the location is kept in the message text instead).
//! Lines without a recognised prefix continue the previous message.

use serde::{Deserialize, Serialize};

/// Severity of a parsed log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogSeverity {
    /// `ERROR:`, `INTERNAL ERROR:` and `UNIMPLEMENTED:` lines.
    Error,
    /// `WARNING:` lines and SPIR-V generator warnings.
    Warning,
    /// `NOTE:` lines.
    Note,
}

/// One message of a glslang info log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// Error, warning or note.
    pub severity: LogSeverity,
    /// 1-based line in source string 0 (the GLSL handed to glslang).
    pub line: Option<u32>,
    /// 1-based column, when glslang reported one.
    pub column: Option<u32>,
    /// The message without prefix and location, e.g. `'foo' : undeclared identifier`.
    pub message: String,
}

const PREFIXES: [(&str, LogSeverity); 5] = [
    ("INTERNAL ERROR:", LogSeverity::Error),
    ("UNIMPLEMENTED:", LogSeverity::Error),
    ("ERROR:", LogSeverity::Error),
    ("WARNING:", LogSeverity::Warning),
    ("NOTE:", LogSeverity::Note),
];

/// Parse a glslang info log into entries, dropping glslang's summary lines
/// (`N compilation errors.  No code generated.`, `compilation terminated`).
pub fn parse_glslang_log(log: &str) -> Vec<LogEntry> {
    let mut out: Vec<LogEntry> = Vec::new();
    // Whether the previous prefixed line was kept (continuations of dropped lines are dropped).
    let mut last_kept = false;
    for raw in log.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let Some((severity, rest)) = split_prefix(line) else {
            if last_kept && let Some(prev) = out.last_mut() {
                prev.message.push('\n');
                prev.message.push_str(line.trim());
            }
            continue;
        };
        let (location, text) = split_location(rest.trim_start());
        let mut message = clean_message(text);
        if is_summary(&message) {
            last_kept = false;
            continue;
        }
        let (line_no, column) = match location {
            Some(Location { string: "0", line, column }) => (Some(line), column),
            Some(Location { string, line, column }) => {
                // A `#line` directive renamed the source string: keep the location as text.
                let col = column.map(|c| format!(":{c}")).unwrap_or_default();
                message = format!("{string}:{line}{col}: {message}");
                (None, None)
            }
            None => (None, None),
        };
        out.push(LogEntry { severity, line: line_no, column, message });
        last_kept = true;
    }
    out
}

/// Parse the message log of glslang's SPIR-V generator (`spv::SpvBuildLogger`):
/// lines such as `warning: ...`, `error: ...`, `TBD functionality: ...` or
/// `Missing functionality: ...`.
///
/// `error:` and `Missing functionality:` are errors: glslang reports the latter
/// when it could not translate an operation (e.g. `matrix swizzle`) and the
/// module it emits is then incomplete. Everything else is a warning.
pub fn parse_generator_messages(log: &str) -> Vec<LogEntry> {
    log.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (severity, message) = if let Some(m) = strip_prefix_ci(l, "error:") {
                (LogSeverity::Error, m.trim())
            } else if let Some(m) = strip_prefix_ci(l, "warning:") {
                (LogSeverity::Warning, m.trim())
            } else if strip_prefix_ci(l, "missing functionality:").is_some() {
                (LogSeverity::Error, l)
            } else {
                (LogSeverity::Warning, l)
            };
            LogEntry { severity, line: None, column: None, message: format!("SPIR-V generator: {message}") }
        })
        .collect()
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &s[prefix.len()..])
}

fn split_prefix(line: &str) -> Option<(LogSeverity, &str)> {
    PREFIXES.iter().find_map(|(p, sev)| line.strip_prefix(p).map(|rest| (*sev, rest)))
}

#[derive(Debug, PartialEq, Eq)]
struct Location<'a> {
    string: &'a str,
    line: u32,
    column: Option<u32>,
}

/// Split `0:12: msg` / `0:12:5: msg` / `file.glsl:3: msg` into location and message.
///
/// glslang always puts a space after the location; source-string names never
/// contain whitespace.
fn split_location(s: &str) -> (Option<Location<'_>>, &str) {
    fn parse(s: &str) -> Option<(Location<'_>, &str)> {
        let (string, rest) = s.split_once(':')?;
        if string.is_empty() || string.contains(char::is_whitespace) {
            return None;
        }
        let (line_txt, rest) = rest.split_once(':')?;
        let line = parse_digits(line_txt)?;
        if let Some((col_txt, after)) = rest.split_once(':')
            && let Some(column) = parse_digits(col_txt)
            && (after.is_empty() || after.starts_with(' '))
        {
            return Some((Location { string, line, column: Some(column) }, after.trim_start()));
        }
        if rest.is_empty() || rest.starts_with(' ') {
            return Some((Location { string, line, column: None }, rest.trim_start()));
        }
        None
    }
    match parse(s) {
        Some((loc, msg)) => (Some(loc), msg),
        None => (None, s),
    }
}

fn parse_digits(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Tidy a message: drop the empty-token prefix (`'' :  syntax error` -> `syntax error`)
/// and trailing whitespace.
fn clean_message(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix("'' :").map(str::trim_start).unwrap_or(s);
    s.to_string()
}

fn is_summary(msg: &str) -> bool {
    msg == "compilation terminated"
        || (msg.ends_with("compilation errors.  No code generated.") && msg.split(' ').next().is_some_and(|n| parse_digits(n).is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn e(severity: LogSeverity, line: Option<u32>, column: Option<u32>, message: &str) -> LogEntry {
        LogEntry { severity, line, column, message: message.to_string() }
    }

    #[test]
    fn parses_error_with_line() {
        let got = parse_glslang_log("ERROR: 0:123: 'foo' : undeclared identifier\n");
        assert_eq!(got, vec![e(LogSeverity::Error, Some(123), None, "'foo' : undeclared identifier")]);
    }

    #[test]
    fn parses_error_with_column() {
        let got = parse_glslang_log("ERROR: 0:12:5: 'x' : undeclared identifier \n");
        assert_eq!(got, vec![e(LogSeverity::Error, Some(12), Some(5), "'x' : undeclared identifier")]);
    }

    #[test]
    fn parses_warning() {
        let got = parse_glslang_log("WARNING: 0:3: '#extension' : extension not supported: GL_foo\n");
        assert_eq!(got, vec![e(LogSeverity::Warning, Some(3), None, "'#extension' : extension not supported: GL_foo")]);
    }

    #[test]
    fn drops_summary_and_termination_lines() {
        let log = "ERROR: 0:4: '=' :  cannot convert from ' const float' to ' temp highp int'\n\
                   ERROR: 0:4: '' : compilation terminated \n\
                   ERROR: 2 compilation errors.  No code generated.\n\n\n";
        let got = parse_glslang_log(log);
        assert_eq!(got, vec![e(LogSeverity::Error, Some(4), None, "'=' :  cannot convert from ' const float' to ' temp highp int'")]);
    }

    #[test]
    fn strips_empty_token() {
        let got = parse_glslang_log("ERROR: 0:4: '' :  syntax error, unexpected RIGHT_BRACE, expecting COMMA or SEMICOLON\n");
        assert_eq!(got[0].message, "syntax error, unexpected RIGHT_BRACE, expecting COMMA or SEMICOLON");
    }

    #[test]
    fn locationless_errors() {
        let log = "ERROR: #version: Desktop shaders for Vulkan SPIR-V require version 140 or higher\n\
                   ERROR: Linking fragment stage: Missing entry point: Each stage requires one entry point\n";
        let got = parse_glslang_log(log);
        assert_eq!(
            got,
            vec![
                e(LogSeverity::Error, None, None, "#version: Desktop shaders for Vulkan SPIR-V require version 140 or higher"),
                e(LogSeverity::Error, None, None, "Linking fragment stage: Missing entry point: Each stage requires one entry point"),
            ]
        );
    }

    #[test]
    fn continuation_lines_are_appended() {
        let log = "ERROR: Linking vertex stage: Types must match:\n    color: \"vec4\" versus \"vec3\"\nWARNING: 0:1: 'x' : y\n";
        let got = parse_glslang_log(log);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].message, "Linking vertex stage: Types must match:\ncolor: \"vec4\" versus \"vec3\"");
        assert_eq!(got[1].severity, LogSeverity::Warning);
    }

    #[test]
    fn other_prefixes() {
        let log = "INTERNAL ERROR: 0:9: boom\nUNIMPLEMENTED: 0:2: feature\nNOTE: 0:1: hint\n";
        let got = parse_glslang_log(log);
        assert_eq!(
            got,
            vec![
                e(LogSeverity::Error, Some(9), None, "boom"),
                e(LogSeverity::Error, Some(2), None, "feature"),
                e(LogSeverity::Note, Some(1), None, "hint"),
            ]
        );
    }

    #[test]
    fn renamed_source_string_keeps_location_in_text() {
        let got = parse_glslang_log("ERROR: lib/a.glsl:7: 'x' : undeclared identifier\nERROR: 3:9: 'y' : z\n");
        assert_eq!(got[0], e(LogSeverity::Error, None, None, "lib/a.glsl:7: 'x' : undeclared identifier"));
        assert_eq!(got[1], e(LogSeverity::Error, None, None, "3:9: 'y' : z"));
    }

    #[test]
    fn message_text_with_colons_is_not_a_location() {
        let got = parse_glslang_log("ERROR: 0:5: 'a' : b: c\n");
        assert_eq!(got[0], e(LogSeverity::Error, Some(5), None, "'a' : b: c"));
        // Location-like text without the trailing space is not a location.
        let got = parse_glslang_log("ERROR: foo:12:bar\n");
        assert_eq!(got[0], e(LogSeverity::Error, None, None, "foo:12:bar"));
    }

    #[test]
    fn garbage_is_tolerated() {
        for log in ["", "\n\n", "random text", "ERROR:", "ERROR: 0:", "ERROR: 0:99999999999: x", "ERROR: :1: x", "WARNING: 0:1:2"] {
            let _ = parse_glslang_log(log);
        }
        let got = parse_glslang_log("ERROR: 0:99999999999: x");
        assert_eq!(got[0].line, None);
    }

    #[test]
    fn generator_messages() {
        let got = parse_generator_messages("warning: thing\nerror: bad\nTBD functionality: x\nMissing functionality: matrix swizzle\n");
        assert_eq!(got.len(), 4);
        assert_eq!(got[0].severity, LogSeverity::Warning);
        assert_eq!(got[0].message, "SPIR-V generator: thing");
        assert_eq!(got[1].severity, LogSeverity::Error);
        assert_eq!(got[2].severity, LogSeverity::Warning);
        assert_eq!(got[2].message, "SPIR-V generator: TBD functionality: x");
        // An untranslated operation leaves the module incomplete.
        assert_eq!(got[3].severity, LogSeverity::Error);
        assert_eq!(got[3].message, "SPIR-V generator: Missing functionality: matrix swizzle");
    }
}
