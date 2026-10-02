//! Type checking and lowering of [`Expr`] trees into [`Node`] trees.
//!
//! Typing rules (a superset of Iris, which is a superset of OptiFine):
//!
//! * `int` promotes to `float` implicitly; `+ - * %` on two ints stay `int`
//!   (wrapping), `/` always divides as floats.
//! * Vectors combine component-wise with vectors of the same size or with scalars
//!   (broadcast). Matrices can only be indexed (`m.i` is column `i`, `m.i.j` is GLSL
//!   `m[i][j]`) or compared for equality.
//! * Conditions (`!`, `&&`, `||`, `if` conditions) accept booleans and, leniently,
//!   numbers (non-zero is true). Comparisons accept numbers and booleans (as 0/1).
//! * Arithmetic on booleans is an error.

use crate::node::{ArithOp, CmpOp, Math1, Math2, Node, fold};
use crate::parse::{BinaryOp, Expr, ExprError, ExprErrorKind, ExprKind, Span, UnaryOp};
use crate::value::{Value, ValueType};

/// What an identifier resolves to.
pub(crate) enum Resolved {
    Const(Value),
    Input(u32, ValueType),
    Var(u32, ValueType),
}

/// Name resolution for identifiers.
pub(crate) trait Scope {
    /// `Ok(None)`: unknown identifier. `Err(msg)`: known but unusable.
    fn resolve(&mut self, name: &str) -> Result<Option<Resolved>, String>;
}

/// A compiled node with its static type.
pub(crate) struct Typed {
    pub node: Node,
    pub ty: ValueType,
}

fn typed(node: Node, ty: ValueType) -> Typed {
    Typed {
        node: fold(node),
        ty,
    }
}

/// Insert a conversion to `to` (no-op when the type already matches).
fn conv(t: Typed, to: ValueType) -> Typed {
    if t.ty == to {
        t
    } else {
        typed(Node::Convert(to, Box::new(t.node)), to)
    }
}

fn b(t: Typed) -> Box<Node> {
    Box::new(t.node)
}

/// Move exactly `N` arguments out of `a` (arity has been checked already).
fn take<const N: usize>(a: Vec<Typed>) -> Option<[Typed; N]> {
    a.try_into().ok()
}

fn type_err(span: Span, msg: impl Into<String>) -> ExprError {
    ExprError::new(ExprErrorKind::Type, span.start, msg)
}

/// Built-in functions.
#[derive(Clone, Copy)]
enum Func {
    M1(Math1),
    Atan,
    Log,
    M2(Math2),
    MinMax(Math2),
    Clamp,
    Mix,
    Lerp,
    Random,
    RandomInt,
    If,
    Ifb,
    Smooth,
    Between,
    Equals,
    In,
    Vec(usize, VecKind),
    Print,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VecKind {
    Float,
    Int,
    Bool,
}

impl Func {
    fn lookup(name: &str) -> Option<Func> {
        use Math1 as M;
        Some(match name {
            "sin" => Func::M1(M::Sin),
            "cos" => Func::M1(M::Cos),
            "tan" => Func::M1(M::Tan),
            "asin" => Func::M1(M::Asin),
            "acos" => Func::M1(M::Acos),
            "atan" => Func::Atan,
            "atan2" => Func::M2(Math2::Atan2),
            "torad" | "radians" => Func::M1(M::ToRad),
            "todeg" | "degrees" => Func::M1(M::ToDeg),
            "exp" => Func::M1(M::Exp),
            "exp2" => Func::M1(M::Exp2),
            "exp10" => Func::M1(M::Exp10),
            "pow" => Func::M2(Math2::Pow),
            "log" => Func::Log,
            "log2" => Func::M1(M::Log2),
            "log10" => Func::M1(M::Log10),
            "sqrt" => Func::M1(M::Sqrt),
            "inversesqrt" => Func::M1(M::InvSqrt),
            "abs" => Func::M1(M::Abs),
            "sign" | "signum" => Func::M1(M::Sign),
            "floor" => Func::M1(M::Floor),
            "ceil" => Func::M1(M::Ceil),
            "frac" => Func::M1(M::Frac),
            "round" => Func::M1(M::Round),
            "min" => Func::MinMax(Math2::Min),
            "max" => Func::MinMax(Math2::Max),
            "clamp" => Func::Clamp,
            "mix" => Func::Mix,
            "lerp" => Func::Lerp,
            "edge" => Func::M2(Math2::Edge),
            "fmod" => Func::M2(Math2::Fmod),
            "random" => Func::Random,
            "randomInt" => Func::RandomInt,
            "if" => Func::If,
            "ifb" => Func::Ifb,
            "smooth" => Func::Smooth,
            "between" => Func::Between,
            "equals" => Func::Equals,
            "in" => Func::In,
            "vec2" => Func::Vec(2, VecKind::Float),
            "vec3" => Func::Vec(3, VecKind::Float),
            "vec4" => Func::Vec(4, VecKind::Float),
            "ivec2" => Func::Vec(2, VecKind::Int),
            "ivec3" => Func::Vec(3, VecKind::Int),
            "ivec4" => Func::Vec(4, VecKind::Int),
            "bvec2" => Func::Vec(2, VecKind::Bool),
            "bvec3" => Func::Vec(3, VecKind::Bool),
            "bvec4" => Func::Vec(4, VecKind::Bool),
            "print" => Func::Print,
            _ => return None,
        })
    }

