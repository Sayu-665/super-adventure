//! The compiled, type-resolved expression tree and its evaluator.
//!
//! Nodes reference inputs, variables and smoothing state by slot index, so
//! evaluation performs no name lookups and no allocation.

use crate::rng::Rng;
use crate::value::{Value, ValueType, from_lanes, lanes};

/// Arithmetic operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

/// Comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CmpOp {
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

/// Unary component-wise math functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Math1 {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    ToRad,
    ToDeg,
    Exp,
    Exp2,
    Exp10,
    Ln,
    Log2,
    Log10,
    Sqrt,
    InvSqrt,
    Frac,
    Abs,
    Sign,
    Floor,
    Ceil,
    Round,
    /// Truncation toward zero (used by `ivecN` constructors).
    Trunc,
    /// `x != 0 ? 1 : 0` (used by `bvecN` constructors).
    NonZero,
}

impl Math1 {
    /// Functions that map `int -> int` instead of promoting to float.
    pub(crate) const fn int_preserving(self) -> bool {
        matches!(
            self,
            Math1::Abs | Math1::Sign | Math1::Floor | Math1::Ceil | Math1::Round | Math1::Trunc
        )
    }
}

/// Binary component-wise math functions (scalars broadcast over vectors).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Math2 {
    /// `atan2(y, x)`
    Atan2,
    Pow,
    /// `log(base, value)`
    LogBase,
    /// Floored modulo (`Math.floorMod` for ints).
    Fmod,
    /// `edge(e, x)`: `x < e ? 0 : 1`.
    Edge,
    Min,
    Max,
}

impl Math2 {
    /// Functions that map `(int, int) -> int` instead of promoting to float.
    pub(crate) const fn int_preserving(self) -> bool {
        matches!(self, Math2::Fmod | Math2::Edge | Math2::Min | Math2::Max)
    }
}

/// A compiled expression node. Operand types were checked and conversions made
/// explicit by the compiler, so evaluation only dispatches on value shapes.
#[derive(Debug, Clone)]
pub(crate) enum Node {
    Const(Value),
    Input(u32),
    Var(u32),
    Convert(ValueType, Box<Node>),
    Neg(Box<Node>),
    Not(Box<Node>),
    Arith(ArithOp, Box<Node>, Box<Node>),
    Compare(CmpOp, Box<Node>, Box<Node>),
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
    /// Component selection; `len == 1` yields a float.
    Swizzle {
        base: Box<Node>,
        comps: [u8; 4],
        len: u8,
    },
    /// Matrix column (GLSL `m[i]`).
    Column(Box<Node>, u8),
    Math1(Math1, Box<Node>),
    Math2(Math2, Box<Node>, Box<Node>),
    /// Left fold of a binary function over 2+ operands (variadic `min`/`max`).
    Fold(Math2, Box<[Node]>),
    /// `[x, lo, hi]`
    Clamp(Box<[Node; 3]>),
    /// `[x, y, a]` -> `x + (y - x) * a`
    Mix(Box<[Node; 3]>),
    /// `[c0, v0, c1, v1, ..., else]`
    If(Box<[Node]>),
    /// `[x, lo, hi]`
    Between(Box<[Node; 3]>),
    /// `[a, b, eps]`
    EqualsEps(Box<[Node; 3]>),
    /// `[x, v1, v2, ...]`
    In(Box<[Node]>),
    /// Float vector of the given size from scalar/vector parts (one scalar splats).
    Construct(u8, Box<[Node]>),
    Random,
    RandomRange(Box<Node>, Box<Node>),
    RandomInt,
    RandomIntBound(Box<Node>),
    RandomIntRange(Box<Node>, Box<Node>),
    Smooth {
        slot: u32,
        value: Box<Node>,
        up: Box<Node>,
        down: Option<Box<Node>>,
    },
}

