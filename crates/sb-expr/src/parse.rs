//! Lexing and parsing of expressions into an untyped syntax tree ([`Expr`]).
//!
//! Grammar (lowest to highest precedence, all binary operators left-associative,
//! matching OptiFine and C):
//!
//! ```text
//! expr    := or
//! or      := and ( "||" and )*
//! and     := eq ( "&&" eq )*
//! eq      := rel ( ("==" | "!=" | "≠") rel )*
//! rel     := add ( ("<" | ">" | "<=" | ">=" | "≤" | "≥") add )*
//! add     := mul ( ("+" | "-") mul )*
//! mul     := unary ( ("*" | "/" | "%") unary )*
//! unary   := ("-" | "!" | "+") unary | postfix
//! postfix := primary ( "." member )*
//! primary := number | "true" | "false" | ident | ident "(" args? ")" | "(" expr ")"
//! ```
//!
//! Numbers: decimal integers, octal integers with a leading `0` (as in Iris and
//! GLSL), hex `0x1F`, binary `0b101`, floats such as `1.5`, `.5`, `2.`, `1e-3`,
//! with an optional `f`/`F` suffix. Integer literals that overflow `i32` become
//! floats. Trailing `;` characters are ignored (some packs end lines with one).

use std::collections::HashSet;
use std::fmt;

/// Maximum nesting depth of an expression tree. Deeper inputs are rejected with
/// [`ExprErrorKind::TooComplex`] so that compiling and evaluating them cannot
/// exhaust the stack.
pub const MAX_DEPTH: usize = 128;

/// A half-open byte range `start..end` in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    /// First byte of the range.
    pub start: usize,
    /// One past the last byte of the range.
    pub end: usize,
}

impl Span {
    /// The range `start..end`.
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// A node of the untyped syntax tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    /// What the expression is.
    pub kind: ExprKind,
    /// Source range covered by this expression (parentheses included).
    pub span: Span,
}

/// The kind of an [`Expr`].
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    /// Integer literal.
    Int(i32),
    /// Floating-point literal.
    Float(f32),
    /// `true` / `false`.
    Bool(bool),
    /// An identifier (builtin uniform, custom uniform/variable, constant or option).
    Ident(String),
    /// A prefix operator applied to `operand`.
    Unary {
        /// The operator.
        op: UnaryOp,
        /// Its operand.
        operand: Box<Expr>,
    },
    /// A binary operator.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// A function call `name(args...)`.
    Call {
        /// Function name.
        name: String,
        /// Source range of the name.
        name_span: Span,
        /// Arguments in order.
        args: Vec<Expr>,
    },
    /// Member access `base.member` (vector component, swizzle or matrix column).
    Member {
        /// The accessed expression.
        base: Box<Expr>,
        /// The text after the dot (`x`, `xyz`, `0`, ...).
        member: String,
        /// Source range of the member text.
        member_span: Span,
    },
}

/// Prefix operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// `-x`
    Neg,
    /// `!x`
    Not,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `*`
    Mul,
    /// `/` (always a float division)
    Div,
    /// `%`
    Rem,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=` or `≤`
    Le,
    /// `>=` or `≥`
    Ge,
    /// `==`
    Eq,
    /// `!=` or `≠`
    Ne,
    /// `&&`
    And,
    /// `||`
    Or,
}

impl BinaryOp {
    /// Binding strength (higher binds tighter).
    pub const fn precedence(self) -> u8 {
        match self {
            BinaryOp::Or => 1,
            BinaryOp::And => 2,
            BinaryOp::Eq | BinaryOp::Ne => 3,
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => 4,
            BinaryOp::Add | BinaryOp::Sub => 5,
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => 6,
        }
    }

    /// The ASCII spelling of the operator.
    pub const fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Lt => "<",
            BinaryOp::Gt => ">",
            BinaryOp::Le => "<=",
            BinaryOp::Ge => ">=",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::And => "&&",
            BinaryOp::Or => "||",
        }
    }
}

impl UnaryOp {
    /// The spelling of the operator.
    pub const fn symbol(self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
        }
    }
}

/// Category of an [`ExprError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExprErrorKind {
    /// Malformed text.
    Syntax,
    /// An identifier that is not a known uniform, variable or constant.
    UnknownIdentifier,
    /// A call to an unknown function.
    UnknownFunction,
    /// A known function called with the wrong number of arguments.
    Arity,
    /// Operand or argument types that do not fit.
    Type,
    /// The expression is nested too deeply.
    TooComplex,
}