    /// `Err(expected)` when `n` arguments are not accepted.
    fn check_arity(self, n: usize) -> Result<(), &'static str> {
        let ok = match self {
            Func::M1(_) => n == 1,
            Func::Atan | Func::Log => n == 1 || n == 2,
            Func::M2(_) => n == 2,
            Func::MinMax(_) => n >= 2,
            Func::Clamp | Func::Mix | Func::Lerp | Func::Between | Func::Print => n == 3,
            Func::Random => n == 0 || n == 2,
            Func::RandomInt => n <= 2,
            Func::If | Func::Ifb => n >= 3 && !n.is_multiple_of(2),
            Func::Smooth => (1..=4).contains(&n),
            Func::Equals => n == 2 || n == 3,
            Func::In => n >= 2,
            Func::Vec(..) => n >= 1,
        };
        if ok {
            return Ok(());
        }
        Err(match self {
            Func::M1(_) => "1 argument",
            Func::Atan | Func::Log => "1 or 2 arguments",
            Func::M2(_) => "2 arguments",
            Func::MinMax(_) | Func::In => "at least 2 arguments",
            Func::Clamp | Func::Mix | Func::Lerp | Func::Between | Func::Print => "3 arguments",
            Func::Random => "0 or 2 arguments",
            Func::RandomInt => "0, 1 or 2 arguments",
            Func::If | Func::Ifb => {
                "an odd number of arguments (at least 3): (cond, value, ..., else)"
            }
            Func::Smooth => "1 to 4 arguments: ([id,] value [, fadeUp [, fadeDown]])",
            Func::Equals => "2 or 3 arguments",
            Func::Vec(..) => "at least 1 argument",
        })
    }
}

/// Result type of numeric unification, or an error message.
fn unify_numeric(types: &[ValueType], int_ok: bool) -> Result<ValueType, String> {
    let mut vec: Option<usize> = None;
    let mut all_int = true;
    for &t in types {
        match t {
            ValueType::Int => {}
            ValueType::Float => all_int = false,
            ValueType::Vec2 | ValueType::Vec3 | ValueType::Vec4 => {
                all_int = false;
                let n = t.vector_len().unwrap_or(0);
                if vec.is_some_and(|v| v != n) {
                    return Err("mixes vectors of different sizes".to_string());
                }
                vec = Some(n);
            }
            ValueType::Bool | ValueType::Mat4 => {
                return Err(format!("expects numbers or vectors, found {t}"));
            }
        }
    }
    Ok(match vec {
        Some(n) => ValueType::vector(n).unwrap_or(ValueType::Vec4),
        None if all_int && int_ok => ValueType::Int,
        None => ValueType::Float,
    })
}