impl Node {
    pub(crate) fn for_each_child(&self, f: &mut dyn FnMut(&Node)) {
        match self {
            Node::Const(_) | Node::Input(_) | Node::Var(_) | Node::Random | Node::RandomInt => {}
            Node::Convert(_, a)
            | Node::Neg(a)
            | Node::Not(a)
            | Node::Swizzle { base: a, .. }
            | Node::Column(a, _)
            | Node::Math1(_, a)
            | Node::RandomIntBound(a) => f(a),
            Node::Arith(_, a, b)
            | Node::Compare(_, a, b)
            | Node::And(a, b)
            | Node::Or(a, b)
            | Node::Math2(_, a, b)
            | Node::RandomRange(a, b)
            | Node::RandomIntRange(a, b) => {
                f(a);
                f(b);
            }
            Node::Clamp(xs) | Node::Mix(xs) | Node::Between(xs) | Node::EqualsEps(xs) => {
                xs.iter().for_each(f)
            }
            Node::Fold(_, xs) | Node::If(xs) | Node::In(xs) | Node::Construct(_, xs) => {
                xs.iter().for_each(f)
            }
            Node::Smooth {
                value, up, down, ..
            } => {
                f(value);
                f(up);
                if let Some(d) = down {
                    f(d);
                }
            }
        }
    }

    pub(crate) fn for_each_child_mut(&mut self, f: &mut dyn FnMut(&mut Node)) {
        match self {
            Node::Const(_) | Node::Input(_) | Node::Var(_) | Node::Random | Node::RandomInt => {}
            Node::Convert(_, a)
            | Node::Neg(a)
            | Node::Not(a)
            | Node::Swizzle { base: a, .. }
            | Node::Column(a, _)
            | Node::Math1(_, a)
            | Node::RandomIntBound(a) => f(a),
            Node::Arith(_, a, b)
            | Node::Compare(_, a, b)
            | Node::And(a, b)
            | Node::Or(a, b)
            | Node::Math2(_, a, b)
            | Node::RandomRange(a, b)
            | Node::RandomIntRange(a, b) => {
                f(a);
                f(b);
            }
            Node::Clamp(xs) | Node::Mix(xs) | Node::Between(xs) | Node::EqualsEps(xs) => {
                xs.iter_mut().for_each(f)
            }
            Node::Fold(_, xs) | Node::If(xs) | Node::In(xs) | Node::Construct(_, xs) => {
                xs.iter_mut().for_each(f)
            }
            Node::Smooth {
                value, up, down, ..
            } => {
                f(value);
                f(up);
                if let Some(d) = down {
                    f(d);
                }
            }
        }
    }

    /// Rewrite input and variable slot indices.
    pub(crate) fn remap(&mut self, input: &dyn Fn(u32) -> u32, var: &dyn Fn(u32) -> u32) {
        match self {
            Node::Input(i) => *i = input(*i),
            Node::Var(i) => *i = var(*i),
            _ => self.for_each_child_mut(&mut |c| c.remap(input, var)),
        }
    }

    /// `true` if the node is pure and all its operands are constants.
    fn foldable(&self) -> bool {
        match self {
            Node::Const(_)
            | Node::Input(_)
            | Node::Var(_)
            | Node::Random
            | Node::RandomInt
            | Node::RandomRange(..)
            | Node::RandomIntBound(_)
            | Node::RandomIntRange(..)
            | Node::Smooth { .. } => false,
            _ => {
                let mut all = true;
                self.for_each_child(&mut |c| all &= matches!(c, Node::Const(_)));
                all
            }
        }
    }
}

/// Constant-fold `node` if it is pure and all operands are constants.
pub(crate) fn fold(node: Node) -> Node {
    if !node.foldable() {
        return node;
    }
    let mut rng = Rng::new(0);
    let mut cx = Ctx {
        inputs: &[],
        vars: &[],
        smooth: &mut [],
        rng: &mut rng,
        dt: 0.0,
    };
    Node::Const(eval(&node, &mut cx))
}

/// Exponential smoothing state of one `smooth()` call site.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct SmoothState {
    acc: [f32; 4],
    init: bool,
}

/// Evaluation context.
pub(crate) struct Ctx<'a> {
    pub inputs: &'a [Value],
    pub vars: &'a [Value],
    pub smooth: &'a mut [SmoothState],
    pub rng: &'a mut Rng,
    /// Frame delta time in seconds (finite, >= 0).
    pub dt: f32,
}

