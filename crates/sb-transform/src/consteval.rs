//! Small constant evaluator: array sizes, `local_size_*`, `layout(binding = N)` and
//! uniform initializers (`uniform float x = 1.0;`).

use std::collections::HashMap;

use crate::ast::*;

/// A constant value: scalar kind plus components (column-major for matrices).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConstVal {
    /// `float`, `int`, `uint`, `bool`, `double` (from [`sb_core::ScalarKind`]).
    pub kind: sb_core::ScalarKind,
    /// Components (rows * cols); bools are 0/1.
    pub comps: Vec<f64>,
    /// Matrix columns (1 for scalars and vectors).
    pub cols: u8,
}

impl ConstVal {
    fn scalar(kind: sb_core::ScalarKind, v: f64) -> Self {
        Self { kind, comps: vec![v], cols: 1 }
    }

    /// The value as a non-negative integer, if it is an integral scalar.
    pub fn as_u32(&self) -> Option<u32> {
        if self.comps.len() != 1 {
            return None;
        }
        let v = self.comps[0];
        (v >= 0.0 && v.fract() == 0.0 && v <= f64::from(u32::MAX)).then_some(v as u32)
    }
}

/// Global constants known so far (`const int N = 4;`), by name.
pub(crate) type ConstEnv = HashMap<String, ConstVal>;

fn scalar_kind_of(name: &str) -> Option<(sb_core::ScalarKind, u8, u8)> {
    let t = sb_core::GlslType::parse(name)?;
    Some((t.scalar, t.rows, t.cols))
}

fn wrap_int(kind: sb_core::ScalarKind, v: f64) -> f64 {
    match kind {
        sb_core::ScalarKind::Int => f64::from(v as i64 as i32),
        sb_core::ScalarKind::Uint => f64::from(v as i64 as u32),
        sb_core::ScalarKind::Bool => f64::from(u8::from(v != 0.0)),
        _ => v,
    }
}

/// Evaluate `e` to a constant, if it only uses literals, constructors, arithmetic,
/// swizzles of constants and constants from `env`. Depth-limited.
pub(crate) fn eval(e: &Expr, env: &ConstEnv) -> Option<ConstVal> {
    eval_depth(e, env, 0)
}

