//! GLSL printer for [`crate::ast`] with a per-output-line source map.
//!
//! Written for ShaderBridge (not derived from glsl-lang's transpiler): it prints the
//! minimal parentheses from operator precedence, never panics, prints non-finite float
//! constants as `uintBitsToFloat(...)` (glsl-lang would print `inf`), and records the
//! source line of every output line.

use std::fmt::Write as _;

use crate::ast::*;

/// Accumulates GLSL text and the source line of every output line.
#[derive(Debug, Default)]
pub(crate) struct Printer {
    out: String,
    /// Source line (parsed-text line, 0 = generated) of every output line.
    lines: Vec<Line>,
    indent: usize,
}

impl Printer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one output line (without trailing newline) mapped to `src`.
    pub fn line(&mut self, text: &str, src: Line) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
        self.lines.push(src);
    }

    /// Append generated text (possibly several lines), each mapped to `src`.
    pub fn text(&mut self, text: &str, src: Line) {
        for l in text.lines() {
            self.line(l, src);
        }
    }

    /// The printed text and line sources.
    pub fn finish(self) -> (String, Vec<Line>) {
        (self.out, self.lines)
    }

    /// Print an external declaration.
    pub fn item(&mut self, item: &Item) {
        let l = item.line;
        match &item.kind {
            ItemKind::Function(f) => {
                let head = format!("{} {{", prototype(&f.proto));
                self.line(&head, l);
                self.indent += 1;
                for s in &f.body {
                    self.stmt(s);
                }
                self.indent -= 1;
                self.line("}", l);
            }
            ItemKind::Prototype(p) => self.line(&format!("{};", prototype(p)), l),
            ItemKind::Decl(d) => self.line(&format!("{};", declaration(d)), l),
            ItemKind::Block(b) => self.line(&format!("{};", block(b)), l),
            ItemKind::Precision(p, t) => self.line(&format!("precision {} {};", precision(*p), type_spec(t)), l),
            ItemKind::Invariant(n) => self.line(&format!("invariant {n};"), l),
            ItemKind::QualifierOnly(q) => self.line(&format!("{};", qualifiers(q)), l),
            ItemKind::Raw(t) => self.text(t, l),
        }
    }

    fn sub_stmt(&mut self, prefix: &str, s: &Stmt, src: Line) {
        if let StmtKind::Block(b) = &s.kind {
            self.line(&format!("{prefix} {{"), src);
            self.indent += 1;
            for x in b {
                self.stmt(x);
            }
            self.indent -= 1;
        } else {
            self.line(&format!("{prefix} {{"), src);
            self.indent += 1;
            self.stmt(s);
            self.indent -= 1;
        }
    }

    /// Print a statement.
    pub fn stmt(&mut self, s: &Stmt) {
        let l = s.line;
        match &s.kind {
            StmtKind::Decl(d) => self.line(&format!("{};", declaration(d)), l),
            StmtKind::Expr(e) => self.line(&format!("{};", expr(e)), l),
            StmtKind::Empty => {}
            StmtKind::Block(b) => {
                self.line("{", l);
                self.indent += 1;
                for x in b {
                    self.stmt(x);
                }
                self.indent -= 1;
                self.line("}", l);
            }
            StmtKind::If { cond, then, els } => {
                self.sub_stmt(&format!("if ({})", expr(cond)), then, l);
                let mut els = els.as_deref();
                while let Some(e) = els {
                    match &e.kind {
                        StmtKind::If { cond, then, els: next } => {
                            self.sub_stmt_close_open(&format!("}} else if ({})", expr(cond)), then, e.line);
                            els = next.as_deref();
                        }
                        _ => {
                            self.sub_stmt_close_open("} else", e, e.line);
                            els = None;
                        }
                    }
                }
                self.line("}", l);
            }
            StmtKind::Switch { expr: e, body } => {
                self.line(&format!("switch ({}) {{", expr(e)), l);
                self.indent += 1;
                for x in body {
                    self.stmt(x);
                }
                self.indent -= 1;
                self.line("}", l);
            }
            StmtKind::Case(e) => {
                self.indent = self.indent.saturating_sub(1);
                self.line(&format!("case {}:", expr(e)), l);
                self.indent += 1;
            }
            StmtKind::Default => {
                self.indent = self.indent.saturating_sub(1);
                self.line("default:", l);
                self.indent += 1;
            }
            StmtKind::While { cond, body } => {
                self.sub_stmt(&format!("while ({})", condition(cond)), body, l);
                self.line("}", l);
            }
            StmtKind::DoWhile { body, cond } => {
                self.sub_stmt("do", body, l);
                self.line(&format!("}} while ({});", expr(cond)), l);
            }
            StmtKind::For { init, cond, step, body } => {
                let init = match init.as_deref().map(|s| &s.kind) {
                    Some(StmtKind::Decl(d)) => declaration(d),
                    Some(StmtKind::Expr(e)) => expr(e),
                    _ => String::new(),
                };
                let cond = cond.as_ref().map(condition).unwrap_or_default();
                let step = step.as_ref().map(expr).unwrap_or_default();
                self.sub_stmt(&format!("for ({init}; {cond}; {step})"), body, l);
                self.line("}", l);
            }
            StmtKind::Return(None) => self.line("return;", l),
            StmtKind::Return(Some(e)) => self.line(&format!("return {};", expr(e)), l),
            StmtKind::Break => self.line("break;", l),
            StmtKind::Continue => self.line("continue;", l),
            StmtKind::Discard => self.line("discard;", l),
            StmtKind::Raw(t) => self.text(t, l),
        }
    }

    fn sub_stmt_close_open(&mut self, prefix: &str, s: &Stmt, src: Line) {
        // `prefix` starts with the closing brace of the previous branch.
        self.sub_stmt(prefix, s, src);
    }
}