/// Evaluate a node. The dispatcher keeps its own stack frame small (every
/// non-trivial case lives in a separate, non-inlined function) because it recurses
/// once per tree level.
pub(crate) fn eval(n: &Node, cx: &mut Ctx<'_>) -> Value {
    match n {
        Node::Const(v) => *v,
        Node::Input(i) => cx
            .inputs
            .get(*i as usize)
            .copied()
            .unwrap_or(Value::Float(0.0)),
        Node::Var(i) => cx
            .vars
            .get(*i as usize)
            .copied()
            .unwrap_or(Value::Float(0.0)),
        Node::Convert(t, a) => eval_convert(*t, a, cx),
        Node::Neg(a) => eval_neg(a, cx),
        Node::Not(a) => Value::Bool(!eval(a, cx).to_bool()),
        Node::Arith(op, a, b) => eval_arith(*op, a, b, cx),
        Node::Compare(op, a, b) => eval_compare(*op, a, b, cx),
        Node::And(a, b) | Node::Or(a, b) => eval_logic(matches!(n, Node::And(..)), a, b, cx),
        Node::Swizzle { base, comps, len } => eval_swizzle(base, comps, *len, cx),
        Node::Column(base, c) => eval_column(base, *c, cx),
        Node::Math1(f, a) => eval_math1(*f, a, cx),
        Node::Math2(f, a, b) => eval_math2(*f, a, b, cx),
        Node::Fold(f, xs) => eval_fold(*f, xs, cx),
        Node::Clamp(xs) => eval_clamp(xs, cx),
        Node::Mix(xs) => eval_mix(xs, cx),
        Node::If(xs) => eval_if(xs, cx),
        Node::Between(xs) => eval_between(xs, cx),
        Node::EqualsEps(xs) => eval_equals_eps(xs, cx),
        Node::In(xs) => eval_in(xs, cx),
        Node::Construct(n, parts) => eval_construct(*n, parts, cx),
        Node::Random => Value::Float(cx.rng.next_f32()),
        Node::RandomRange(a, b) => eval_random_range(a, b, cx),
        Node::RandomInt => Value::Int(cx.rng.next_i32()),
        Node::RandomIntBound(a) => eval_random_bound(a, cx),
        Node::RandomIntRange(a, b) => eval_random_int_range(a, b, cx),
        Node::Smooth {
            slot,
            value,
            up,
            down,
        } => eval_smooth(*slot, value, up, down.as_deref(), cx),
    }
}

#[inline(never)]
fn eval_convert(t: ValueType, a: &Node, cx: &mut Ctx<'_>) -> Value {
    eval(a, cx).convert(t)
}

#[inline(never)]
fn eval_neg(a: &Node, cx: &mut Ctx<'_>) -> Value {
    match eval(a, cx) {
        Value::Int(i) => Value::Int(i.wrapping_neg()),
        v => map1(v, |x| -x),
    }
}

#[inline(never)]
fn eval_arith(op: ArithOp, a: &Node, b: &Node, cx: &mut Ctx<'_>) -> Value {
    let a = eval(a, cx);
    let b = eval(b, cx);
    arith(op, a, b)
}

/// `&&` / `||`. Both operands are always evaluated, as in Iris (its `and`/`or` are
/// ordinary two-argument functions): a `smooth()` or `random()` on the right-hand
/// side keeps advancing whatever the left-hand side is. Short-circuiting (OptiFine)
/// would freeze such a `smooth()` and make it jump once the operand is evaluated
/// again, because smoothing here follows Iris and is driven by the frame delta.
#[inline(never)]
fn eval_logic(and: bool, a: &Node, b: &Node, cx: &mut Ctx<'_>) -> Value {
    let a = eval(a, cx).to_bool();
    let b = eval(b, cx).to_bool();
    Value::Bool(if and { a && b } else { a || b })
}

#[inline(never)]
fn eval_compare(op: CmpOp, a: &Node, b: &Node, cx: &mut Ctx<'_>) -> Value {
    let a = eval(a, cx);
    let b = eval(b, cx);
    Value::Bool(compare(op, a, b))
}

#[inline(never)]
fn eval_swizzle(base: &Node, comps: &[u8; 4], len: u8, cx: &mut Ctx<'_>) -> Value {
    let (l, _) = lanes(&eval(base, cx));
    let mut out = [0.0; 4];
    for (o, c) in out.iter_mut().zip(comps) {
        *o = l[usize::from(*c) & 3];
    }
    from_lanes(out, usize::from(len))
}