impl ExprErrorKind {
    /// Stable diagnostic code (`expr.syntax`, `expr.type`, ...).
    pub const fn code(self) -> &'static str {
        match self {
            ExprErrorKind::Syntax => "expr.syntax",
            ExprErrorKind::UnknownIdentifier => "expr.unknown-identifier",
            ExprErrorKind::UnknownFunction => "expr.unknown-function",
            ExprErrorKind::Arity => "expr.arity",
            ExprErrorKind::Type => "expr.type",
            ExprErrorKind::TooComplex => "expr.too-complex",
        }
    }
}

/// An error while parsing or type-checking an expression, with the byte offset in
/// the source text where it was detected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} (at byte {offset})")]
pub struct ExprError {
    /// Byte offset into the expression source.
    pub offset: usize,
    /// Human-readable description.
    pub message: String,
    /// Error category.
    pub kind: ExprErrorKind,
}

impl ExprError {
    /// Create an error.
    pub fn new(kind: ExprErrorKind, offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
            kind,
        }
    }

    pub(crate) fn syntax(offset: usize, message: impl Into<String>) -> Self {
        Self::new(ExprErrorKind::Syntax, offset, message)
    }

    /// The 1-based character column of [`offset`](Self::offset) in `src`.
    pub fn column(&self, src: &str) -> usize {
        let mut end = self.offset.min(src.len());
        while !src.is_char_boundary(end) {
            end -= 1;
        }
        src[..end].chars().count() + 1
    }
}

/// Parse an expression.
///
/// ```
/// let e = sb_expr::parse("a + b * -c.x").unwrap();
/// assert_eq!(e.to_string(), "(a + (b * (-c.x)))");
/// let err = sb_expr::parse("1 +").unwrap_err();
/// assert_eq!(err.offset, 3);
/// ```
pub fn parse(src: &str) -> Result<Expr, ExprError> {
    let mut p = Parser {
        src,
        pos: 0,
        nesting: 0,
    };
    p.skip_ws();
    if p.at_end() || p.peek() == Some(';') {
        return Err(ExprError::syntax(p.pos, "empty expression"));
    }
    let (e, _) = p.expr(0)?;
    p.skip_ws();
    while p.peek() == Some(';') {
        p.pos += 1;
        p.skip_ws();
    }
    if let Some(c) = p.peek() {
        let msg = match c {
            ')' => "unmatched ')'".to_string(),
            ',' => "unexpected ',' outside of a function call".to_string(),
            _ => format!("unexpected {} after the end of the expression", describe(c)),
        };
        return Err(ExprError::syntax(p.pos, msg));
    }
    Ok(e)
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    nesting: usize,
}

