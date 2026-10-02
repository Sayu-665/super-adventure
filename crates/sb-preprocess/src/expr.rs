//! `#if` / `#elif` integer constant-expression evaluation.
//!
//! Works on fully macro-expanded tokens in which `defined` has already been
//! replaced. Semantics follow C (64-bit signed, wrapping arithmetic,
//! short-circuit `&&`, `||` and `?:`, remaining identifiers are 0) with the
//! JCPP (Iris) extension that floating-point literals are accepted and
//! truncated to their integer part (`0.75` -> 0, `1.5e1` -> 10), because packs
//! tested on Iris rely on it.

use crate::intern::Interner;
use crate::lexer::{Kind, Tok};

/// Outcome of evaluating a condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EvalResult {
    pub value: i64,
    /// A floating-point literal was truncated.
    pub used_float: bool,
    /// Tokens left over after a complete expression.
    pub trailing: bool,
    /// Non-fatal errors (division by zero in an evaluated subexpression).
    pub errors: Vec<String>,
    /// Notes about JCPP-compatible but unusual input (e.g. `08` read as decimal).
    pub warnings: Vec<String>,
    /// Integer literals that do not fit in a signed 64-bit integer: JCPP
    /// (`Long.parseLong`) throws on them, so Iris rejects the program.
    pub overflows: Vec<String>,
}

const MAX_DEPTH: u32 = 256;

/// A parsed `#if` number operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Number {
    pub value: i64,
    /// The literal is a floating-point literal (truncated, JCPP semantics).
    pub float: bool,
    /// The literal starts with `0` but is not valid octal (`08`); JCPP reads it
    /// as decimal and warns.
    pub bad_octal: bool,
    /// The integer part does not fit in an `i64` (JCPP throws; the value saturates
    /// or wraps here).
    pub overflow: bool,
}

/// Parse a preprocessing number as an `#if` operand, following JCPP 1.4.14
/// (`LexerSource.number` + `NumericValue.longValue`):
///
/// * integers: decimal, octal (`0` prefix) or hex (`0x`), with any `u`/`l`
///   suffixes; a `0`-prefixed literal with an `8`/`9` digit is decimal (JCPP
///   warns); there are no binary literals (`0b101` is an invalid token for
///   JCPP, and GLSL has none);
/// * floats: `(long) (integer_part * 10^exponent)` — the fraction is dropped;
///   hex floats `0x1.8p3` give `integer_part << exponent`;
/// * anything else (`2FOO`, `1e`) is invalid.
pub(crate) fn parse_number(text: &str) -> Option<Number> {
    let lower = text.to_ascii_lowercase();
    let t = lower.as_str();
    /// `(value, overflow)` of an unsigned digit string, saturating/wrapping like before.
    fn digits(d: &str, radix: u32) -> (i64, bool) {
        match u64::from_str_radix(d, radix) {
            Ok(v) => (v as i64, v > i64::MAX as u64),
            Err(_) => (i64::MAX, true),
        }
    }
    if let Some(h) = t.strip_prefix("0x") {
        let d = h.trim_end_matches(['u', 'l']);
        if !d.is_empty() && d.bytes().all(|c| c.is_ascii_hexdigit()) {
            let (value, overflow) = digits(d, 16);
            return Some(Number {
                value,
                float: false,
                bad_octal: false,
                overflow,
            });
        }
        // Hex float: hexdigits [. hexdigits] p [+-] digits [f|l|d]
        let f = h.trim_end_matches(['f', 'l', 'd']);
        let (mantissa, exp) = f.split_once('p')?;
        let ip = mantissa.split_once('.').map_or(mantissa, |(i, _)| i);
        let fp = mantissa.split_once('.').map_or("", |(_, f)| f);
        if !ip.bytes().all(|c| c.is_ascii_hexdigit())
            || !fp.bytes().all(|c| c.is_ascii_hexdigit())
            || (ip.is_empty() && fp.is_empty())
        {
            return None;
        }
        let (base, overflow) = if ip.is_empty() { (0, false) } else { digits(ip, 16) };
        let e: i32 = exp.parse().ok()?;
        // Java `long << int` masks the shift count to 6 bits.
        return Some(Number {
            value: base.wrapping_shl((e & 63) as u32),
            float: true,
            bad_octal: false,
            overflow,
        });
    }
    // Decimal / octal integer with optional u/l suffixes.
    let int_part = t.trim_end_matches(['u', 'l']);
    if !int_part.is_empty() && int_part.bytes().all(|c| c.is_ascii_digit()) {
        let octal = int_part.starts_with('0') && int_part.bytes().all(|c| c < b'8');
        let bad_octal = int_part.starts_with('0') && !octal;
        let (value, overflow) = digits(int_part, if octal { 8 } else { 10 });
        return Some(Number {
            value,
            float: false,
            bad_octal,
            overflow,
        });
    }
    // Floating-point literal: digits [. digits] [e [+-] digits] [suffixes]
    // (JCPP truncation semantics; JCPP accepts any mix of u/l/i/f/d suffixes).
    let f = t.trim_end_matches(['f', 'd', 'l', 'u', 'i']);
    let (mantissa, exp) = match f.split_once('e') {
        Some((m, e)) => (m, Some(e)),
        None => (f, None),
    };
    let (ip, fp) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if !ip.bytes().all(|c| c.is_ascii_digit())
        || !fp.bytes().all(|c| c.is_ascii_digit())
        || (ip.is_empty() && fp.is_empty())
    {
        return None;
    }
    let (base, overflow) = if ip.is_empty() { (0, false) } else { digits(ip, 10) };
    let value = match exp {
        Some(e) => {
            let e: i32 = e.parse().ok()?;
            // JCPP: (long) (integer_part * Math.pow(10, exponent))
            let v = (base as f64) * 10f64.powi(e);
            if v.is_finite() { v as i64 } else { i64::MAX }
        }
        None => base,
    };
    Some(Number {
        value,
        float: true,
        bad_octal: false,
        overflow,
    })
}

