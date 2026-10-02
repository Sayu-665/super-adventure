//! Protection against macro "bombs" in sources handed to glslang.
//!
//! ShaderBridge feeds glslang fully preprocessed (macro-free) GLSL, but
//! [`crate::compile_glsl`] accepts any text, and glslang's preprocessor has no
//! limits of its own:
//!
//! * expansion can grow exponentially (`#define A B B`, `#define B C C`, ... or
//!   `#define D(x) x x` nested as `D(D(D(...)))`): 2^30 tokens exhaust memory;
//! * nested invocations cost quadratic time and memory (`M(M(M(...)))` with
//!   `#define M(x) x` takes 3.5 GB at 4,000 levels).
//!
//! Before glslang runs, [`macro_hazard`] estimates the expansion of the source
//! from its `#define`s without performing it: the size of every macro
//! invocation is computed from the sizes of its arguments (memoized, so the
//! estimate itself stays linear), and nested invocations are counted. All
//! definitions of a name are considered and conditionals are ignored, so the
//! estimate errs on the large side. Token pasting and the rescanning of a
//! parameter that names a function-like macro are not modelled.

use std::collections::{HashMap, HashSet};

/// Largest accepted number of tokens after macro expansion. The largest
/// program of the reference corpus is estimated at about 500,000 tokens
/// (inactive conditional branches included).
const MAX_EXPANDED_TOKENS: u64 = 4_000_000;

/// Largest accepted nesting of macro invocations inside macro arguments.
const MAX_INVOCATION_NESTING: usize = 64;

/// Largest accepted chain of macros expanding to macros (bounds this module's
/// own recursion; glslang itself handles deeper chains).
const MAX_EXPANSION_CHAIN: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok<'a> {
    Ident(&'a str),
    /// A parameter of the macro whose body this is.
    Param(usize),
    LParen,
    RParen,
    Comma,
    Other,
}

#[derive(Debug)]
struct Macro<'a> {
    /// `None` for object-like macros.
    params: Option<usize>,
    body: Vec<Tok<'a>>,
}

/// Why a source is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hazard {
    TooLarge,
    TooNested,
    ChainTooLong,
}

impl Hazard {
    fn describe(self) -> String {
        match self {
            Self::TooLarge => format!("macro expansion too large to compile safely (more than {MAX_EXPANDED_TOKENS} tokens)"),
            Self::TooNested => {
                format!("macro invocations nested too deeply to compile safely (more than {MAX_INVOCATION_NESTING} levels)")
            }
            Self::ChainTooLong => {
                format!("macros expand to other macros too deeply to compile safely (more than {MAX_EXPANSION_CHAIN} levels)")
            }
        }
    }
}

/// Replace comments by spaces (keeping newlines) so that lines and directives
/// can be split naively.
fn strip_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            out.push(b' ');
            while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                if bytes[i] == b'\n' {
                    out.push(b'\n');
                }
                i += 1;
            }
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    // Only ASCII bytes were removed or inserted, so this is still valid UTF-8.
    String::from_utf8(out).unwrap_or_default()
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Tokenize one logical line (identifiers, parentheses, commas; everything else
/// is one `Other` token per character, which only overestimates sizes).
fn lex<'a>(s: &'a str, line: u32, out: &mut Vec<(Tok<'a>, u32)>) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if is_ident_start(b) {
            let start = i;
            while i < bytes.len() && is_ident_char(bytes[i]) {
                i += 1;
            }
            out.push((Tok::Ident(&s[start..i]), line));
            continue;
        }
        if b.is_ascii_digit() || (b == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            while i < bytes.len() && (is_ident_char(bytes[i]) || bytes[i] == b'.') {
                i += 1;
            }
            out.push((Tok::Other, line));
            continue;
        }
        match b {
            b'(' => out.push((Tok::LParen, line)),
            b')' => out.push((Tok::RParen, line)),
            b',' => out.push((Tok::Comma, line)),
            _ if b.is_ascii_whitespace() => {}
            _ => out.push((Tok::Other, line)),
        }
        i += 1;
    }
}