fn describe(c: char) -> String {
    format!("character {c:?}")
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_ident_part(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_at(&self, byte_offset: usize) -> Option<char> {
        self.src
            .get(self.pos + byte_offset..)
            .and_then(|s| s.chars().next())
    }

    fn at_end(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn enter(&mut self) -> Result<(), ExprError> {
        self.nesting += 1;
        if self.nesting > MAX_DEPTH {
            return Err(ExprError::new(
                ExprErrorKind::TooComplex,
                self.pos,
                format!("expression is nested too deeply (limit {MAX_DEPTH})"),
            ));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.nesting -= 1;
    }

    fn check_depth(&self, depth: usize, offset: usize) -> Result<(), ExprError> {
        if depth > MAX_DEPTH {
            return Err(ExprError::new(
                ExprErrorKind::TooComplex,
                offset,
                format!("expression is nested too deeply (limit {MAX_DEPTH})"),
            ));
        }
        Ok(())
    }

    /// Recognize a binary operator at the current position.
    fn peek_binop(&self) -> Result<Option<(BinaryOp, usize)>, ExprError> {
        const OPS: &[(&str, BinaryOp)] = &[
            ("&&", BinaryOp::And),
            ("||", BinaryOp::Or),
            ("==", BinaryOp::Eq),
            ("!=", BinaryOp::Ne),
            ("<=", BinaryOp::Le),
            (">=", BinaryOp::Ge),
            ("≠", BinaryOp::Ne),
            ("≤", BinaryOp::Le),
            ("≥", BinaryOp::Ge),
            ("<", BinaryOp::Lt),
            (">", BinaryOp::Gt),
            ("+", BinaryOp::Add),
            ("-", BinaryOp::Sub),
            ("*", BinaryOp::Mul),
            ("/", BinaryOp::Div),
            ("%", BinaryOp::Rem),
        ];
        let rest = self.rest();
        if let Some((s, op)) = OPS.iter().find(|(s, _)| rest.starts_with(s)) {
            return Ok(Some((*op, s.len())));
        }
        let hint = match rest.chars().next() {
            Some('&') => Some("'&' is not an operator; did you mean '&&'?"),
            Some('|') => Some("'|' is not an operator; did you mean '||'?"),
            Some('=') => Some("'=' is not an operator; did you mean '=='?"),
            _ => None,
        };
        match hint {
            Some(h) => Err(ExprError::syntax(self.pos, h)),
            None => Ok(None),
        }
    }

    /// Precedence climbing. Returns the expression and its tree depth.
    fn expr(&mut self, min_prec: u8) -> Result<(Expr, usize), ExprError> {
        self.enter()?;
        let (mut lhs, mut depth) = self.unary()?;
        loop {
            self.skip_ws();
            let Some((op, len)) = self.peek_binop()? else {
                break;
            };
            let prec = op.precedence();
            if prec < min_prec {
                break;
            }
            let op_pos = self.pos;
            self.pos += len;
            let (rhs, rdepth) = self.expr(prec + 1)?;
            depth = 1 + depth.max(rdepth);
            self.check_depth(depth, op_pos)?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        self.leave();
        Ok((lhs, depth))
    }

    fn unary(&mut self) -> Result<(Expr, usize), ExprError> {
        self.skip_ws();
        let start = self.pos;
        let op = match self.peek() {
            Some('-') => Some(Some(UnaryOp::Neg)),
            Some('!') if self.peek_at(1) != Some('=') => Some(Some(UnaryOp::Not)),
            Some('+') => Some(None),
            _ => None,
        };
        let Some(op) = op else { return self.postfix() };
        self.pos += 1;
        self.enter()?;
        let (operand, depth) = self.unary()?;
        self.leave();
        match op {
            None => Ok((operand, depth)),
            Some(op) => {
                let depth = depth + 1;
                self.check_depth(depth, start)?;
                let span = Span::new(start, operand.span.end);
                Ok((
                    Expr {
                        kind: ExprKind::Unary {
                            op,
                            operand: Box::new(operand),
                        },
                        span,
                    },
                    depth,
                ))
            }
        }
    }

    fn postfix(&mut self) -> Result<(Expr, usize), ExprError> {
        let (mut e, mut depth) = self.primary()?;
        loop {
            let save = self.pos;
            self.skip_ws();
            if self.peek() != Some('.') {
                self.pos = save;
                break;
            }
            let dot = self.pos;
            self.pos += 1;
            self.skip_ws();
            let mstart = self.pos;
            while self.peek().is_some_and(is_ident_part) {
                self.pos += 1;
            }
            if mstart == self.pos {
                let msg = match self.peek() {
                    Some(c) => {
                        format!("expected a component name after '.', found {}", describe(c))
                    }
                    None => "expected a component name after '.'".to_string(),
                };
                return Err(ExprError::syntax(self.pos, msg));
            }
            depth += 1;
            self.check_depth(depth, dot)?;
            let member = self.src[mstart..self.pos].to_string();
            let span = Span::new(e.span.start, self.pos);
            e = Expr {
                kind: ExprKind::Member {
                    base: Box::new(e),
                    member,
                    member_span: Span::new(mstart, self.pos),
                },
                span,
            };
        }
        Ok((e, depth))
    }

    fn primary(&mut self) -> Result<(Expr, usize), ExprError> {
        self.skip_ws();
        let start = self.pos;
        let Some(c) = self.peek() else {
            return Err(ExprError::syntax(
                start,
                "unexpected end of expression; expected a value",
            ));
        };
        if c.is_ascii_digit() || (c == '.' && self.peek_at(1).is_some_and(|d| d.is_ascii_digit())) {
            return self.number();
        }
        if is_ident_start(c) {
            while self.peek().is_some_and(is_ident_part) {
                self.pos += 1;
            }
            let name = &self.src[start..self.pos];
            let name_span = Span::new(start, self.pos);
            match name {
                "true" => {
                    return Ok((
                        Expr {
                            kind: ExprKind::Bool(true),
                            span: name_span,
                        },
                        1,
                    ));
                }
                "false" => {
                    return Ok((
                        Expr {
                            kind: ExprKind::Bool(false),
                            span: name_span,
                        },
                        1,
                    ));
                }
                _ => {}
            }
            let save = self.pos;
            self.skip_ws();
            if self.peek() == Some('(') {
                let open = self.pos;
                self.pos += 1;
                let (args, depth) = self.call_args(open)?;
                self.check_depth(depth, start)?;
                let kind = ExprKind::Call {
                    name: name.to_string(),
                    name_span,
                    args,
                };
                return Ok((
                    Expr {
                        kind,
                        span: Span::new(start, self.pos),
                    },
                    depth,
                ));
            }
            self.pos = save;
            return Ok((
                Expr {
                    kind: ExprKind::Ident(name.to_string()),
                    span: name_span,
                },
                1,
            ));
        }
        if c == '(' {
            self.pos += 1;
            self.skip_ws();
            if self.peek() == Some(')') {
                return Err(ExprError::syntax(self.pos, "empty parentheses"));
            }
            let (inner, depth) = self.expr(0)?;
            self.skip_ws();
            return match self.peek() {
                Some(')') => {
                    self.pos += 1;
                    Ok((
                        Expr {
                            kind: inner.kind,
                            span: Span::new(start, self.pos),
                        },
                        depth,
                    ))
                }
                Some(',') => Err(ExprError::syntax(
                    self.pos,
                    "unexpected ',' in parentheses (not a function call)",
                )),
                Some(other) => Err(ExprError::syntax(
                    self.pos,
                    format!("expected ')', found {}", describe(other)),
                )),
                None => Err(ExprError::syntax(start, "unclosed '('")),
            };
        }
        let msg = match c {
            ')' => "expected a value, found ')'".to_string(),
            ',' => "expected a value, found ','".to_string(),
            _ => format!("expected a value, found {}", describe(c)),
        };
        Err(ExprError::syntax(start, msg))
    }

    fn call_args(&mut self, open: usize) -> Result<(Vec<Expr>, usize), ExprError> {
        let mut args = Vec::new();
        let mut depth = 0;
        self.skip_ws();
        if self.peek() == Some(')') {
            self.pos += 1;
            return Ok((args, 1));
        }
        loop {
            let (arg, d) = self.expr(0)?;
            depth = depth.max(d);
            args.push(arg);
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.pos += 1;
                    self.skip_ws();
                    match self.peek() {
                        // Iris accepts a trailing comma.
                        Some(')') => {
                            self.pos += 1;
                            break;
                        }
                        None => {
                            return Err(ExprError::syntax(open, "unclosed '(' of function call"));
                        }
                        Some(_) => {}
                    }
                }
                Some(')') => {
                    self.pos += 1;
                    break;
                }
                Some(c) => {
                    return Err(ExprError::syntax(
                        self.pos,
                        format!("expected ',' or ')', found {}", describe(c)),
                    ));
                }
                None => return Err(ExprError::syntax(open, "unclosed '(' of function call")),
            }
        }
        Ok((args, depth + 1))
    }

    fn number(&mut self) -> Result<(Expr, usize), ExprError> {
        let start = self.pos;
        let bytes = self.src.as_bytes();
        let at = |i: usize| bytes.get(i).copied();
        let is_digit = |i: usize| at(i).is_some_and(|b| b.is_ascii_digit());
        let lit = |kind: ExprKind, end: usize| {
            Ok((
                Expr {
                    kind,
                    span: Span::new(start, end),
                },
                1,
            ))
        };

        // Hexadecimal / binary integers.
        if at(start) == Some(b'0') && matches!(at(start + 1), Some(b'x' | b'X' | b'b' | b'B')) {
            let radix = if matches!(at(start + 1), Some(b'x' | b'X')) {
                16
            } else {
                2
            };
            let digits = start + 2;
            let mut i = digits;
            while at(i).is_some_and(|b| (b as char).is_digit(radix)) {
                i += 1;
            }
            if i == digits || at(i).is_some_and(|b| is_ident_part(b as char)) {
                return Err(ExprError::syntax(
                    start,
                    format!("malformed number literal {:?}", self.literal_text(start)),
                ));
            }
            let v = u32::from_str_radix(&self.src[digits..i], radix)
                .map_err(|_| ExprError::syntax(start, "integer literal is too large"))?;
            self.pos = i;
            // Like GLSL, a 32-bit pattern above i32::MAX wraps to a negative int.
            return lit(ExprKind::Int(v as i32), i);
        }

        let mut i = start;
        while is_digit(i) {
            i += 1;
        }
        let mut is_float = false;
        if at(i) == Some(b'.') {
            is_float = true;
            i += 1;
            while is_digit(i) {
                i += 1;
            }
        }
        if matches!(at(i), Some(b'e' | b'E')) {
            let mut j = i + 1;
            if matches!(at(j), Some(b'+' | b'-')) {
                j += 1;
            }
            if !is_digit(j) {
                return Err(ExprError::syntax(
                    start,
                    format!("malformed exponent in {:?}", self.literal_text(start)),
                ));
            }
            while is_digit(j) {
                j += 1;
            }
            is_float = true;
            i = j;
        }
        let text_end = i;
        if matches!(at(i), Some(b'f' | b'F')) {
            is_float = true;
            i += 1;
        }
        if at(i).is_some_and(|b| is_ident_part(b as char)) {
            return Err(ExprError::syntax(
                start,
                format!("malformed number literal {:?}", self.literal_text(start)),
            ));
        }
        self.pos = i;
        let text = &self.src[start..text_end];
        if !is_float {
            let parsed = if text.len() > 1 && text.starts_with('0') {
                // Octal, as in Iris (`Integer.parseInt(s.substring(1), 8)`) and GLSL.
                i32::from_str_radix(&text[1..], 8).ok()
            } else {
                text.parse::<i32>().ok()
            };
            if let Some(v) = parsed {
                return lit(ExprKind::Int(v), i);
            }
            // Out of range (or not valid octal): fall back to a float, like Iris.
        }
        let v: f32 = text
            .parse()
            .map_err(|_| ExprError::syntax(start, format!("malformed number literal {text:?}")))?;
        lit(ExprKind::Float(v), i)
    }

    fn literal_text(&self, start: usize) -> &'a str {
        let rest = &self.src[start..];
        let end = rest
            .char_indices()
            .find(|&(_, c)| !(is_ident_part(c) || c == '.'))
            .map_or(rest.len(), |(i, _)| i);
        &rest[..end]
    }
}

impl Expr {
    /// Visit this expression and all sub-expressions in pre-order.
    pub fn walk<'e>(&'e self, f: &mut impl FnMut(&'e Expr)) {
        f(self);
        match &self.kind {
            ExprKind::Unary { operand, .. } => operand.walk(f),
            ExprKind::Binary { lhs, rhs, .. } => {
                lhs.walk(f);
                rhs.walk(f);
            }
            ExprKind::Call { args, .. } => args.iter().for_each(|a| a.walk(f)),
            ExprKind::Member { base, .. } => base.walk(f),
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Ident(_) => {}
        }
    }

    /// All identifiers referenced by this expression (excluding function names and
    /// member names), in order of first appearance, without duplicates.
    pub fn identifiers(&self) -> Vec<&str> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut out: Vec<&str> = Vec::new();
        self.walk(&mut |e| {
            if let ExprKind::Ident(name) = &e.kind
                && seen.insert(name.as_str())
            {
                out.push(name);
            }
        });
        out
    }
}