fn eval_depth(e: &Expr, env: &ConstEnv, depth: u32) -> Option<ConstVal> {
    use sb_core::ScalarKind as K;
    if depth > 64 {
        return None;
    }
    let d = depth + 1;
    Some(match e {
        Expr::Int(v) => ConstVal::scalar(K::Int, f64::from(*v)),
        Expr::UInt(v) => ConstVal::scalar(K::Uint, f64::from(*v)),
        Expr::Bool(v) => ConstVal::scalar(K::Bool, f64::from(u8::from(*v))),
        Expr::Float(v) => ConstVal::scalar(K::Float, f64::from(*v)),
        Expr::Double(v) => ConstVal::scalar(K::Double, *v),
        Expr::Ident(n) => env.get(n)?.clone(),
        Expr::Unary(op, a) => {
            let mut v = eval_depth(a, env, d)?;
            match op {
                UnaryOp::Minus => v.comps.iter_mut().for_each(|c| *c = -*c),
                UnaryOp::Plus => {}
                UnaryOp::Not => v.comps.iter_mut().for_each(|c| *c = f64::from(u8::from(*c == 0.0))),
                UnaryOp::Complement => {
                    if !matches!(v.kind, K::Int | K::Uint) {
                        return None;
                    }
                    v.comps.iter_mut().for_each(|c| *c = !(*c as i64) as f64);
                }
                UnaryOp::Inc | UnaryOp::Dec => return None,
            }
            v.comps.iter_mut().for_each(|c| *c = wrap_int(v.kind, *c));
            v
        }
        Expr::Binary(op, a, b) => {
            let a = eval_depth(a, env, d)?;
            let b = eval_depth(b, env, d)?;
            if a.cols > 1 || b.cols > 1 {
                return None;
            }
            let n = a.comps.len().max(b.comps.len());
            if !(a.comps.len() == n || a.comps.len() == 1) || !(b.comps.len() == n || b.comps.len() == 1) {
                return None;
            }
            let kind = if a.kind == K::Double || b.kind == K::Double {
                K::Double
            } else if a.kind == K::Float || b.kind == K::Float {
                K::Float
            } else if a.kind == K::Uint || b.kind == K::Uint {
                K::Uint
            } else {
                a.kind
            };
            let integral = matches!(kind, K::Int | K::Uint);
            let get = |v: &ConstVal, i: usize| if v.comps.len() == 1 { v.comps[0] } else { v.comps[i] };
            let mut comps = Vec::with_capacity(n);
            let mut bool_result = false;
            for i in 0..n {
                let (x, y) = (get(&a, i), get(&b, i));
                let r = match op {
                    BinaryOp::Add => x + y,
                    BinaryOp::Sub => x - y,
                    BinaryOp::Mult => x * y,
                    BinaryOp::Div => {
                        if integral {
                            if y == 0.0 {
                                return None;
                            }
                            (x / y).trunc()
                        } else {
                            x / y
                        }
                    }
                    BinaryOp::Mod => {
                        if !integral || y == 0.0 {
                            return None;
                        }
                        x % y
                    }
                    BinaryOp::LShift | BinaryOp::RShift | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor => {
                        if !integral {
                            return None;
                        }
                        let (xi, yi) = (x as i64, y as i64);
                        match op {
                            BinaryOp::LShift => (xi.checked_shl(u32::try_from(yi).ok()?)?) as f64,
                            BinaryOp::RShift => (xi.checked_shr(u32::try_from(yi).ok()?)?) as f64,
                            BinaryOp::BitAnd => (xi & yi) as f64,
                            BinaryOp::BitOr => (xi | yi) as f64,
                            _ => (xi ^ yi) as f64,
                        }
                    }
                    BinaryOp::Equal | BinaryOp::NonEqual | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Lte | BinaryOp::Gte => {
                        if n != 1 {
                            return None;
                        }
                        bool_result = true;
                        f64::from(u8::from(match op {
                            BinaryOp::Equal => x == y,
                            BinaryOp::NonEqual => x != y,
                            BinaryOp::Lt => x < y,
                            BinaryOp::Gt => x > y,
                            BinaryOp::Lte => x <= y,
                            _ => x >= y,
                        }))
                    }
                    BinaryOp::And | BinaryOp::Or | BinaryOp::Xor => {
                        bool_result = true;
                        let (p, q) = (x != 0.0, y != 0.0);
                        f64::from(u8::from(match op {
                            BinaryOp::And => p && q,
                            BinaryOp::Or => p || q,
                            _ => p != q,
                        }))
                    }
                };
                comps.push(r);
            }
            let kind = if bool_result { K::Bool } else { kind };
            let comps = comps.into_iter().map(|c| wrap_int(kind, c)).collect();
            ConstVal { kind, comps, cols: 1 }
        }
        Expr::Ternary(c, a, b) => {
            let c = eval_depth(c, env, d)?;
            if c.comps.len() != 1 {
                return None;
            }
            if c.comps[0] != 0.0 { eval_depth(a, env, d)? } else { eval_depth(b, env, d)? }
        }
        Expr::Field(a, sw) => {
            let v = eval_depth(a, env, d)?;
            if v.cols > 1 {
                return None;
            }
            let mut comps = Vec::with_capacity(sw.len());
            for ch in sw.chars() {
                let i = match ch {
                    'x' | 'r' | 's' => 0,
                    'y' | 'g' | 't' => 1,
                    'z' | 'b' | 'p' => 2,
                    'w' | 'a' | 'q' => 3,
                    _ => return None,
                };
                comps.push(*v.comps.get(i)?);
            }
            if comps.is_empty() || comps.len() > 4 {
                return None;
            }
            ConstVal { kind: v.kind, comps, cols: 1 }
        }
        Expr::Call(Callee::Name(name), args) => {
            let (kind, rows, cols) = scalar_kind_of(name)?;
            let n = usize::from(rows) * usize::from(cols);
            let vals: Vec<ConstVal> = args.iter().map(|a| eval_depth(a, env, d)).collect::<Option<_>>()?;
            let mut comps: Vec<f64> = Vec::with_capacity(n);
            if vals.len() == 1 && vals[0].comps.len() == 1 {
                let x = vals[0].comps[0];
                if cols > 1 {
                    // Diagonal matrix.
                    for c in 0..cols {
                        for r in 0..rows {
                            comps.push(if r == c { x } else { 0.0 });
                        }
                    }
                } else {
                    comps.resize(n, x);
                }
            } else if vals.len() == 1 && vals[0].cols > 1 && cols > 1 {
                // Matrix from matrix: copy the overlapping part, identity elsewhere.
                let src = &vals[0];
                let src_cols = usize::from(src.cols);
                let src_rows = src.comps.len() / src_cols.max(1);
                for c in 0..usize::from(cols) {
                    for r in 0..usize::from(rows) {
                        comps.push(if c < src_cols && r < src_rows {
                            src.comps[c * src_rows + r]
                        } else if r == c {
                            1.0
                        } else {
                            0.0
                        });
                    }
                }
            } else {
                for v in &vals {
                    comps.extend_from_slice(&v.comps);
                }
                if comps.len() < n {
                    return None;
                }
                comps.truncate(n);
            }
            let comps = comps
                .into_iter()
                .map(|c| match kind {
                    K::Int | K::Uint => wrap_int(kind, c.trunc()),
                    K::Bool => f64::from(u8::from(c != 0.0)),
                    _ => c,
                })
                .collect();
            ConstVal { kind, comps, cols }
        }
        _ => return None,
    })
}