#[inline(never)]
fn eval_column(base: &Node, c: u8, cx: &mut Ctx<'_>) -> Value {
    match eval(base, cx) {
        Value::Mat4(m) => {
            let i = (usize::from(c) & 3) * 4;
            Value::Vec4([m[i], m[i + 1], m[i + 2], m[i + 3]])
        }
        other => other.convert(ValueType::Vec4),
    }
}

#[inline(never)]
fn eval_math1(f: Math1, a: &Node, cx: &mut Ctx<'_>) -> Value {
    let v = eval(a, cx);
    math1(f, v)
}

#[inline(never)]
fn eval_math2(f: Math2, a: &Node, b: &Node, cx: &mut Ctx<'_>) -> Value {
    let a = eval(a, cx);
    let b = eval(b, cx);
    math2(f, a, b)
}

#[inline(never)]
fn eval_fold(f: Math2, xs: &[Node], cx: &mut Ctx<'_>) -> Value {
    let mut it = xs.iter();
    let Some(first) = it.next() else {
        return Value::Float(0.0);
    };
    let mut acc = eval(first, cx);
    for x in it {
        let v = eval(x, cx);
        acc = math2(f, acc, v);
    }
    acc
}

#[inline(never)]
fn eval_clamp(xs: &[Node; 3], cx: &mut Ctx<'_>) -> Value {
    let [x, lo, hi] = eval3(xs, cx);
    match (x, lo, hi) {
        // Iris: Math.max(min, Math.min(max, val)); never panics when lo > hi.
        (Value::Int(x), Value::Int(lo), Value::Int(hi)) => Value::Int(lo.max(hi.min(x))),
        _ => zip3(x, lo, hi, |x, lo, hi| jmax(lo, jmin(hi, x))),
    }
}

#[inline(never)]
fn eval_mix(xs: &[Node; 3], cx: &mut Ctx<'_>) -> Value {
    let [x, y, a] = eval3(xs, cx);
    zip3(x, y, a, |x, y, a| x + (y - x) * a)
}

#[inline(never)]
fn eval_if(xs: &[Node], cx: &mut Ctx<'_>) -> Value {
    let mut i = 0;
    while i + 1 < xs.len() {
        if eval(&xs[i], cx).to_bool() {
            return eval(&xs[i + 1], cx);
        }
        i += 2;
    }
    xs.last().map_or(Value::Float(0.0), |e| eval(e, cx))
}

#[inline(never)]
fn eval_between(xs: &[Node; 3], cx: &mut Ctx<'_>) -> Value {
    let [x, lo, hi] = eval3(xs, cx);
    Value::Bool(match (x, lo, hi) {
        (Value::Int(x), Value::Int(lo), Value::Int(hi)) => x >= lo && x <= hi,
        _ => {
            let x = x.to_f32();
            x >= lo.to_f32() && x <= hi.to_f32()
        }
    })
}

#[inline(never)]
fn eval_equals_eps(xs: &[Node; 3], cx: &mut Ctx<'_>) -> Value {
    let [a, b, eps] = eval3(xs, cx);
    Value::Bool((a.to_f32() - b.to_f32()).abs() <= eps.to_f32())
}

#[inline(never)]
fn eval_in(xs: &[Node], cx: &mut Ctx<'_>) -> Value {
    let mut it = xs.iter();
    let Some(first) = it.next() else {
        return Value::Bool(false);
    };
    let x = eval(first, cx).to_f32();
    for v in it {
        if eval(v, cx).to_f32() == x {
            return Value::Bool(true);
        }
    }
    Value::Bool(false)
}

#[inline(never)]
fn eval_construct(n: u8, parts: &[Node], cx: &mut Ctx<'_>) -> Value {
    let mut out = [0.0; 4];
    let mut k = 0;
    for p in parts {
        let (l, m) = lanes(&eval(p, cx));
        for &x in &l[..m] {
            if let Some(o) = out.get_mut(k) {
                *o = x;
            }
            k += 1;
        }
    }
    if k == 1 {
        out = [out[0]; 4];
    }
    from_lanes(out, usize::from(n))
}

#[inline(never)]
fn eval_random_range(a: &Node, b: &Node, cx: &mut Ctx<'_>) -> Value {
    let lo = eval(a, cx).to_f32();
    let hi = eval(b, cx).to_f32();
    Value::Float(lo + cx.rng.next_f32() * (hi - lo))
}