/// Value of a character literal token (`'a'`, `'\n'`, `'\101'`, `'\x41'`).
/// Like JCPP, exactly one (possibly escaped) character is allowed; empty and
/// multi-character literals are invalid tokens.
fn char_value(text: &str) -> Option<i64> {
    let inner = text.strip_prefix('\'')?.strip_suffix('\'')?;
    let mut chars = inner.chars().peekable();
    let value = match chars.next()? {
        '\\' => match chars.next()? {
            'a' => 7,
            'b' => 8,
            'f' => 12,
            'n' => 10,
            'r' => 13,
            't' => 9,
            'v' => 11,
            c @ '0'..='7' => {
                let mut v = i64::from(c as u8 - b'0');
                for _ in 0..2 {
                    match chars.peek() {
                        Some(&d @ '0'..='7') => {
                            v = (v << 3) + i64::from(d as u8 - b'0');
                            chars.next();
                        }
                        _ => break,
                    }
                }
                v
            }
            'x' => {
                let mut v: i64 = 0;
                let mut any = false;
                while let Some(d) = chars.peek().and_then(|c| c.to_digit(16)) {
                    v = v.wrapping_shl(4) | i64::from(d);
                    any = true;
                    chars.next();
                }
                if !any {
                    return None;
                }
                v
            }
            c => c as i64,
        },
        c => c as i64,
    };
    chars.next().is_none().then_some(value)
}

struct Parser<'t> {
    toks: Vec<(Kind, &'t str)>,
    pos: usize,
    used_float: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
    overflows: Vec<String>,
    depth: u32,
}

type PResult = Result<i64, String>;