impl fmt::Display for Expr {
    /// Prints the expression fully parenthesized, e.g. `((a + b) * c)`. Parsing the
    /// printed text of a tree produced by [`parse`] yields the same tree: negative
    /// integer literals (which only hex/binary literals such as `0xFFFFFFFF` produce)
    /// print in hex, and a float literal that overflowed to infinity prints as `1e999`.
    /// Hand-built negative or NaN float literals have no literal syntax; they print as
    /// equivalent expressions (`(-1.5)`, `(0.0 / 0.0)`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ExprKind::Int(i) if *i < 0 => write!(f, "0x{:X}", *i as u32),
            ExprKind::Int(i) => write!(f, "{i}"),
            ExprKind::Float(x) if x.is_nan() => f.write_str("(0.0 / 0.0)"),
            ExprKind::Float(x) if x.is_infinite() => {
                f.write_str(if *x > 0.0 { "1e999" } else { "(-1e999)" })
            }
            ExprKind::Float(x) if x.is_sign_negative() => write!(f, "({x:?})"),
            ExprKind::Float(x) => write!(f, "{x:?}"),
            ExprKind::Bool(b) => write!(f, "{b}"),
            ExprKind::Ident(n) => f.write_str(n),
            ExprKind::Unary { op, operand } => write!(f, "({}{operand})", op.symbol()),
            ExprKind::Binary { op, lhs, rhs } => write!(f, "({lhs} {} {rhs})", op.symbol()),
            ExprKind::Call { name, args, .. } => {
                write!(f, "{name}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str(")")
            }
            // `1.x` / `2.0` would lex as (part of) a number literal.
            ExprKind::Member { base, member, .. }
                if matches!(base.kind, ExprKind::Int(_) | ExprKind::Float(_)) =>
            {
                write!(f, "({base}).{member}")
            }
            ExprKind::Member { base, member, .. } => write!(f, "{base}.{member}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn p(s: &str) -> String {
        parse(s)
            .unwrap_or_else(|e| panic!("parse {s:?}: {e}"))
            .to_string()
    }