#[inline(never)]
fn eval_random_bound(a: &Node, cx: &mut Ctx<'_>) -> Value {
    let bound = eval(a, cx).to_i32();
    Value::Int(cx.rng.below(bound))
}

#[inline(never)]
fn eval_random_int_range(a: &Node, b: &Node, cx: &mut Ctx<'_>) -> Value {
    let lo = eval(a, cx).to_i32();
    let hi = eval(b, cx).to_i32();
    Value::Int(cx.rng.range(lo, hi))
}

#[inline(never)]
fn eval_smooth(slot: u32, value: &Node, up: &Node, down: Option<&Node>, cx: &mut Ctx<'_>) -> Value {
    let target = eval(value, cx);
    let up = eval(up, cx).to_f32();
    let down = match down {
        Some(d) => eval(d, cx).to_f32(),
        None => up,
    };
    let (t, n) = lanes(&target);
    let dt = cx.dt;
    let Some(st) = cx.smooth.get_mut(slot as usize) else {
        return target;
    };
    if !st.init {
        // The first value is not smoothed.
        st.init = true;
        st.acc = t;
    } else {
        for (acc, &v) in st.acc.iter_mut().zip(&t).take(n) {
            *acc = smooth_step(*acc, v, up, down, dt);
        }
    }
    from_lanes(st.acc, n)
}

fn eval3(xs: &[Node; 3], cx: &mut Ctx<'_>) -> [Value; 3] {
    let a = eval(&xs[0], cx);
    let b = eval(&xs[1], cx);
    let c = eval(&xs[2], cx);
    [a, b, c]
}

/// One step of Iris' `SmoothFloat`: exponential smoothing whose half-life is a tenth
/// of the fade time, so the value is within 0.1% of the target after `fade` seconds.
fn smooth_step(acc: f32, value: f32, fade_up: f32, fade_down: f32, dt: f32) -> f32 {
    let fade = if value > acc { fade_up } else { fade_down };
    // A zero/negative/NaN fade time means "no smoothing"; a non-finite accumulator
    // (e.g. after a NaN target) recovers by snapping.
    if fade.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || !acc.is_finite() {
        return value;
    }
    let decay = std::f32::consts::LN_2 / (fade * 0.1);
    let alpha = 1.0 - (-decay * dt).exp();
    (1.0 - alpha) * acc + alpha * value
}

fn map1(v: Value, f: impl Fn(f32) -> f32) -> Value {
    let (l, n) = lanes(&v);
    let mut out = [0.0; 4];
    for (o, &x) in out.iter_mut().zip(&l).take(n) {
        *o = f(x);
    }
    from_lanes(out, n)
}

/// Component-wise binary operation with scalar broadcasting.
fn zip2(a: Value, b: Value, f: impl Fn(f32, f32) -> f32) -> Value {
    let (la, na) = lanes(&a);
    let (lb, nb) = lanes(&b);
    let n = na.max(nb);
    let mut out = [0.0; 4];
    for (i, o) in out.iter_mut().enumerate().take(n) {
        let x = if na == 1 { la[0] } else { la[i] };
        let y = if nb == 1 { lb[0] } else { lb[i] };
        *o = f(x, y);
    }
    from_lanes(out, n)
}

/// Component-wise ternary operation with scalar broadcasting.
fn zip3(a: Value, b: Value, c: Value, f: impl Fn(f32, f32, f32) -> f32) -> Value {
    let (la, na) = lanes(&a);
    let (lb, nb) = lanes(&b);
    let (lc, nc) = lanes(&c);
    let n = na.max(nb).max(nc);
    let pick = |l: &[f32; 4], len: usize, i: usize| if len == 1 { l[0] } else { l[i] };
    let mut out = [0.0; 4];
    for (i, o) in out.iter_mut().enumerate().take(n) {
        *o = f(pick(&la, na, i), pick(&lb, nb, i), pick(&lc, nc, i));
    }
    from_lanes(out, n)
}