fn precision(p: Precision) -> &'static str {
    match p {
        Precision::High => "highp",
        Precision::Medium => "mediump",
        Precision::Low => "lowp",
    }
}

/// `T name(params)`
pub(crate) fn prototype(p: &Prototype) -> String {
    let mut s = full_type(&p.ret);
    s.push(' ');
    s.push_str(&p.name);
    s.push('(');
    for (i, param) in p.params.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        let q = qualifiers(&param.quals);
        if !q.is_empty() {
            s.push_str(&q);
            s.push(' ');
        }
        s.push_str(&type_spec(&param.ty));
        if let Some(n) = &param.name {
            s.push(' ');
            s.push_str(n);
        }
        s.push_str(&array_dims(&param.array));
    }
    s.push(')');
    s
}

/// Qualifiers separated by spaces.
pub(crate) fn qualifiers(q: &[Qualifier]) -> String {
    let mut parts = Vec::with_capacity(q.len());
    for x in q {
        parts.push(match x {
            Qualifier::Storage(s) => storage(s),
            Qualifier::Layout(ids) => {
                let inner: Vec<String> = ids
                    .iter()
                    .map(|l| match &l.value {
                        Some(v) => format!("{} = {}", l.name, expr(v)),
                        None => l.name.clone(),
                    })
                    .collect();
                format!("layout({})", inner.join(", "))
            }
            Qualifier::Precision(p) => precision(*p).to_string(),
            Qualifier::Interp(Interp::Smooth) => "smooth".into(),
            Qualifier::Interp(Interp::Flat) => "flat".into(),
            Qualifier::Interp(Interp::NoPerspective) => "noperspective".into(),
            Qualifier::Invariant => "invariant".into(),
            Qualifier::Precise => "precise".into(),
        });
    }
    parts.join(" ")
}

fn storage(s: &Storage) -> String {
    match s {
        Storage::Const => "const",
        Storage::InOut => "inout",
        Storage::In => "in",
        Storage::Out => "out",
        Storage::Centroid => "centroid",
        Storage::Patch => "patch",
        Storage::Sample => "sample",
        Storage::Uniform => "uniform",
        Storage::Buffer => "buffer",
        Storage::Shared => "shared",
        Storage::Coherent => "coherent",
        Storage::Volatile => "volatile",
        Storage::Restrict => "restrict",
        Storage::ReadOnly => "readonly",
        Storage::WriteOnly => "writeonly",
        Storage::Attribute => "attribute",
        Storage::Varying => "varying",
        Storage::Subroutine(names) => return format!("subroutine({})", names.join(", ")),
    }
    .to_string()
}

/// Qualifiers + type.
pub(crate) fn full_type(t: &FullType) -> String {
    let q = qualifiers(&t.quals);
    if q.is_empty() { type_spec(&t.ty) } else { format!("{q} {}", type_spec(&t.ty)) }
}

/// A type with its array dimensions (inline structs printed in full).
pub(crate) fn type_spec(t: &TypeSpec) -> String {
    let mut s = match &t.base {
        TypeBase::Named(n) => n.clone(),
        TypeBase::Struct(st) => struct_def(st),
    };
    s.push_str(&array_dims(&t.array));
    s
}