    fn err(s: &str) -> ExprError {
        parse(s).expect_err(s)
    }

    #[test]
    fn precedence() {
        assert_eq!(p("a + b * c"), "(a + (b * c))");
        assert_eq!(p("a * b + c"), "((a * b) + c)");
        assert_eq!(p("a - b - c"), "((a - b) - c)");
        assert_eq!(p("a / b / c"), "((a / b) / c)");
        assert_eq!(p("a % b * c"), "((a % b) * c)");
        assert_eq!(p("a + b < c * d"), "((a + b) < (c * d))");
        assert_eq!(p("a < b == c > d"), "((a < b) == (c > d))");
        assert_eq!(p("a == b && c != d"), "((a == b) && (c != d))");
        assert_eq!(p("a || b && c"), "(a || (b && c))");
        assert_eq!(p("a && b || c && d"), "((a && b) || (c && d))");
        assert_eq!(p("a || b || c"), "((a || b) || c)");
        assert_eq!(p("(a + b) * c"), "((a + b) * c)");
        assert_eq!(p("-a * b"), "((-a) * b)");
        assert_eq!(p("-a.x"), "(-a.x)");
        assert_eq!(p("!a && b"), "((!a) && b)");
        assert_eq!(p("!!a"), "(!(!a))");
        assert_eq!(p("a - -b"), "(a - (-b))");
        assert_eq!(p("+a"), "a");
        assert_eq!(p("a<=b"), "(a <= b)");
        assert_eq!(p("a>=-1"), "(a >= (-1))");
    }