fn arith(op: ArithOp, a: Value, b: Value) -> Value {
    if let (Value::Int(x), Value::Int(y)) = (a, b) {
        return match op {
            ArithOp::Add => Value::Int(x.wrapping_add(y)),
            ArithOp::Sub => Value::Int(x.wrapping_sub(y)),
            ArithOp::Mul => Value::Int(x.wrapping_mul(y)),
            // `/` is always a float division (Iris only defines float `divide`).
            ArithOp::Div => Value::Float(x as f32 / y as f32),
            // Java `%`, except that a zero divisor yields 0 instead of throwing.
            ArithOp::Rem => Value::Int(if y == 0 { 0 } else { x.wrapping_rem(y) }),
        };
    }
    zip2(a, b, |x, y| match op {
        ArithOp::Add => x + y,
        ArithOp::Sub => x - y,
        ArithOp::Mul => x * y,
        ArithOp::Div => x / y,
        ArithOp::Rem => x % y,
    })
}

fn compare(op: CmpOp, a: Value, b: Value) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    let by_order = |o: Option<std::cmp::Ordering>| match op {
        CmpOp::Lt => o == Some(Less),
        CmpOp::Gt => o == Some(Greater),
        CmpOp::Le => matches!(o, Some(Less | Equal)),
        CmpOp::Ge => matches!(o, Some(Greater | Equal)),
        CmpOp::Eq => o == Some(Equal),
        CmpOp::Ne => o != Some(Equal),
    };
    let by_eq = |eq: bool| match op {
        CmpOp::Eq => eq,
        CmpOp::Ne => !eq,
        _ => false,
    };
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => by_order(Some(x.cmp(&y))),
        (Value::Mat4(x), Value::Mat4(y)) => by_eq(x == y),
        (Value::Vec2(_) | Value::Vec3(_) | Value::Vec4(_), _)
        | (_, Value::Vec2(_) | Value::Vec3(_) | Value::Vec4(_)) => {
            let (la, na) = lanes(&a);
            let (lb, nb) = lanes(&b);
            by_eq(na == nb && la[..na] == lb[..nb])
        }
        _ => by_order(a.to_f32().partial_cmp(&b.to_f32())),
    }
}

/// `Math.min` semantics: NaN propagates.
fn jmin(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a <= b {
        a
    } else {
        b
    }
}

/// `Math.max` semantics: NaN propagates.
fn jmax(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a >= b {
        a
    } else {
        b
    }
}