impl<'t> Parser<'t> {
    fn peek(&self) -> Option<&'t str> {
        self.toks.get(self.pos).map(|(_, t)| *t)
    }

    fn peek_op(&self) -> Option<&'t str> {
        self.toks
            .get(self.pos)
            .filter(|(k, _)| *k == Kind::Punct)
            .map(|(_, t)| *t)
    }

    fn eat(&mut self, op: &str) -> bool {
        if self.peek_op() == Some(op) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err("expression nesting too deep".into())
        } else {
            Ok(())
        }
    }

    fn cond(&mut self, live: bool) -> PResult {
        self.enter()?;
        let c = self.binary(0, live)?;
        let r = if self.eat("?") {
            let t = self.cond(live && c != 0)?;
            if !self.eat(":") {
                return Err(format!(
                    "expected ':' in conditional expression, found {}",
                    self.describe()
                ));
            }
            let f = self.cond(live && c == 0)?;
            if c != 0 { t } else { f }
        } else {
            c
        };
        self.depth -= 1;
        Ok(r)
    }

    fn describe(&self) -> String {
        match self.peek() {
            Some(t) => format!("'{t}'"),
            None => "end of line".into(),
        }
    }

    fn prec(op: &str) -> Option<u8> {
        Some(match op {
            "||" => 1,
            "&&" => 2,
            "|" => 3,
            "^" => 4,
            "&" => 5,
            "==" | "!=" => 6,
            "<" | ">" | "<=" | ">=" => 7,
            "<<" | ">>" => 8,
            "+" | "-" => 9,
            "*" | "/" | "%" => 10,
            _ => return None,
        })
    }

    /// Precedence climbing for left-associative binary operators.
    fn binary(&mut self, min: u8, live: bool) -> PResult {
        let mut lhs = self.unary(live)?;
        while let Some(op) = self.peek_op() {
            let Some(p) = Self::prec(op) else { break };
            if p <= min {
                break;
            }
            self.pos += 1;
            let rhs_live = match op {
                "&&" => live && lhs != 0,
                "||" => live && lhs == 0,
                _ => live,
            };
            let rhs = self.binary(p, rhs_live)?;
            lhs = self.apply(op, lhs, rhs, live);
        }
        Ok(lhs)
    }

    fn apply(&mut self, op: &str, a: i64, b: i64, live: bool) -> i64 {
        let bool_i = |x: bool| i64::from(x);
        match op {
            "||" => bool_i(a != 0 || b != 0),
            "&&" => bool_i(a != 0 && b != 0),
            "|" => a | b,
            "^" => a ^ b,
            "&" => a & b,
            "==" => bool_i(a == b),
            "!=" => bool_i(a != b),
            "<" => bool_i(a < b),
            ">" => bool_i(a > b),
            "<=" => bool_i(a <= b),
            ">=" => bool_i(a >= b),
            // Java semantics (JCPP): shift count masked to 6 bits.
            "<<" => a.wrapping_shl((b & 63) as u32),
            ">>" => a.wrapping_shr((b & 63) as u32),
            "+" => a.wrapping_add(b),
            "-" => a.wrapping_sub(b),
            "*" => a.wrapping_mul(b),
            "/" | "%" => {
                if b == 0 {
                    if live {
                        self.errors.push(if op == "/" {
                            "division by zero".into()
                        } else {
                            "modulo by zero".into()
                        });
                    }
                    0
                } else if op == "/" {
                    a.wrapping_div(b)
                } else {
                    a.wrapping_rem(b)
                }
            }
            _ => 0,
        }
    }

    fn unary(&mut self, live: bool) -> PResult {
        self.enter()?;
        let r = match self.peek_op() {
            Some("+") => {
                self.pos += 1;
                self.unary(live)?
            }
            Some("-") => {
                self.pos += 1;
                self.unary(live)?.wrapping_neg()
            }
            Some("~") => {
                self.pos += 1;
                !self.unary(live)?
            }
            Some("!") => {
                self.pos += 1;
                i64::from(self.unary(live)? == 0)
            }
            _ => self.primary(live)?,
        };
        self.depth -= 1;
        Ok(r)
    }

    fn primary(&mut self, live: bool) -> PResult {
        let Some(&(kind, text)) = self.toks.get(self.pos) else {
            return Err("expected a value, found end of line".into());
        };
        self.pos += 1;
        match kind {
            Kind::Number => match parse_number(text) {
                Some(n) => {
                    self.used_float |= n.float;
                    if n.overflow {
                        self.overflows.push(format!(
                            "integer constant '{text}' does not fit in 64 bits (Iris/JCPP rejects the program)"
                        ));
                    }
                    if n.bad_octal {
                        self.warnings.push(format!(
                            "decimal constant '{text}' starts with 0 but is not octal; read as decimal (Iris/JCPP)"
                        ));
                    }
                    Ok(n.value)
                }
                None => Err(format!(
                    "invalid number '{text}' in preprocessor expression"
                )),
            },
            Kind::Str if text.starts_with('\'') => char_value(text).ok_or_else(|| {
                format!("invalid character constant {text} in preprocessor expression")
            }),
            // Identifiers left after macro expansion evaluate to 0 (incl. `true`/`false`, as in C and JCPP).
            Kind::Ident => Ok(0),
            Kind::Punct if text == "(" => {
                let v = self.cond(live)?;
                if !self.eat(")") {
                    return Err(format!(
                        "expected ')' in preprocessor expression, found {}",
                        self.describe()
                    ));
                }
                Ok(v)
            }
            _ => Err(format!("unexpected '{text}' in preprocessor expression")),
        }
    }
}