    #[test]
    fn unicode_operators() {
        assert_eq!(p("a ≠ b"), "(a != b)");
        assert_eq!(p("a≤b"), "(a <= b)");
        assert_eq!(p("a ≥ b && c"), "((a >= b) && c)");
    }

    #[test]
    fn literals() {
        let lit = |s: &str| parse(s).unwrap().kind;
        assert_eq!(lit("42"), ExprKind::Int(42));
        assert_eq!(lit("1.5"), ExprKind::Float(1.5));
        assert_eq!(lit(".5"), ExprKind::Float(0.5));
        assert_eq!(lit("2."), ExprKind::Float(2.0));
        assert_eq!(lit("1e-3"), ExprKind::Float(1e-3));
        assert_eq!(lit("1.5E+2"), ExprKind::Float(150.0));
        assert_eq!(lit("3e2"), ExprKind::Float(300.0));
        assert_eq!(lit("1.0f"), ExprKind::Float(1.0));
        assert_eq!(lit("2F"), ExprKind::Float(2.0));
        assert_eq!(lit("0x1F"), ExprKind::Int(31));
        assert_eq!(lit("0XfF"), ExprKind::Int(255));
        assert_eq!(lit("0xFFFFFFFF"), ExprKind::Int(-1));
        assert_eq!(lit("0b101"), ExprKind::Int(5));
        assert_eq!(lit("010"), ExprKind::Int(8));
        assert_eq!(lit("0"), ExprKind::Int(0));
        assert_eq!(lit("08"), ExprKind::Float(8.0));
        assert_eq!(lit("2147483647"), ExprKind::Int(i32::MAX));
        assert_eq!(lit("2147483648"), ExprKind::Float(2_147_483_648.0));
        assert_eq!(lit("1000000000000000000"), ExprKind::Float(1e18));
        assert_eq!(lit("true"), ExprKind::Bool(true));
        assert_eq!(lit("false"), ExprKind::Bool(false));
        assert_eq!(lit("  7  "), ExprKind::Int(7));
        assert_eq!(lit("7;"), ExprKind::Int(7));
    }

