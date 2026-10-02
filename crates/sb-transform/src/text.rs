//! Token-level helpers over GLSL text (comments and numbers are skipped correctly).

/// Byte-level scanner state shared by the helpers below.
struct Scanner<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
}

/// A coarse GLSL token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok<'a> {
    Ident(&'a str),
    Number(&'a str),
    Punct(u8),
    /// Comment or whitespace (`text` is the raw source).
    Trivia(&'a str),
}

impl<'a> Scanner<'a> {
    fn new(s: &'a str) -> Self {
        Self { s, b: s.as_bytes(), i: 0 }
    }

    fn next_tok(&mut self) -> Option<(usize, Tok<'a>)> {
        let b = self.b;
        let start = self.i;
        let c = *b.get(start)?;
        if c == b'/' && b.get(start + 1) == Some(&b'/') {
            let end = b[start..].iter().position(|&x| x == b'\n').map_or(b.len(), |p| start + p);
            self.i = end;
            return Some((start, Tok::Trivia(&self.s[start..end])));
        }
        if c == b'/' && b.get(start + 1) == Some(&b'*') {
            let end = self.s[start + 2..].find("*/").map_or(b.len(), |p| start + 2 + p + 2);
            self.i = end;
            return Some((start, Tok::Trivia(&self.s[start..end])));
        }
        if c.is_ascii_whitespace() {
            let mut j = start;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            self.i = j;
            return Some((start, Tok::Trivia(&self.s[start..j])));
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let mut j = start;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            self.i = j;
            return Some((start, Tok::Ident(&self.s[start..j])));
        }
        if c.is_ascii_digit() || (c == b'.' && b.get(start + 1).is_some_and(u8::is_ascii_digit)) {
            let mut j = start + 1;
            while j < b.len() {
                let d = b[j];
                let exp_sign = (d == b'+' || d == b'-')
                    && matches!(b[j - 1], b'e' | b'E')
                    && !self.s[start..j].starts_with("0x")
                    && !self.s[start..j].starts_with("0X");
                if d.is_ascii_alphanumeric() || d == b'.' || d == b'_' || exp_sign {
                    j += 1;
                } else {
                    break;
                }
            }
            self.i = j;
            return Some((start, Tok::Number(&self.s[start..j])));
        }
        // Any other character (multi-byte UTF-8 included) is punctuation.
        let len = self.s[start..].chars().next().map_or(1, char::len_utf8);
        self.i = start + len;
        Some((start, Tok::Punct(c)))
    }
}

impl<'a> Iterator for Scanner<'a> {
    type Item = (usize, Tok<'a>);
    fn next(&mut self) -> Option<Self::Item> {
        self.next_tok()
    }
}

/// Every identifier token of `text` (comments and number suffixes skipped).
pub(crate) fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    Scanner::new(text).filter_map(|(_, t)| match t {
        Tok::Ident(s) => Some(s),
        _ => None,
    })
}

/// Names of the functions defined at brace depth 0 in `code` (`T name(...) {`).
pub(crate) fn defined_functions(code: &str) -> Vec<String> {
    let toks: Vec<Tok> = Scanner::new(code).map(|(_, t)| t).filter(|t| !matches!(t, Tok::Trivia(_))).collect();
    let mut out = Vec::new();
    let mut depth = 0i32;
    for (k, t) in toks.iter().enumerate() {
        match t {
            Tok::Punct(b'{') => depth += 1,
            Tok::Punct(b'}') => depth -= 1,
            Tok::Ident(name) if depth == 0 => {
                let prev_is_type = k > 0 && matches!(toks[k - 1], Tok::Ident(_));
                if prev_is_type && toks.get(k + 1) == Some(&Tok::Punct(b'(')) {
                    out.push((*name).to_string());
                }
            }
            _ => {}
        }
    }
    out
}

/// Parse an unsuffixed GLSL integer literal (decimal, octal or hex) to its value.
fn int_literal_value(tok: &str) -> Option<u64> {
    if let Some(h) = tok.strip_prefix("0x").or_else(|| tok.strip_prefix("0X")) {
        return u64::from_str_radix(h, 16).ok();
    }
    if !tok.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if tok.len() > 1 && tok.starts_with('0') {
        return u64::from_str_radix(&tok[1..], 8).ok();
    }
    tok.parse::<u64>().ok()
}

/// Work around glsl-lang 0.8 rejecting unsuffixed integer literals in
/// `0x80000000..=0xFFFFFFFF` (legal GLSL: they denote negative `int`s): rewrite them to
/// `int(<literal>u)`. Comments are left untouched and line structure is preserved.
/// Returns the new text and the number of literals rewritten.
pub(crate) fn wrap_large_int_literals(code: &str) -> (String, usize) {
    let mut out = String::new();
    let mut last = 0;
    let mut count = 0;
    for (pos, t) in Scanner::new(code) {
        if let Tok::Number(n) = t
            && let Some(v) = int_literal_value(n)
            && (0x8000_0000..=0xFFFF_FFFF).contains(&v)
        {
            if out.is_empty() {
                out.reserve(code.len() + 64);
            }
            out.push_str(&code[last..pos]);
            out.push_str("int(");
            out.push_str(n);
            out.push_str("u)");
            last = pos + n.len();
            count += 1;
        }
    }
    if count == 0 {
        return (code.to_string(), 0);
    }
    out.push_str(&code[last..]);
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_skip_comments_and_numbers() {
        let ids: Vec<&str> = identifiers("vec4 a = 1.0e5f * b; // c d\n/* e */ f0x").collect();
        assert_eq!(ids, ["vec4", "a", "b", "f0x"]);
    }

    #[test]
    fn functions_at_depth_zero() {
        let f = defined_functions("vec3 foo(uint i) { return bar(i); }\nint baz() { if (x) { g(); } return 1; }");
        assert_eq!(f, ["foo", "baz"]);
    }

    #[test]
    fn large_literals_wrapped() {
        let (s, n) = wrap_large_int_literals("int a = 0x80000000; uint b = 0xFFFFFFFFu; int c = 4294967295; // 0x80000000\nint d = 2147483647; float e = 3e10;");
        assert_eq!(n, 2);
        assert_eq!(
            s,
            "int a = int(0x80000000u); uint b = 0xFFFFFFFFu; int c = int(4294967295u); // 0x80000000\nint d = 2147483647; float e = 3e10;"
        );
        assert_eq!(wrap_large_int_literals("x = 020000000000;").0, "x = int(020000000000u);");
        assert_eq!(wrap_large_int_literals("x = 0x1p+5;").1, 0);
    }
}