/// Evaluate an array dimension expression to a positive length.
pub(crate) fn array_len(e: &Expr, env: &ConstEnv) -> Option<u32> {
    eval(e, env).and_then(|v| v.as_u32()).filter(|&n| n > 0)
}

/// Collect the global `const` scalars/vectors of `unit` with constant initializers.
pub(crate) fn global_consts(unit: &TranslationUnit) -> ConstEnv {
    let mut env = ConstEnv::new();
    for item in &unit.items {
        let ItemKind::Decl(d) = &item.kind else { continue };
        if !d.ty.has_storage(&Storage::Const) || !d.ty.ty.array.is_empty() {
            continue;
        }
        let Some(name) = d.ty.ty.name() else { continue };
        let Some((kind, rows, cols)) = scalar_kind_of(name) else { continue };
        for v in &d.vars {
            if !v.array.is_empty() {
                continue;
            }
            let Some(Init::Expr(init)) = &v.init else { continue };
            if let Some(mut c) = eval(init, &env)
                && c.comps.len() == usize::from(rows) * usize::from(cols)
            {
                // Implicit conversion of the initializer to the declared type.
                c.comps = c.comps.into_iter().map(|x| if matches!(kind, sb_core::ScalarKind::Int | sb_core::ScalarKind::Uint) { wrap_int(kind, x.trunc()) } else { x }).collect();
                c.kind = kind;
                c.cols = cols;
                env.insert(v.name.clone(), c);
            }
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{parse_expr, parse_glsl};

    fn ev(s: &str) -> Option<ConstVal> {
        eval(&parse_expr(s).unwrap(), &ConstEnv::new())
    }

    #[test]
    fn arithmetic_and_constructors() {
        assert_eq!(ev("2 * 3 + 1").unwrap().comps, vec![7.0]);
        assert_eq!(ev("vec3(1.0, 2.0, 3.0).zy").unwrap().comps, vec![3.0, 2.0]);
        assert_eq!(ev("vec4(0.5)").unwrap().comps, vec![0.5; 4]);
        assert_eq!(ev("mat2(2.0)").unwrap().comps, vec![2.0, 0.0, 0.0, 2.0]);
        assert_eq!(ev("ivec2(1.7, -1.7)").unwrap().comps, vec![1.0, -1.0]);
        assert_eq!(ev("7 / 2").unwrap().comps, vec![3.0]);
        assert_eq!(ev("1 << 4").unwrap().comps, vec![16.0]);
        assert_eq!(ev("true ? 3 : 4").unwrap().comps, vec![3.0]);
        assert!(ev("foo(1)").is_none());
        assert!(ev("1 / 0").is_none());
        assert!(ev("x + 1").is_none());
    }

    #[test]
    fn globals_and_array_sizes() {
        let u = parse_glsl("const int N = 4;\nconst int M = N * 2;\nconst float F = sqrt(2.0);\nfloat a[M];\n", 330).unwrap();
        let env = global_consts(&u);
        assert_eq!(env["M"].comps, vec![8.0]);
        assert!(!env.contains_key("F"));
        assert_eq!(array_len(&parse_expr("M - 3").unwrap(), &env), Some(5));
        assert_eq!(array_len(&parse_expr("N - 4").unwrap(), &env), None);
    }
}