fn struct_def(st: &StructDef) -> String {
    let mut s = String::from("struct");
    if let Some(n) = &st.name {
        s.push(' ');
        s.push_str(n);
    }
    s.push_str(" { ");
    for f in &st.fields {
        s.push_str(&field(f));
        s.push(' ');
    }
    s.push('}');
    s
}

/// `quals T a[2], b;`
pub(crate) fn field(f: &Field) -> String {
    let q = qualifiers(&f.quals);
    let mut s = if q.is_empty() { type_spec(&f.ty) } else { format!("{q} {}", type_spec(&f.ty)) };
    for (i, (n, dims)) in f.names.iter().enumerate() {
        s.push_str(if i == 0 { " " } else { ", " });
        s.push_str(n);
        s.push_str(&array_dims(dims));
    }
    s.push(';');
    s
}

/// `[2][]`
pub(crate) fn array_dims(d: &[ArrayDim]) -> String {
    let mut s = String::new();
    for x in d {
        match x {
            ArrayDim::Unsized => s.push_str("[]"),
            ArrayDim::Sized(e) => {
                let _ = write!(s, "[{}]", expr(e));
            }
        }
    }
    s
}

/// A declaration without the trailing `;`.
pub(crate) fn declaration(d: &Declaration) -> String {
    let mut s = full_type(&d.ty);
    for (i, v) in d.vars.iter().enumerate() {
        s.push_str(if i == 0 { " " } else { ", " });
        s.push_str(&v.name);
        s.push_str(&array_dims(&v.array));
        if let Some(init) = &v.init {
            s.push_str(" = ");
            s.push_str(&initializer(init));
        }
    }
    s
}

fn initializer(i: &Init) -> String {
    match i {
        Init::Expr(e) => expr_prec(e, PREC_ASSIGN),
        Init::List(l) => {
            let inner: Vec<String> = l.iter().map(initializer).collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// An interface block without the trailing `;`.
pub(crate) fn block(b: &InterfaceBlock) -> String {
    let mut s = qualifiers(&b.quals);
    if !s.is_empty() {
        s.push(' ');
    }
    s.push_str(&b.name);
    s.push_str(" { ");
    for f in &b.fields {
        s.push_str(&field(f));
        s.push(' ');
    }
    s.push('}');
    if let Some((n, dims)) = &b.instance {
        s.push(' ');
        s.push_str(n);
        s.push_str(&array_dims(dims));
    }
    s
}

fn condition(c: &Condition) -> String {
    match c {
        Condition::Expr(e) => expr(e),
        Condition::Decl { ty, name, init } => format!("{} {name} = {}", full_type(ty), initializer(init)),
    }
}

// Precedence levels (higher binds tighter).
const PREC_COMMA: u8 = 1;
const PREC_ASSIGN: u8 = 2;
const PREC_TERNARY: u8 = 3;
const PREC_UNARY: u8 = 16;
const PREC_POSTFIX: u8 = 17;
const PREC_PRIMARY: u8 = 18;

fn binary_prec(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Or => 4,
        BinaryOp::Xor => 5,
        BinaryOp::And => 6,
        BinaryOp::BitOr => 7,
        BinaryOp::BitXor => 8,
        BinaryOp::BitAnd => 9,
        BinaryOp::Equal | BinaryOp::NonEqual => 10,
        BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Lte | BinaryOp::Gte => 11,
        BinaryOp::LShift | BinaryOp::RShift => 12,
        BinaryOp::Add | BinaryOp::Sub => 13,
        BinaryOp::Mult | BinaryOp::Div | BinaryOp::Mod => 14,
    }
}

/// GLSL spelling of a binary operator.
pub(crate) fn binary_op(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Or => "||",
        BinaryOp::Xor => "^^",
        BinaryOp::And => "&&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::BitAnd => "&",
        BinaryOp::Equal => "==",
        BinaryOp::NonEqual => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::Gt => ">",
        BinaryOp::Lte => "<=",
        BinaryOp::Gte => ">=",
        BinaryOp::LShift => "<<",
        BinaryOp::RShift => ">>",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mult => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
    }
}

fn assign_op(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Equal => "=",
        AssignOp::Mult => "*=",
        AssignOp::Div => "/=",
        AssignOp::Mod => "%=",
        AssignOp::Add => "+=",
        AssignOp::Sub => "-=",
        AssignOp::LShift => "<<=",
        AssignOp::RShift => ">>=",
        AssignOp::And => "&=",
        AssignOp::Xor => "^=",
        AssignOp::Or => "|=",
    }
}