/// Convert an operand to the unified type `r`. Scalars stay scalars (and broadcast
/// at runtime) when `r` is a vector.
fn coerce_operand(t: Typed, r: ValueType) -> Typed {
    match r {
        ValueType::Int => t,
        _ if t.ty == ValueType::Int => conv(t, ValueType::Float),
        _ => t,
    }
}

/// Map a component letter or digit to its index.
fn component_index(c: char) -> Option<(u8, u8)> {
    // (index, family): 0 = xyzw, 1 = rgba, 2 = stpq, 3 = digits
    Some(match c {
        'x' => (0, 0),
        'y' => (1, 0),
        'z' => (2, 0),
        'w' => (3, 0),
        'r' => (0, 1),
        'g' => (1, 1),
        'b' => (2, 1),
        'a' => (3, 1),
        's' => (0, 2),
        't' => (1, 2),
        'p' => (2, 2),
        'q' => (3, 2),
        '0' => (0, 3),
        '1' => (1, 3),
        '2' => (2, 3),
        '3' => (3, 3),
        _ => return None,
    })
}

pub(crate) struct Compiler<'s> {
    scope: &'s mut dyn Scope,
    /// Next free `smooth()` state slot.
    pub(crate) smooth_slots: u32,
}

impl<'s> Compiler<'s> {
    pub(crate) fn new(scope: &'s mut dyn Scope, first_smooth_slot: u32) -> Self {
        Self {
            scope,
            smooth_slots: first_smooth_slot,
        }
    }

    /// Compile `e` and convert the result to `to` (the declared type of a custom
    /// uniform): numbers and booleans convert freely, scalars splat into vectors and
    /// longer vectors truncate.
    pub(crate) fn compile_as(&mut self, e: &Expr, to: ValueType) -> Result<Node, ExprError> {
        use ValueType as T;
        let t = self.compile(e)?;
        let scalar = |t: T| matches!(t, T::Float | T::Int | T::Bool);
        let ok = t.ty == to
            || (scalar(t.ty) && scalar(to))
            || (scalar(t.ty) && to.vector_len().is_some())
            || matches!((t.ty.vector_len(), to.vector_len()), (Some(m), Some(n)) if m > n);
        if !ok {
            return Err(type_err(
                e.span,
                format!("cannot convert a {} result to {to}", t.ty),
            ));
        }
        Ok(conv(t, to).node)
    }