/// Parse `define NAME[(params)] body` (the text after `#`).
fn parse_define(directive: &str) -> Option<(&str, Macro<'_>)> {
    let rest = directive.trim_start().strip_prefix("define")?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let rest = rest.trim_start();
    let end = rest.bytes().position(|b| !is_ident_char(b)).unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty() || !is_ident_start(name.as_bytes()[0]) {
        return None;
    }
    let mut body_text = &rest[end..];
    let mut params: Option<Vec<&str>> = None;
    if let Some(after) = body_text.strip_prefix('(') {
        let close = after.find(')')?;
        let list: Vec<&str> = after[..close]
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| if p == "..." { "__VA_ARGS__" } else { p })
            .collect();
        params = Some(list);
        body_text = &after[close + 1..];
    }
    let mut toks = Vec::new();
    lex(body_text, 0, &mut toks);
    let body = toks
        .into_iter()
        .map(|(t, _)| match (t, &params) {
            (Tok::Ident(id), Some(ps)) => ps.iter().position(|p| *p == id).map_or(t, Tok::Param),
            _ => t,
        })
        .collect();
    Some((name, Macro { params: params.map(|p| p.len()), body }))
}

/// Split the arguments of an invocation whose `(` is at `open`. Returns the
/// argument ranges and the index after the closing `)`, or `None` if the
/// parentheses are not closed (glslang then reports an error).
fn split_args(toks: &[Tok<'_>], open: usize) -> Option<(Vec<(usize, usize)>, usize)> {
    let mut depth = 0usize;
    let mut args = Vec::new();
    let mut start = open + 1;
    for (i, t) in toks.iter().enumerate().skip(open) {
        match t {
            Tok::LParen => depth += 1,
            Tok::RParen => {
                depth -= 1;
                if depth == 0 {
                    args.push((start, i));
                    return Some((args, i + 1));
                }
            }
            Tok::Comma if depth == 1 => {
                args.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
    }
    None
}

struct Estimator<'m, 'a> {
    macros: &'m HashMap<&'a str, Vec<Macro<'a>>>,
    object_memo: HashMap<&'a str, u64>,
    function_memo: HashMap<(&'a str, Vec<u64>), u64>,
    active: HashSet<&'a str>,
    nesting: usize,
}

impl<'a> Estimator<'_, 'a> {
    /// Expanded size of `toks`, where `Param(i)` stands for `params[i]` tokens.
    fn stream(&mut self, toks: &[Tok<'a>], params: &[u64]) -> Result<u64, Hazard> {
        let mut total = 0u64;
        let mut i = 0;
        while i < toks.len() {
            let (size, next) = self.token(toks, i, params)?;
            total = total.saturating_add(size);
            if total > MAX_EXPANDED_TOKENS {
                return Err(Hazard::TooLarge);
            }
            i = next;
        }
        Ok(total)
    }

    /// Expanded size of the token at `i` (an invocation consumes its
    /// arguments) and the index of the next token.
    fn token(&mut self, toks: &[Tok<'a>], i: usize, params: &[u64]) -> Result<(u64, usize), Hazard> {
        let name = match toks[i] {
            Tok::Param(p) => return Ok((params.get(p).copied().unwrap_or(1), i + 1)),
            Tok::Ident(name) if self.macros.contains_key(name) && !self.active.contains(name) => name,
            _ => return Ok((1, i + 1)),
        };
        let is_function = self.macros[name].iter().any(|m| m.params.is_some());
        let invoked = is_function && toks.get(i + 1) == Some(&Tok::LParen);
        if invoked && let Some((args, next)) = split_args(toks, i + 1) {
            if self.nesting >= MAX_INVOCATION_NESTING {
                return Err(Hazard::TooNested);
            }
            self.nesting += 1;
            let sizes: Result<Vec<u64>, Hazard> = args.iter().map(|&(a, b)| self.stream(&toks[a..b], params)).collect();
            self.nesting -= 1;
            return Ok((self.expand(name, true, sizes?)?, next));
        }
        if self.macros[name].iter().any(|m| m.params.is_none()) {
            return Ok((self.expand(name, false, Vec::new())?, i + 1));
        }
        Ok((1, i + 1))
    }

    /// Expanded size of `name` (its function-like definitions with argument
    /// sizes `args`, or its object-like ones); the largest over all definitions.
    fn expand(&mut self, name: &'a str, function: bool, args: Vec<u64>) -> Result<u64, Hazard> {
        if !function && let Some(&size) = self.object_memo.get(name) {
            return Ok(size);
        }
        let key = (name, args);
        if function && let Some(&size) = self.function_memo.get(&key) {
            return Ok(size);
        }
        if self.active.len() >= MAX_EXPANSION_CHAIN {
            return Err(Hazard::ChainTooLong);
        }
        self.active.insert(name);
        let mut largest = 0u64;
        let mut result = Ok(());
        let macros = self.macros;
        for def in macros.get(name).into_iter().flatten().filter(|d| d.params.is_some() == function) {
            match self.stream(&def.body, &key.1) {
                Ok(size) => largest = largest.max(size),
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        self.active.remove(name);
        result?;
        if function {
            self.function_memo.insert(key, largest);
        } else {
            self.object_memo.insert(name, largest);
        }
        Ok(largest)
    }
}

/// Check a source before glslang preprocesses it. Returns the 1-based line of
/// the offending code and a message when its macros would expand
/// exponentially, too far, or through too deeply nested invocations.
pub(crate) fn macro_hazard(text: &str) -> Option<(u32, String)> {
    if !text.contains("define") {
        return None; // fast path: no macros (ShaderBridge's own output)
    }
    let clean = strip_comments(text);
    // Directives (with `\` continuations joined) and code lines.
    let mut directives: Vec<String> = Vec::new();
    let mut code_lines: Vec<(u32, &str)> = Vec::new();
    let mut lines = clean.split('\n').enumerate();
    while let Some((index, first)) = lines.next() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let Some(directive) = first.trim_start().strip_prefix('#') else {
            code_lines.push((line, first));
            continue;
        };
        let mut joined = String::new();
        let mut piece = directive;
        loop {
            let trimmed = piece.trim_end();
            match trimmed.strip_suffix('\\') {
                Some(head) => {
                    joined.push_str(head);
                    joined.push(' ');
                    match lines.next() {
                        Some((_, more)) => piece = more,
                        None => break,
                    }
                }
                None => {
                    joined.push_str(trimmed);
                    break;
                }
            }
        }
        directives.push(joined);
    }
    let mut macros: HashMap<&str, Vec<Macro<'_>>> = HashMap::new();
    for (name, m) in directives.iter().filter_map(|d| parse_define(d)) {
        macros.entry(name).or_default().push(m);
    }
    if macros.is_empty() {
        return None;
    }
    let mut code: Vec<(Tok<'_>, u32)> = Vec::new();
    for (line, text) in code_lines {
        lex(text, line, &mut code);
    }
    let toks: Vec<Tok<'_>> = code.iter().map(|(t, _)| *t).collect();
    let mut est = Estimator {
        macros: &macros,
        object_memo: HashMap::new(),
        function_memo: HashMap::new(),
        active: HashSet::new(),
        nesting: 0,
    };
    let mut total = 0u64;
    let mut i = 0;
    while i < toks.len() {
        let result = est.token(&toks, i, &[]);
        let (size, next) = match result {
            Ok(r) => r,
            Err(h) => return Some((code[i].1, h.describe())),
        };
        total = total.saturating_add(size);
        if total > MAX_EXPANDED_TOKENS {
            return Some((code[i].1, Hazard::TooLarge.describe()));
        }
        i = next;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hazard(src: &str) -> Option<String> {
        macro_hazard(src).map(|(line, msg)| format!("{line}: {msg}"))
    }

    #[test]
    fn ordinary_macros_are_fine() {
        assert_eq!(hazard("void main() { x = 1; }"), None);
        assert_eq!(hazard("#define A 1\n#define F(x, y) ((x) + (y) * A)\nfloat v = F(F(1, 2), F(A, 3));\n"), None);
        // Self-reference is not expanded again.
        assert_eq!(hazard("#define X X + X\nfloat v = X;\n"), None);
        // Unused large definitions cost nothing.
        let mut src = String::from("#define M0 y\n");
        for i in 1..=40 {
            src.push_str(&format!("#define M{i} M{p} + M{p}\n", p = i - 1));
        }
        assert_eq!(hazard(&src), None);
    }

    #[test]
    fn exponential_object_chains_are_rejected() {
        let mut src = String::from("#define M0 y\n");
        for i in 1..=30 {
            src.push_str(&format!("#define M{i} M{p} + M{p}\n", p = i - 1));
        }
        src.push_str("void main() {\n float x = M30;\n}\n");
        let msg = hazard(&src).unwrap();
        assert!(msg.starts_with("33: macro expansion too large"), "{msg}");
        // 2^17 tokens is within the budget (the depth guard handles the rest).
        let small = src.replace("M30;", "M17;");
        assert_eq!(hazard(&small), None);
    }

    #[test]
    fn duplicating_arguments_are_rejected() {
        let src = format!("#define D(x) x x\nfloat v = {}y{};\n", "D(".repeat(40), ")".repeat(40));
        assert!(hazard(&src).unwrap().contains("too large"));
        // Duplication inside bodies: E applies D ten times (4^10 per E).
        let src = "#define D(x) x x x x\n#define E(x) D(D(D(D(D(D(D(D(D(D(x))))))))))\nfloat v = E(E(1));\n";
        assert!(hazard(src).unwrap().contains("too large"));
        assert_eq!(hazard("#define D(x) x x x x\n#define E(x) D(D(D(x)))\nfloat v = E(1);\n"), None);
    }

    #[test]
    fn deep_invocation_nesting_is_rejected() {
        let src = format!("#define M(x) x\nfloat v = {}1.0{};\n", "M(".repeat(1000), ")".repeat(1000));
        assert!(hazard(&src).unwrap().contains("nested too deeply"));
        let ok = format!("#define M(x) x\nfloat v = {}1.0{};\n", "M(".repeat(60), ")".repeat(60));
        assert_eq!(hazard(&ok), None);
    }

    #[test]
    fn long_definition_chains_are_bounded() {
        let mut src = String::from("#define M0 1.0\n");
        for i in 1..2000 {
            src.push_str(&format!("#define M{i} M{}\n", i - 1));
        }
        src.push_str("float v = M1999;\n");
        assert!(hazard(&src).unwrap().contains("too deeply"));
    }

    #[test]
    fn comments_continuations_and_garbage() {
        // Definitions inside comments do not count; continued directives do.
        let src = "/* #define A B B */\n// #define B A A\n#define F(x) \\\n x x\nfloat v = F(F(1));\n";
        assert_eq!(hazard(src), None);
        let src = format!("#define F(x) \\\n x x \\\n x x\nfloat v = {}1{};\n", "F(".repeat(20), ")".repeat(20));
        assert!(hazard(&src).unwrap().contains("too large"), "continued bodies count");
        for src in ["#define", "#define (", "#define F(x", "#define F(x) x\nF(", "#define F(x) x\nF((((", "#define F(...) __VA_ARGS__\nF(1, 2)"] {
            let _ = hazard(src);
        }
    }

    #[test]
    fn all_definitions_count() {
        // A conditional redefinition might be the active one: take the larger.
        let mut src = String::from("#define M0 y\n");
        for i in 1..=30 {
            src.push_str(&format!("#ifdef BIG\n#define M{i} M{p} + M{p}\n#else\n#define M{i} y\n#endif\n", p = i - 1));
        }
        src.push_str("float x = M30;\n");
        assert!(hazard(&src).unwrap().contains("too large"));
    }
}