    #[test]
    fn members_and_calls() {
        assert_eq!(p("sunPosition.y"), "sunPosition.y");
        assert_eq!(p("gbufferModelView.0.1"), "gbufferModelView.0.1");
        assert_eq!(p("m . 2 . 3"), "m.2.3");
        assert_eq!(p("v.xyz"), "v.xyz");
        assert_eq!(p("vec3(1, 2, 3).x"), "vec3(1, 2, 3).x");
        assert_eq!(p("(a).y"), "a.y");
        assert_eq!(p("random()"), "random()");
        assert_eq!(p("max(a, b, c,)"), "max(a, b, c)");
        assert_eq!(p("sin (x)"), "sin(x)");
        assert_eq!(p("a.x*b.y"), "(a.x * b.y)");
        assert_eq!(p("x * .5"), "(x * 0.5)");
        assert_eq!(p("if(a, 1, b, 2, 3)"), "if(a, 1, b, 2, 3)");
    }

    #[test]
    fn spans() {
        let e = parse("  foo + bar(1)").unwrap();
        assert_eq!(e.span, Span::new(2, 14));
        let ExprKind::Binary { lhs, rhs, .. } = &e.kind else {
            panic!()
        };
        assert_eq!(lhs.span, Span::new(2, 5));
        assert_eq!(rhs.span, Span::new(8, 14));
        let ExprKind::Call { name_span, .. } = &rhs.kind else {
            panic!()
        };
        assert_eq!(*name_span, Span::new(8, 11));
        assert_eq!(parse("(a)").unwrap().span, Span::new(0, 3));
    }

    #[test]
    fn errors_have_positions() {
        let e = err("1 +");
        assert_eq!((e.offset, e.kind), (3, ExprErrorKind::Syntax));
        assert_eq!(err("").offset, 0);
        assert_eq!(err("   ").message, "empty expression");
        assert_eq!(err("(a + b").offset, 0);
        assert_eq!(err("a + b)").offset, 5);
        assert_eq!(err("a b").offset, 2);
        assert_eq!(err("f(a b)").offset, 4);
        assert_eq!(err("f(a,").offset, 1);
        assert_eq!(err("a & b").offset, 2);
        assert!(err("a & b").message.contains("&&"));
        assert_eq!(err("a = b").offset, 2);
        assert_eq!(err("a | b").offset, 2);
        assert_eq!(err("a.").offset, 2);
        assert_eq!(err("a.+").offset, 2);
        assert_eq!(err("1e").offset, 0);
        assert_eq!(err("1x").offset, 0);
        assert_eq!(err("0x").offset, 0);
        assert_eq!(err("0x1G").offset, 0);
        assert_eq!(err("0x1FFFFFFFF").offset, 0);
        assert_eq!(err("a # b").offset, 2);
        assert_eq!(err("()").offset, 1);
        assert_eq!(err("(a, b)").offset, 2);
        assert_eq!(err("a, b").offset, 1);
        assert_eq!(err(";").offset, 0);
        assert_eq!(err("*a").offset, 0);
        // Byte offsets account for multi-byte characters.
        let e = err("a ≤ ");
        assert_eq!(e.offset, 6);
        assert_eq!(e.column("a ≤ "), 5);
    }