/// Evaluate an expression given as (non-whitespace) tokens.
pub(crate) fn evaluate(toks: &[Tok], int: &Interner) -> Result<EvalResult, String> {
    let toks: Vec<(Kind, &str)> = toks
        .iter()
        .filter(|t| !t.kind.is_white() && t.kind != Kind::Placemarker)
        .map(|t| (t.kind, int.get(t.sym)))
        .collect();
    if toks.is_empty() {
        return Err("#if with no expression".into());
    }
    let mut p = Parser {
        toks,
        pos: 0,
        used_float: false,
        errors: Vec::new(),
        warnings: Vec::new(),
        overflows: Vec::new(),
        depth: 0,
    };
    let value = p.cond(true)?;
    let trailing = p.pos < p.toks.len();
    Ok(EvalResult {
        value,
        used_float: p.used_float,
        trailing,
        errors: p.errors,
        warnings: p.warnings,
        overflows: p.overflows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{LexState, lex_line};

    fn eval(s: &str) -> Result<EvalResult, String> {
        let mut int = Interner::new();
        let mut v = Vec::new();
        lex_line(s, LexState::Normal, &mut int, &mut v);
        evaluate(&v, &int)
    }

    fn val(s: &str) -> i64 {
        eval(s).unwrap_or_else(|e| panic!("{s}: {e}")).value
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(val("1 + 2 * 3"), 7);
        assert_eq!(val("(1 + 2) * 3"), 9);
        assert_eq!(val("10 - 4 - 3"), 3);
        assert_eq!(val("100 / 10 / 5"), 2);
        assert_eq!(val("7 % 4"), 3);
        assert_eq!(val("-7 / 2"), -3);
        assert_eq!(val("1 << 4 >> 2"), 4);
        assert_eq!(val("1 < 2 == 1"), 1);
        assert_eq!(val("6 & 3 | 8 ^ 1"), 2 | (8 ^ 1));
        assert_eq!(val("~0"), -1);
        assert_eq!(val("!0 + !5"), 1);
        assert_eq!(val("- - 3"), 3);
        assert_eq!(val("+3"), 3);
        assert_eq!(val("1 || 0 && 0"), 1);
        assert_eq!(val("2 >= 2 && 3 <= 2"), 0);
        assert_eq!(val("5 != 5"), 0);
    }

    #[test]
    fn ternary() {
        assert_eq!(val("1 ? 2 : 3"), 2);
        assert_eq!(val("0 ? 2 : 3"), 3);
        assert_eq!(val("0 ? 1 : 0 ? 2 : 3"), 3);
        assert_eq!(val("1 ? 0 ? 5 : 6 : 7"), 6);
        assert_eq!(val("(1 ? 2 : 3) + 1"), 3);
        assert_eq!(val("1 + 1 ? 10 : 20"), 10);
    }

    #[test]
    fn short_circuit_suppresses_div_by_zero() {
        let r = eval("0 && (1 / 0)").expect("ok");
        assert_eq!(r.value, 0);
        assert!(r.errors.is_empty());
        let r = eval("1 || (1 % 0)").expect("ok");
        assert!(r.errors.is_empty());
        let r = eval("1 ? 2 : 1/0").expect("ok");
        assert!(r.errors.is_empty());
        let r = eval("1 / 0").expect("ok");
        assert_eq!(r.value, 0);
        assert_eq!(r.errors.len(), 1);
    }

    #[test]
    fn numbers() {
        assert_eq!(val("0x10"), 16);
        assert_eq!(val("0X1fu"), 31);
        assert_eq!(val("010"), 8);
        assert_eq!(val("0"), 0);
        assert_eq!(val("10u + 2UL"), 12);
        assert_eq!(val("'A'"), 65);
        assert_eq!(val("'\\n'"), 10);
        assert_eq!(val("0xFFFFFFFFFFFFFFFF"), -1);
        assert!(eval("2FOO").is_err());
    }

    /// Regression (review): number forms checked against JCPP 1.4.14, the
    /// preprocessor Iris uses.
    #[test]
    fn numbers_follow_jcpp() {
        // `08`/`09` are read as decimal with a warning (JCPP: "Decimal
        // constant starts with 0, but not octal").
        let r = eval("08 == 8").expect("ok");
        assert_eq!(r.value, 1);
        assert_eq!(r.warnings.len(), 1);
        assert_eq!(val("0789"), 789);
        assert!(eval("07").expect("ok").warnings.is_empty());
        // No binary literals: JCPP treats `0b101` as an invalid token (and GLSL has none).
        assert!(eval("0b101").is_err());
        assert!(eval("0b101 == 5").is_err());
        // Hex floats: integer part shifted by the binary exponent.
        assert_eq!(val("0x1p3"), 8);
        assert_eq!(val("0x1.8p1"), 2);
        assert!(eval("0x1p3").expect("ok").used_float);
        assert!(eval("0x").is_err());
        assert!(eval("0xg").is_err());
        assert!(eval("1e").is_err());
        // Any mix of JCPP suffixes on floats.
        assert_eq!(val("2.5u"), 2);
        assert_eq!(val("2f"), 2);
        // Overflowing literals saturate instead of panicking, and are flagged
        // (JCPP's Long.parseLong throws on them).
        let r = eval("99999999999999999999").expect("ok");
        assert_eq!(r.value, i64::MAX);
        assert_eq!(r.overflows.len(), 1);
        assert_eq!(eval("0xFFFFFFFFFFFFFFFF").expect("ok").overflows.len(), 1);
        assert!(eval("0x7FFFFFFFFFFFFFFF").expect("ok").overflows.is_empty());
        assert_eq!(eval("99999999999999999999.5").expect("ok").overflows.len(), 1);
        // A huge exponent saturates in Java too ((long) Infinity), no overflow.
        let r = eval("1e400").expect("ok");
        assert_eq!(r.value, i64::MAX);
        assert!(r.overflows.is_empty());
    }

    /// Regression (review): JCPP accepts exactly one (possibly escaped)
    /// character in a character constant; `'ab'` and `''` are invalid tokens.
    #[test]
    fn char_constants_follow_jcpp() {
        assert_eq!(val("'\\101'"), 65);
        assert_eq!(val("'\\x41'"), 65);
        assert_eq!(val("'\\0'"), 0);
        assert_eq!(val("'\\\\'"), 92);
        assert_eq!(val("'\\''"), 39);
        assert!(eval("'ab'").is_err());
        assert!(eval("''").is_err());
        assert!(eval("'\\x'").is_err());
        assert!(eval("'a").is_err());
    }

    #[test]
    fn floats_truncate_like_jcpp() {
        let r = eval("0.75").expect("ok");
        assert_eq!(r.value, 0);
        assert!(r.used_float);
        assert_eq!(val("1.5"), 1);
        assert_eq!(val("1.5 > 1"), 0);
        assert_eq!(val("1e3"), 1000);
        assert_eq!(val("1.5e1"), 10);
        assert_eq!(val("2.0f"), 2);
        assert_eq!(val(".5"), 0);
        assert_eq!(val("3.0lf"), 3);
    }

    #[test]
    fn identifiers_are_zero() {
        assert_eq!(val("FOO"), 0);
        assert_eq!(val("true"), 0);
        assert_eq!(val("FOO == 0"), 1);
    }

    #[test]
    fn errors() {
        assert!(eval("").is_err());
        assert!(eval("1 +").is_err());
        assert!(eval("(1").is_err());
        assert!(eval("1 ? 2").is_err());
        assert!(eval(")").is_err());
        assert!(eval("\"str\"").is_err());
        let r = eval("1 2").expect("ok");
        assert!(r.trailing);
        let deep = "(".repeat(5000) + "1" + &")".repeat(5000);
        assert!(eval(&deep).is_err());
        let neg = "-".repeat(5000) + "1";
        assert!(eval(&neg).is_err());
    }

    #[test]
    fn shifts_wrap_like_java() {
        assert_eq!(val("1 << 64"), 1);
        assert_eq!(val("1 << 65"), 2);
        assert_eq!(val("-8 >> 1"), -4);
    }
}