fn expr_level(e: &Expr) -> u8 {
    match e {
        Expr::Ident(_) | Expr::Bool(_) | Expr::UInt(_) | Expr::Raw(_) => PREC_PRIMARY,
        Expr::Int(v) => {
            if *v < 0 {
                PREC_UNARY
            } else {
                PREC_PRIMARY
            }
        }
        Expr::Float(v) => {
            if v.is_sign_negative() && !v.is_nan() {
                PREC_UNARY
            } else {
                PREC_PRIMARY
            }
        }
        Expr::Double(v) => {
            if v.is_sign_negative() && !v.is_nan() {
                PREC_UNARY
            } else {
                PREC_PRIMARY
            }
        }
        Expr::Unary(..) => PREC_UNARY,
        Expr::Binary(op, ..) => binary_prec(*op),
        Expr::Ternary(..) => PREC_TERNARY,
        Expr::Assign(..) => PREC_ASSIGN,
        Expr::Index(..) | Expr::Call(..) | Expr::Field(..) | Expr::PostInc(_) | Expr::PostDec(_) => PREC_POSTFIX,
        Expr::Comma(..) => PREC_COMMA,
    }
}

/// Print an expression.
pub(crate) fn expr(e: &Expr) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, PREC_COMMA);
    s
}

/// Print an expression that must bind at least as tightly as `min`.
fn expr_prec(e: &Expr, min: u8) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, min);
    s
}

fn write_f32(s: &mut String, v: f32) {
    if v.is_nan() {
        s.push_str("uintBitsToFloat(0x7FC00000u)");
    } else if v.is_infinite() {
        s.push_str(if v > 0.0 { "uintBitsToFloat(0x7F800000u)" } else { "uintBitsToFloat(0xFF800000u)" });
    } else {
        let _ = write!(s, "{v:?}");
    }
}

fn write_f64(s: &mut String, v: f64) {
    if v.is_nan() {
        s.push_str("double(uintBitsToFloat(0x7FC00000u))");
    } else if v.is_infinite() {
        s.push_str(if v > 0.0 { "double(uintBitsToFloat(0x7F800000u))" } else { "double(uintBitsToFloat(0xFF800000u))" });
    } else {
        let _ = write!(s, "{v:?}lf");
    }
}