    #[test]
    fn depth_limit() {
        let deep = format!(
            "{}1{}",
            "(".repeat(MAX_DEPTH + 5),
            ")".repeat(MAX_DEPTH + 5)
        );
        assert_eq!(err(&deep).kind, ExprErrorKind::TooComplex);
        let chain = vec!["1"; MAX_DEPTH + 5].join(" + ");
        assert_eq!(err(&chain).kind, ExprErrorKind::TooComplex);
        let neg = format!("{}1", "-".repeat(MAX_DEPTH + 5));
        assert_eq!(err(&neg).kind, ExprErrorKind::TooComplex);
        let members = format!("v{}", ".x".repeat(MAX_DEPTH + 5));
        assert_eq!(err(&members).kind, ExprErrorKind::TooComplex);
        // Wide is fine.
        let wide = format!("max({})", vec!["1"; 5000].join(", "));
        assert!(parse(&wide).is_ok());
        let ok_chain = vec!["1"; MAX_DEPTH / 2].join(" + ");
        assert!(parse(&ok_chain).is_ok());
    }

    /// Strip spans so trees from different texts can be compared structurally.
    fn shape(e: &Expr) -> Expr {
        let kind = match &e.kind {
            ExprKind::Unary { op, operand } => ExprKind::Unary {
                op: *op,
                operand: Box::new(shape(operand)),
            },
            ExprKind::Binary { op, lhs, rhs } => ExprKind::Binary {
                op: *op,
                lhs: Box::new(shape(lhs)),
                rhs: Box::new(shape(rhs)),
            },
            ExprKind::Call { name, args, .. } => ExprKind::Call {
                name: name.clone(),
                name_span: Span::default(),
                args: args.iter().map(shape).collect(),
            },
            ExprKind::Member { base, member, .. } => ExprKind::Member {
                base: Box::new(shape(base)),
                member: member.clone(),
                member_span: Span::default(),
            },
            other => other.clone(),
        };
        Expr {
            kind,
            span: Span::default(),
        }
    }

    #[test]
    fn display_reparses_to_the_same_tree() {
        // Regression: negative ints (from hex/binary literals) used to print as `(-1)`,
        // which re-parses as a negation, and `i32::MIN` as `(-2147483648)`, which
        // re-parses as a *float*; an overflowing float printed as `inf`, which
        // re-parses as an identifier.
        for src in [
            "0xFFFFFFFF",
            "0x80000000",
            "0b11111111111111111111111111111111 + 1",
            "1e999",
            "1e999 * x",
            "2147483648",
            "-2147483647 - 1",
            "a.x + f(1, -2.5, .5)",
            "max(v.xyz, 3) % 0x7fffffff",
            "3.4028235e38",
            "1e-45",
            // Member access on number literals (a type error later, but valid syntax).
            "(1).x",
            "(2).0",
            "(1.5).y",
            "(0x10).1",
        ] {
            let tree = parse(src).unwrap();
            let printed = tree.to_string();
            let again = parse(&printed).unwrap_or_else(|e| panic!("{src} -> {printed}: {e}"));
            assert_eq!(shape(&again), shape(&tree), "{src} -> {printed}");
        }
        assert_eq!(p("0xFFFFFFFF"), "0xFFFFFFFF");
        assert_eq!(p("0x80000000"), "0x80000000");
        assert_eq!(p("1e999"), "1e999");
        // Hand-built literals without a literal syntax still print as valid text.
        let lit = |x: f32| Expr {
            kind: ExprKind::Float(x),
            span: Span::default(),
        };
        assert_eq!(lit(f32::NEG_INFINITY).to_string(), "(-1e999)");
        assert_eq!(lit(f32::NAN).to_string(), "(0.0 / 0.0)");
        assert_eq!(lit(-1.5).to_string(), "(-1.5)");
        for x in [f32::NEG_INFINITY, f32::NAN, -1.5] {
            assert!(parse(&lit(x).to_string()).is_ok());
        }
    }

    #[test]
    fn identifiers_helper() {
        let e = parse("a + f(b, a.x) * c.0").unwrap();
        assert_eq!(e.identifiers(), vec!["a", "b", "c"]);
    }
}