/// `Math.signum` semantics: zero and NaN map to themselves.
fn jsignum(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

fn math1(f: Math1, v: Value) -> Value {
    if let Value::Int(i) = v
        && f.int_preserving()
    {
        return Value::Int(match f {
            Math1::Abs => i.wrapping_abs(),
            Math1::Sign => i.signum(),
            _ => i,
        });
    }
    map1(v, |x| apply1(f, x))
}

fn apply1(f: Math1, x: f32) -> f32 {
    // Transcendentals are computed in f64, like Java's Math, then rounded.
    let d = f64::from(x);
    let r = match f {
        Math1::Sin => d.sin(),
        Math1::Cos => d.cos(),
        Math1::Tan => d.tan(),
        Math1::Asin => d.asin(),
        Math1::Acos => d.acos(),
        Math1::Atan => d.atan(),
        Math1::ToRad => d.to_radians(),
        Math1::ToDeg => d.to_degrees(),
        Math1::Exp => d.exp(),
        Math1::Exp2 => d.exp2(),
        Math1::Exp10 => 10f64.powf(d),
        Math1::Ln => d.ln(),
        Math1::Log2 => d.log2(),
        Math1::Log10 => d.log10(),
        Math1::Sqrt => d.sqrt(),
        Math1::InvSqrt => 1.0 / d.sqrt(),
        Math1::Frac => d - d.floor(),
        // OptiFine `round` is `Math.round`: halves round toward +infinity.
        Math1::Round => (d + 0.5).floor(),
        Math1::Abs => return x.abs(),
        Math1::Sign => return jsignum(x),
        Math1::Floor => return x.floor(),
        Math1::Ceil => return x.ceil(),
        Math1::Trunc => return x.trunc(),
        Math1::NonZero => return if x != 0.0 { 1.0 } else { 0.0 },
    };
    r as f32
}

fn math2(f: Math2, a: Value, b: Value) -> Value {
    if let (Value::Int(x), Value::Int(y)) = (a, b)
        && f.int_preserving()
    {
        return Value::Int(match f {
            Math2::Fmod => floor_mod(x, y),
            Math2::Edge => i32::from(y >= x),
            Math2::Min => x.min(y),
            Math2::Max => x.max(y),
            _ => 0,
        });
    }
    zip2(a, b, |x, y| apply2(f, x, y))
}

/// `Math.floorMod` (result has the sign of the divisor); 0 for a zero divisor.
fn floor_mod(x: i32, y: i32) -> i32 {
    if y == 0 {
        return 0;
    }
    let r = x.wrapping_rem(y);
    if r != 0 && ((r ^ y) < 0) {
        r.wrapping_add(y)
    } else {
        r
    }
}

fn apply2(f: Math2, x: f32, y: f32) -> f32 {
    let (dx, dy) = (f64::from(x), f64::from(y));
    match f {
        Math2::Atan2 => dx.atan2(dy) as f32,
        Math2::Pow => dx.powf(dy) as f32,
        Math2::LogBase => (dy.ln() / dx.ln()) as f32,
        Math2::Fmod => (x % y + y) % y,
        Math2::Edge => {
            if y < x {
                0.0
            } else {
                1.0
            }
        }
        Math2::Min => jmin(x, y),
        Math2::Max => jmax(x, y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_mod_matches_java() {
        assert_eq!(floor_mod(7, 3), 1);
        assert_eq!(floor_mod(-7, 3), 2);
        assert_eq!(floor_mod(7, -3), -2);
        assert_eq!(floor_mod(-7, -3), -1);
        assert_eq!(floor_mod(6, 3), 0);
        assert_eq!(floor_mod(5, 0), 0);
        assert_eq!(floor_mod(i32::MIN, -1), 0);
    }

    #[test]
    fn smooth_step_behaviour() {
        // Zero fade snaps.
        assert_eq!(smooth_step(0.0, 1.0, 0.0, 0.0, 0.016), 1.0);
        // Half-life is fade/10: after fade/10 seconds, half the distance is covered.
        let v = smooth_step(0.0, 1.0, 1.0, 1.0, 0.1);
        assert!((v - 0.5).abs() < 1e-5, "{v}");
        // Up and down fade times are chosen by direction.
        let up = smooth_step(0.0, 1.0, 1.0, 100.0, 0.1);
        let down = smooth_step(1.0, 0.0, 1.0, 100.0, 0.1);
        assert!((up - 0.5).abs() < 1e-5);
        assert!(down > 0.99);
        // No time passed: no change.
        assert_eq!(smooth_step(0.25, 1.0, 1.0, 1.0, 0.0), 0.25);
        // NaN accumulator recovers.
        assert_eq!(smooth_step(f32::NAN, 2.0, 1.0, 1.0, 0.1), 2.0);
    }

    #[test]
    fn compare_semantics() {
        assert!(compare(CmpOp::Lt, Value::Int(1), Value::Int(2)));
        assert!(compare(
            CmpOp::Ne,
            Value::Float(f32::NAN),
            Value::Float(f32::NAN)
        ));
        assert!(!compare(
            CmpOp::Eq,
            Value::Float(f32::NAN),
            Value::Float(f32::NAN)
        ));
        assert!(compare(
            CmpOp::Eq,
            Value::Vec2([1.0, 2.0]),
            Value::Vec2([1.0, 2.0])
        ));
        assert!(compare(
            CmpOp::Ne,
            Value::Vec2([1.0, 2.0]),
            Value::Vec2([1.0, 3.0])
        ));
        assert!(!compare(
            CmpOp::Lt,
            Value::Vec2([1.0, 2.0]),
            Value::Vec2([3.0, 3.0])
        ));
        assert!(compare(
            CmpOp::Eq,
            Value::Mat4([1.0; 16]),
            Value::Mat4([1.0; 16])
        ));
    }

    #[test]
    fn folding_skips_impure_nodes() {
        let n = fold(Node::Arith(
            ArithOp::Add,
            Box::new(Node::Const(Value::Int(1))),
            Box::new(Node::Const(Value::Int(2))),
        ));
        assert!(matches!(n, Node::Const(Value::Int(3))));
        let n = fold(Node::Arith(
            ArithOp::Add,
            Box::new(Node::Random),
            Box::new(Node::Const(Value::Int(2))),
        ));
        assert!(matches!(n, Node::Arith(..)));
        assert!(matches!(fold(Node::Random), Node::Random));
    }
}