fn write_expr(s: &mut String, e: &Expr, min: u8) {
    let level = expr_level(e);
    let paren = level < min;
    if paren {
        s.push('(');
    }
    match e {
        Expr::Ident(n) => s.push_str(n),
        Expr::Int(v) => {
            if *v == i32::MIN {
                s.push_str("int(0x80000000u)");
            } else {
                let _ = write!(s, "{v}");
            }
        }
        Expr::UInt(v) => {
            let _ = write!(s, "{v}u");
        }
        Expr::Bool(v) => s.push_str(if *v { "true" } else { "false" }),
        Expr::Float(v) => write_f32(s, *v),
        Expr::Double(v) => write_f64(s, *v),
        Expr::Raw(t) => {
            s.push('(');
            s.push_str(t);
            s.push(')');
        }
        Expr::Unary(op, a) => {
            let o = match op {
                UnaryOp::Inc => "++",
                UnaryOp::Dec => "--",
                UnaryOp::Plus => "+",
                UnaryOp::Minus => "-",
                UnaryOp::Not => "!",
                UnaryOp::Complement => "~",
            };
            s.push_str(o);
            // Avoid `- -x` turning into `--x`.
            let inner = expr_prec(a, PREC_UNARY);
            if (matches!(op, UnaryOp::Minus | UnaryOp::Dec) && inner.starts_with('-'))
                || (matches!(op, UnaryOp::Plus | UnaryOp::Inc) && inner.starts_with('+'))
            {
                s.push(' ');
            }
            s.push_str(&inner);
        }
        Expr::Binary(op, a, b) => {
            let p = binary_prec(*op);
            write_expr(s, a, p);
            s.push(' ');
            s.push_str(binary_op(*op));
            s.push(' ');
            write_expr(s, b, p + 1);
        }
        Expr::Ternary(c, a, b) => {
            write_expr(s, c, PREC_TERNARY + 1);
            s.push_str(" ? ");
            write_expr(s, a, PREC_ASSIGN);
            s.push_str(" : ");
            write_expr(s, b, PREC_TERNARY);
        }
        Expr::Assign(a, op, b) => {
            write_expr(s, a, PREC_UNARY);
            s.push(' ');
            s.push_str(assign_op(*op));
            s.push(' ');
            write_expr(s, b, PREC_ASSIGN);
        }
        Expr::Index(a, i) => {
            write_expr(s, a, PREC_POSTFIX);
            s.push('[');
            write_expr(s, i, PREC_COMMA);
            s.push(']');
        }
        Expr::Call(callee, args) => {
            match callee {
                Callee::Name(n) => s.push_str(n),
                Callee::ArrayCtor(t) => s.push_str(&type_spec(t)),
                Callee::Method(r, m) => {
                    write_expr(s, r, PREC_POSTFIX);
                    s.push('.');
                    s.push_str(m);
                }
            }
            s.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                write_expr(s, a, PREC_ASSIGN);
            }
            s.push(')');
        }
        Expr::Field(a, f) => {
            write_expr(s, a, PREC_POSTFIX);
            s.push('.');
            s.push_str(f);
        }
        Expr::PostInc(a) => {
            write_expr(s, a, PREC_POSTFIX);
            s.push_str("++");
        }
        Expr::PostDec(a) => {
            write_expr(s, a, PREC_POSTFIX);
            s.push_str("--");
        }
        Expr::Comma(a, b) => {
            write_expr(s, a, PREC_COMMA);
            s.push_str(", ");
            write_expr(s, b, PREC_ASSIGN);
        }
    }
    if paren {
        s.push(')');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{parse_expr, parse_glsl};

    fn rt(e: &str) -> String {
        expr(&parse_expr(e).unwrap())
    }

    #[test]
    fn precedence_round_trip() {
        assert_eq!(rt("a + b * c"), "a + b * c");
        assert_eq!(rt("(a + b) * c"), "(a + b) * c");
        assert_eq!(rt("a - (b - c)"), "a - (b - c)");
        assert_eq!(rt("a - b - c"), "a - b - c");
        assert_eq!(rt("-(-x)"), "- -x");
        assert_eq!(rt("a ? b : c ? d : e"), "a ? b : c ? d : e");
        assert_eq!(rt("(a ? b : c) ? d : e"), "(a ? b : c) ? d : e");
        assert_eq!(rt("x = y = 3"), "x = y = 3");
        assert_eq!(rt("f(a, (b, c))"), "f(a, (b, c))");
        assert_eq!(rt("(a + b).xy"), "(a + b).xy");
        assert_eq!(rt("v[i + 1].x++"), "v[i + 1].x++");
        assert_eq!(rt("float[2](1.0, 2.0)"), "float[2](1.0, 2.0)");
        assert_eq!(rt("arr.length()"), "arr.length()");
        assert_eq!(rt("!(a && b) || c"), "!(a && b) || c");
        assert_eq!(rt("1u + 0x10u"), "1u + 16u");
    }

    #[test]
    fn floats_print_round_trip() {
        assert_eq!(rt("1.0"), "1.0");
        assert_eq!(rt("0.1"), "0.1");
        assert_eq!(rt("1e39"), "uintBitsToFloat(0x7F800000u)");
        assert_eq!(rt("3.0e-8"), "3e-8");
        assert_eq!(rt("2.5lf"), "2.5lf");
        assert_eq!(expr(&Expr::Float(-0.5)), "-0.5");
        assert_eq!(expr(&Expr::Unary(UnaryOp::Minus, Box::new(Expr::Float(-0.5)))), "- -0.5");
        assert_eq!(expr(&Expr::Int(i32::MIN)), "int(0x80000000u)");
    }

    #[test]
    fn statements_and_line_map() {
        let src = "uniform float a;\nvoid main() {\n  if (a > 0.0) discard; else { a; }\n  for (int i = 0; i < 4; i++) a;\n}\n";
        let unit = parse_glsl(src, 120).unwrap();
        let mut p = Printer::new();
        for it in &unit.items {
            p.item(it);
        }
        let (text, lines) = p.finish();
        assert_eq!(
            text,
            "uniform float a;\nvoid main() {\n    if (a > 0.0) {\n        discard;\n    } else {\n        a;\n    }\n    for (int i = 0; i < 4; i++) {\n        a;\n    }\n}\n"
        );
        assert_eq!(lines, [1, 2, 3, 3, 3, 3, 3, 4, 4, 4, 2]);
    }
}