    /// Compile `e`. This function recurses once per tree level, so it only
    /// dispatches; all non-trivial work happens in separate, non-inlined functions
    /// to keep the recursive stack frame small.
    pub(crate) fn compile(&mut self, e: &Expr) -> Result<Typed, ExprError> {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Ident(_) => {
                self.leaf(e)
            }
            ExprKind::Unary { op, operand } => {
                let a = self.compile(operand)?;
                self.unary(*op, a, operand.span, e.span)
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.compile(lhs)?;
                let r = self.compile(rhs)?;
                self.binary(*op, l, r, lhs.span, rhs.span, e.span)
            }
            ExprKind::Call {
                name,
                name_span,
                args,
            } => {
                let func = self.lookup(name, *name_span, args.len())?;
                if let Func::Smooth = func {
                    return self.smooth(*name_span, args);
                }
                let mut a = Vec::with_capacity(args.len());
                for arg in args {
                    a.push(self.compile(arg)?);
                }
                self.call(func, name, *name_span, args, a)
            }
            ExprKind::Member {
                base,
                member,
                member_span,
            } => {
                let t = self.compile(base)?;
                self.member(t, member, *member_span)
            }
        }
    }

    #[inline(never)]
    fn leaf(&mut self, e: &Expr) -> Result<Typed, ExprError> {
        match &e.kind {
            ExprKind::Int(i) => Ok(typed(Node::Const(Value::Int(*i)), ValueType::Int)),
            ExprKind::Float(f) => Ok(typed(Node::Const(Value::Float(*f)), ValueType::Float)),
            ExprKind::Bool(v) => Ok(typed(Node::Const(Value::Bool(*v)), ValueType::Bool)),
            ExprKind::Ident(name) => match self.scope.resolve(name) {
                Ok(Some(Resolved::Const(v))) => Ok(typed(Node::Const(v), v.ty())),
                Ok(Some(Resolved::Input(slot, ty))) => Ok(typed(Node::Input(slot), ty)),
                Ok(Some(Resolved::Var(slot, ty))) => Ok(typed(Node::Var(slot), ty)),
                Ok(None) => Err(ExprError::new(
                    ExprErrorKind::UnknownIdentifier,
                    e.span.start,
                    format!("unknown identifier `{name}`"),
                )),
                Err(msg) => Err(type_err(e.span, msg)),
            },
            _ => Err(type_err(e.span, "internal error: not a leaf")),
        }
    }

    #[inline(never)]
    fn unary(
        &mut self,
        op: UnaryOp,
        a: Typed,
        operand: Span,
        span: Span,
    ) -> Result<Typed, ExprError> {
        match op {
            UnaryOp::Neg => match a.ty {
                ValueType::Bool | ValueType::Mat4 => Err(type_err(
                    span,
                    format!("unary `-` cannot be applied to {}", a.ty),
                )),
                ty => Ok(typed(Node::Neg(b(a)), ty)),
            },
            UnaryOp::Not => {
                let a = self.condition(a, operand, "operand of `!`")?;
                Ok(typed(Node::Not(b(a)), ValueType::Bool))
            }
        }
    }

    #[inline(never)]
    fn lookup(&self, name: &str, name_span: Span, n: usize) -> Result<Func, ExprError> {
        let func = Func::lookup(name).ok_or_else(|| {
            ExprError::new(
                ExprErrorKind::UnknownFunction,
                name_span.start,
                format!("unknown function `{name}`"),
            )
        })?;
        func.check_arity(n).map_err(|expected| {
            ExprError::new(
                ExprErrorKind::Arity,
                name_span.start,
                format!("`{name}` expects {expected}, found {n}"),
            )
        })?;
        Ok(func)
    }

    /// Accept a boolean (or, leniently, a number) as a condition.
    fn condition(&self, t: Typed, span: Span, what: &str) -> Result<Typed, ExprError> {
        match t.ty {
            ValueType::Bool => Ok(t),
            ValueType::Int | ValueType::Float => Ok(conv(t, ValueType::Bool)),
            ty => Err(type_err(
                span,
                format!("{what} must be a boolean, found {ty}"),
            )),
        }
    }

    /// Numeric scalar argument, converted to `to` (`Float` or `Int`).
    fn scalar_arg(
        &self,
        t: Typed,
        span: Span,
        func: &str,
        to: ValueType,
    ) -> Result<Typed, ExprError> {
        if !t.ty.is_numeric_scalar() {
            return Err(type_err(
                span,
                format!("`{func}` expects a number here, found {}", t.ty),
            ));
        }
        Ok(conv(t, to))
    }

    /// Two scalar operands for comparisons: booleans count as 0/1, ints stay ints
    /// when both sides are ints, otherwise both become floats.
    fn scalar_pair(
        &self,
        l: Typed,
        r: Typed,
        op: BinaryOp,
        span: Span,
    ) -> Result<(Typed, Typed), ExprError> {
        let ok = |t: ValueType| matches!(t, ValueType::Float | ValueType::Int | ValueType::Bool);
        if !ok(l.ty) || !ok(r.ty) {
            return Err(type_err(
                span,
                format!(
                    "operator `{}` cannot compare {} with {}",
                    op.symbol(),
                    l.ty,
                    r.ty
                ),
            ));
        }
        let int_like = |t: ValueType| matches!(t, ValueType::Int | ValueType::Bool);
        let to = if int_like(l.ty) && int_like(r.ty) {
            ValueType::Int
        } else {
            ValueType::Float
        };
        Ok((conv(l, to), conv(r, to)))
    }

    #[inline(never)]
    fn binary(
        &mut self,
        op: BinaryOp,
        l: Typed,
        r: Typed,
        ls: Span,
        rs: Span,
        span: Span,
    ) -> Result<Typed, ExprError> {
        let cmp = |op: BinaryOp| match op {
            BinaryOp::Lt => CmpOp::Lt,
            BinaryOp::Gt => CmpOp::Gt,
            BinaryOp::Le => CmpOp::Le,
            BinaryOp::Ge => CmpOp::Ge,
            BinaryOp::Eq => CmpOp::Eq,
            _ => CmpOp::Ne,
        };
        match op {
            BinaryOp::And | BinaryOp::Or => {
                let what = format!("operand of `{}`", op.symbol());
                let l = self.condition(l, ls, &what)?;
                let r = self.condition(r, rs, &what)?;
                let node = if op == BinaryOp::And {
                    Node::And(b(l), b(r))
                } else {
                    Node::Or(b(l), b(r))
                };
                Ok(typed(node, ValueType::Bool))
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                let (l, r) = self.scalar_pair(l, r, op, span)?;
                Ok(typed(Node::Compare(cmp(op), b(l), b(r)), ValueType::Bool))
            }
            BinaryOp::Eq | BinaryOp::Ne => {
                if l.ty == r.ty && !l.ty.is_numeric_scalar() {
                    return Ok(typed(Node::Compare(cmp(op), b(l), b(r)), ValueType::Bool));
                }
                let (l, r) = self.scalar_pair(l, r, op, span)?;
                Ok(typed(Node::Compare(cmp(op), b(l), b(r)), ValueType::Bool))
            }
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                let aop = match op {
                    BinaryOp::Add => ArithOp::Add,
                    BinaryOp::Sub => ArithOp::Sub,
                    BinaryOp::Mul => ArithOp::Mul,
                    BinaryOp::Div => ArithOp::Div,
                    _ => ArithOp::Rem,
                };
                let r_ty = unify_numeric(&[l.ty, r.ty], aop != ArithOp::Div).map_err(|m| {
                    type_err(
                        span,
                        format!(
                            "operator `{}` {m} ({} {} {})",
                            op.symbol(),
                            l.ty,
                            op.symbol(),
                            r.ty
                        ),
                    )
                })?;
                let l = coerce_operand(l, r_ty);
                let r = coerce_operand(r, r_ty);
                Ok(typed(Node::Arith(aop, b(l), b(r)), r_ty))
            }
        }
    }

    #[inline(never)]
    fn member(&mut self, t: Typed, member: &str, span: Span) -> Result<Typed, ExprError> {
        match t.ty {
            ValueType::Vec2 | ValueType::Vec3 | ValueType::Vec4 => {
                let n = t.ty.vector_len().unwrap_or(4);
                let chars: Vec<char> = member.chars().collect();
                let bad = || {
                    type_err(
                        span,
                        format!("invalid component selection `.{member}` on {}", t.ty),
                    )
                };
                if chars.is_empty() || chars.len() > 4 {
                    return Err(bad());
                }
                let mut comps = [0u8; 4];
                let mut family = None;
                for (slot, &c) in comps.iter_mut().zip(&chars) {
                    let (idx, fam) = component_index(c).ok_or_else(bad)?;
                    if family.is_some_and(|f| f != fam) || (fam == 3 && chars.len() > 1) {
                        return Err(bad());
                    }
                    family = Some(fam);
                    if usize::from(idx) >= n {
                        return Err(type_err(
                            span,
                            format!("component `.{c}` is out of range for {}", t.ty),
                        ));
                    }
                    *slot = idx;
                }
                let len = chars.len();
                let ty = if len == 1 {
                    ValueType::Float
                } else {
                    ValueType::vector(len).unwrap_or(ValueType::Vec4)
                };
                Ok(typed(
                    Node::Swizzle {
                        base: b(t),
                        comps,
                        len: len as u8,
                    },
                    ty,
                ))
            }
            ValueType::Mat4 => {
                let mut chars = member.chars();
                match (chars.next().and_then(component_index), chars.next()) {
                    (Some((idx, _)), None) => Ok(typed(Node::Column(b(t), idx), ValueType::Vec4)),
                    _ => Err(type_err(
                        span,
                        format!(
                            "invalid matrix access `.{member}`; use `.0`..`.3` to select a column"
                        ),
                    )),
                }
            }
            ty => Err(type_err(
                span,
                format!("cannot access `.{member}` on a {ty} value"),
            )),
        }
    }

    #[inline(never)]
    fn call(
        &mut self,
        func: Func,
        name: &str,
        name_span: Span,
        args: &[Expr],
        a: Vec<Typed>,
    ) -> Result<Typed, ExprError> {
        let spans: Vec<Span> = args.iter().map(|e| e.span).collect();
        let types: Vec<ValueType> = a.iter().map(|t| t.ty).collect();
        let unify = |int_ok: bool| {
            unify_numeric(&types, int_ok).map_err(|m| type_err(name_span, format!("`{name}` {m}")))
        };
        let internal = || type_err(name_span, format!("internal error: bad arity for `{name}`"));

        match func {
            Func::M1(f) => {
                let [x] = take(a).ok_or_else(internal)?;
                self.math1(f, x, name, spans[0])
            }
            Func::Atan | Func::Log if args.len() == 1 => {
                let f = if matches!(func, Func::Atan) {
                    Math1::Atan
                } else {
                    Math1::Ln
                };
                let [x] = take(a).ok_or_else(internal)?;
                self.math1(f, x, name, spans[0])
            }
            Func::Atan | Func::Log | Func::M2(_) => {
                let f = match func {
                    Func::Atan => Math2::Atan2,
                    Func::Log => Math2::LogBase,
                    Func::M2(f) => f,
                    _ => return Err(internal()),
                };
                let r = unify(f.int_preserving())?;
                let [x, y] = take(a).ok_or_else(internal)?;
                let (x, y) = (coerce_operand(x, r), coerce_operand(y, r));
                Ok(typed(Node::Math2(f, b(x), b(y)), r))
            }
            Func::MinMax(f) => {
                let r = unify(true)?;
                let nodes: Box<[Node]> = a.into_iter().map(|t| coerce_operand(t, r).node).collect();
                Ok(typed(Node::Fold(f, nodes), r))
            }
            Func::Clamp | Func::Mix | Func::Lerp => {
                let r = unify(matches!(func, Func::Clamp))?;
                let [x, y, z] = take(a)
                    .ok_or_else(internal)?
                    .map(|t| coerce_operand(t, r).node);
                Ok(match func {
                    Func::Clamp => typed(Node::Clamp(Box::new([x, y, z])), r),
                    Func::Mix => typed(Node::Mix(Box::new([x, y, z])), r),
                    // lerp(k, x, y) = mix(x, y, k)
                    _ => typed(Node::Mix(Box::new([y, z, x])), r),
                })
            }
            Func::Random => {
                if a.is_empty() {
                    return Ok(typed(Node::Random, ValueType::Float));
                }
                let [lo, hi] = take(a).ok_or_else(internal)?;
                let lo = self.scalar_arg(lo, spans[0], name, ValueType::Float)?;
                let hi = self.scalar_arg(hi, spans[1], name, ValueType::Float)?;
                Ok(typed(Node::RandomRange(b(lo), b(hi)), ValueType::Float))
            }
            Func::RandomInt => match a.len() {
                0 => Ok(typed(Node::RandomInt, ValueType::Int)),
                1 => {
                    let [n] = take(a).ok_or_else(internal)?;
                    let n = self.scalar_arg(n, spans[0], name, ValueType::Int)?;
                    Ok(typed(Node::RandomIntBound(b(n)), ValueType::Int))
                }
                _ => {
                    let [lo, hi] = take(a).ok_or_else(internal)?;
                    let lo = self.scalar_arg(lo, spans[0], name, ValueType::Int)?;
                    let hi = self.scalar_arg(hi, spans[1], name, ValueType::Int)?;
                    Ok(typed(Node::RandomIntRange(b(lo), b(hi)), ValueType::Int))
                }
            },
            Func::If | Func::Ifb => self.if_(a, &spans, name, name_span, matches!(func, Func::Ifb)),
            Func::Between => {
                for (t, s) in a.iter().zip(&spans) {
                    if !t.ty.is_numeric_scalar() {
                        return Err(type_err(
                            *s,
                            format!("`{name}` expects numbers, found {}", t.ty),
                        ));
                    }
                }
                let to = if types.iter().all(|t| *t == ValueType::Int) {
                    ValueType::Int
                } else {
                    ValueType::Float
                };
                let [x, lo, hi] = take(a).ok_or_else(internal)?.map(|t| conv(t, to).node);
                Ok(typed(Node::Between(Box::new([x, lo, hi])), ValueType::Bool))
            }
            Func::Equals => {
                if a.len() == 2 {
                    // Iris also exposes the `==` operator as `equals(a, b)`.
                    let [l, r] = take(a).ok_or_else(internal)?;
                    let span = Span::new(spans[0].start, spans[1].end);
                    return self.binary(BinaryOp::Eq, l, r, spans[0], spans[1], span);
                }
                let [x, y, eps] = take(a).ok_or_else(internal)?;
                let x = self.scalar_arg(x, spans[0], name, ValueType::Float)?.node;
                let y = self.scalar_arg(y, spans[1], name, ValueType::Float)?.node;
                let eps = self.scalar_arg(eps, spans[2], name, ValueType::Float)?.node;
                Ok(typed(
                    Node::EqualsEps(Box::new([x, y, eps])),
                    ValueType::Bool,
                ))
            }
            Func::In => {
                let mut nodes = Vec::with_capacity(a.len());
                for (t, s) in a.into_iter().zip(&spans) {
                    nodes.push(self.scalar_arg(t, *s, name, ValueType::Float)?.node);
                }
                Ok(typed(Node::In(nodes.into_boxed_slice()), ValueType::Bool))
            }
            Func::Vec(n, kind) => self.construct(n, kind, a, &spans, name, name_span),
            Func::Print => {
                // OptiFine `print(id, n, x)` logs x every n-th frame and returns x.
                let [_, _, x] = take(a).ok_or_else(internal)?;
                Ok(x)
            }
            Func::Smooth => Err(internal()),
        }
    }

    fn math1(&mut self, f: Math1, x: Typed, name: &str, span: Span) -> Result<Typed, ExprError> {
        match x.ty {
            ValueType::Int if f.int_preserving() => Ok(typed(Node::Math1(f, b(x)), ValueType::Int)),
            ValueType::Int | ValueType::Float => {
                let x = conv(x, ValueType::Float);
                Ok(typed(Node::Math1(f, b(x)), ValueType::Float))
            }
            ValueType::Vec2 | ValueType::Vec3 | ValueType::Vec4 => {
                let ty = x.ty;
                Ok(typed(Node::Math1(f, b(x)), ty))
            }
            ty => Err(type_err(
                span,
                format!("`{name}` expects a number or vector, found {ty}"),
            )),
        }
    }

    fn if_(
        &mut self,
        all: Vec<Typed>,
        spans: &[Span],
        name: &str,
        name_span: Span,
        ifb: bool,
    ) -> Result<Typed, ExprError> {
        let n = all.len();
        let is_cond = |i: usize| i.is_multiple_of(2) && i + 1 < n;
        let value_types: Vec<ValueType> = all
            .iter()
            .enumerate()
            .filter(|(i, _)| !is_cond(*i))
            .map(|(_, t)| t.ty)
            .collect();
        let result = if ifb || value_types.iter().all(|t| *t == ValueType::Bool) {
            ValueType::Bool
        } else if value_types.iter().all(|t| *t == ValueType::Mat4) {
            ValueType::Mat4
        } else {
            unify_numeric(&value_types, true)
                .map_err(|m| type_err(name_span, format!("`{name}` branches: {m}")))?
        };
        let mut nodes = Vec::with_capacity(n);
        for (i, (t, s)) in all.into_iter().zip(spans).enumerate() {
            let t = if is_cond(i) {
                self.condition(t, *s, &format!("condition of `{name}`"))?
            } else if result == ValueType::Bool {
                self.condition(t, *s, &format!("value of `{name}`"))?
            } else {
                conv(t, result)
            };
            nodes.push(t.node);
        }
        Ok(typed(Node::If(nodes.into_boxed_slice()), result))
    }

    fn construct(
        &mut self,
        n: usize,
        kind: VecKind,
        all: Vec<Typed>,
        spans: &[Span],
        name: &str,
        name_span: Span,
    ) -> Result<Typed, ExprError> {
        // GLSL constructor rules: one scalar splats; otherwise the arguments must supply
        // at least `n` components and the last argument must be (at least partly)
        // used. Unused trailing components of the last argument are dropped, so a
        // single longer vector truncates (`vec3(v4)`, `vec3(v2, v2)`).
        let mut total = 0;
        let mut before_last = 0;
        let mut parts = Vec::with_capacity(all.len());
        let single = all.len() == 1;
        for (t, s) in all.into_iter().zip(spans) {
            before_last = total;
            match t.ty {
                ValueType::Float | ValueType::Int | ValueType::Bool => {
                    total += 1;
                    parts.push(conv(t, ValueType::Float).node);
                }
                ValueType::Vec2 | ValueType::Vec3 | ValueType::Vec4 => {
                    total += t.ty.vector_len().unwrap_or(0);
                    parts.push(t.node);
                }
                ValueType::Mat4 => {
                    return Err(type_err(
                        *s,
                        format!("`{name}` cannot take a mat4 argument"),
                    ));
                }
            }
        }
        if !((single && total == 1) || (total >= n && before_last < n)) {
            return Err(ExprError::new(
                ExprErrorKind::Arity,
                name_span.start,
                format!("`{name}` expects {n} components, found {total}"),
            ));
        }
        let ty = ValueType::vector(n).unwrap_or(ValueType::Vec4);
        let mut node = fold(Node::Construct(n as u8, parts.into_boxed_slice()));
        match kind {
            VecKind::Float => {}
            VecKind::Int => node = fold(Node::Math1(Math1::Trunc, Box::new(node))),
            VecKind::Bool => node = fold(Node::Math1(Math1::NonZero, Box::new(node))),
        }
        Ok(Typed { node, ty })
    }

    /// `smooth([id,] value [, fadeUp [, fadeDown]])`. As in OptiFine and Iris, the
    /// first argument is an id when it is a bare number literal and more arguments
    /// follow (or when there are four arguments). As in Iris, every call site keeps
    /// its own state; the id is accepted for compatibility only.
    #[inline(never)]
    fn smooth(&mut self, name_span: Span, args: &[Expr]) -> Result<Typed, ExprError> {
        let literal = |e: &Expr| matches!(e.kind, ExprKind::Int(_) | ExprKind::Float(_));
        let has_id = args.len() == 4 || (args.len() >= 2 && literal(&args[0]));
        if has_id && !literal(&args[0]) {
            // Four arguments with a computed id (OptiFine evaluates it at run time).
            // The id never selects state here, but it must still be a valid number
            // expression: unknown identifiers in it are errors like anywhere else.
            let id = self.compile(&args[0])?;
            self.scalar_arg(id, args[0].span, "smooth", ValueType::Float)?;
        }
        let rest = if has_id { &args[1..] } else { args };
        let Some((value_expr, fades)) = rest.split_first() else {
            return Err(ExprError::new(
                ExprErrorKind::Arity,
                name_span.start,
                "`smooth` expects a value",
            ));
        };
        let v = self.compile(value_expr)?;
        let value = match v.ty {
            ValueType::Int | ValueType::Float => conv(v, ValueType::Float),
            ValueType::Vec2 | ValueType::Vec3 | ValueType::Vec4 => v,
            ty => {
                return Err(type_err(
                    value_expr.span,
                    format!("`smooth` expects a number or vector, found {ty}"),
                ));
            }
        };
        let mut fade = |e: Option<&Expr>| -> Result<Option<Box<Node>>, ExprError> {
            let Some(e) = e else { return Ok(None) };
            let t = self.compile(e)?;
            Ok(Some(b(self.scalar_arg(
                t,
                e.span,
                "smooth",
                ValueType::Float,
            )?)))
        };
        let up = fade(fades.first())?.unwrap_or_else(|| Box::new(Node::Const(Value::Float(1.0))));
        let down = fade(fades.get(1))?;
        let slot = self.smooth_slots;
        self.smooth_slots += 1;
        let ty = value.ty;
        Ok(Typed {
            node: Node::Smooth {
                slot,
                value: b(value),
                up,
                down,
            },
            ty,
        })
    }
}
