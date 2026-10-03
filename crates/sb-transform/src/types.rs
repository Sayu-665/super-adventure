//! Small, conservative expression type inference over the pack AST (scalars, vectors
//! and matrices; anything else is unknown), with scoped variable types.
//!
//! It powers the lenient-driver conversion fixes in `fixes.rs`: NVIDIA's compiler
//! accepts implicit narrowing conversions (`uint x = float(...)`, `int i = someUint`)
//! with a warning, glslang rejects them. An inference that is unsure returns `None`,
//! and the fixes then leave the code alone.

use std::collections::HashMap;

use sb_core::{GlslType, ScalarKind};

use crate::ast::*;

/// The type of a variable: a value type (with `array` for one-dimensional arrays), or
/// unknown (structs, multi-dimensional arrays, opaque types).
pub(crate) type VarType = Option<GlslType>;

/// Variable types of a declaration's declarator.
pub(crate) fn declared_type(ty: &TypeSpec, var_dims: &[ArrayDim], env: &crate::consteval::ConstEnv) -> VarType {
    let base = GlslType::parse(ty.name()?)?;
    let mut dims = crate::analyze::dims(&ty.array, env);
    dims.extend(crate::analyze::dims(var_dims, env));
    match dims.as_slice() {
        [] => Some(base),
        [Some(n)] => Some(base.with_array(*n)),
        [None] => Some(base.with_array(0)),
        _ => None,
    }
}

/// Scoped variable types for one function body, plus the unit-wide tables.
pub(crate) struct TypeScope<'a> {
    globals: &'a HashMap<String, VarType>,
    functions: &'a HashMap<String, VarType>,
    opaque: &'a HashMap<String, String>,
    scopes: Vec<HashMap<String, VarType>>,
}

/// Unit-wide type tables: global variables (pack globals, uniform block members, the
/// layout members referenced by name) and user function return types (when every
/// overload agrees).
pub(crate) struct UnitTypes {
    pub globals: HashMap<String, VarType>,
    pub functions: HashMap<String, VarType>,
}

impl UnitTypes {
    /// Collect the tables of `unit`. `extra` adds globals not declared in the pack code
    /// (the `sb_Frame`/`sb_Draw` members its loose uniforms resolve to).
    pub(crate) fn collect(unit: &TranslationUnit, extra: impl IntoIterator<Item = (String, GlslType)>) -> Self {
        let env = crate::consteval::global_consts(unit);
        let mut globals: HashMap<String, VarType> = extra.into_iter().map(|(n, t)| (n, Some(t))).collect();
        let mut functions: HashMap<String, VarType> = HashMap::new();
        for item in &unit.items {
            match &item.kind {
                ItemKind::Decl(d) => {
                    for v in &d.vars {
                        globals.insert(v.name.clone(), declared_type(&d.ty.ty, &v.array, &env));
                    }
                }
                ItemKind::Block(b) => {
                    if b.instance.is_none() {
                        for f in &b.fields {
                            for (n, dims) in &f.names {
                                globals.insert(n.clone(), declared_type(&f.ty, dims, &env));
                            }
                        }
                    } else if let Some((n, _)) = &b.instance {
                        globals.insert(n.clone(), None);
                    }
                }
                ItemKind::Function(_) | ItemKind::Prototype(_) => {
                    let p = match &item.kind {
                        ItemKind::Function(f) => &f.proto,
                        ItemKind::Prototype(p) => p,
                        _ => continue,
                    };
                    let ret = if p.ret.ty.array.is_empty() { p.ret.ty.name().and_then(GlslType::parse) } else { None };
                    match functions.get(&p.name) {
                        Some(prev) if *prev != ret => {
                            functions.insert(p.name.clone(), None);
                        }
                        Some(_) => {}
                        None => {
                            functions.insert(p.name.clone(), ret);
                        }
                    }
                }
                _ => {}
            }
        }
        Self { globals, functions }
    }

    /// A scope for a function body (parameters declared).
    pub(crate) fn scope<'a>(&'a self, params: &[Param], opaque: &'a HashMap<String, String>) -> TypeScope<'a> {
        let env = crate::consteval::ConstEnv::new();
        let mut first = HashMap::new();
        for p in params {
            if let Some(n) = &p.name {
                first.insert(n.clone(), declared_type(&p.ty, &p.array, &env));
            }
        }
        TypeScope { globals: &self.globals, functions: &self.functions, opaque, scopes: vec![first] }
    }
}

const SWIZZLE_SETS: [&str; 3] = ["xyzw", "rgba", "stpq"];

fn is_swizzle(f: &str) -> bool {
    !f.is_empty() && f.len() <= 4 && SWIZZLE_SETS.iter().any(|set| f.chars().all(|c| set.contains(c)))
}

fn kind_rank(k: ScalarKind) -> u8 {
    match k {
        ScalarKind::Bool => 0,
        ScalarKind::Int => 1,
        ScalarKind::Uint => 2,
        ScalarKind::Float => 3,
        ScalarKind::Double => 4,
    }
}

fn with_kind(t: GlslType, k: ScalarKind) -> GlslType {
    GlslType { scalar: k, ..t }
}

/// Built-ins whose result has the type of argument `n` (component-wise functions).
fn generic_arg(name: &str) -> Option<usize> {
    Some(match name {
        "abs" | "sign" | "floor" | "trunc" | "round" | "roundEven" | "ceil" | "fract" | "mod" | "min" | "max" | "clamp"
        | "mix" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh" | "asinh" | "acosh"
        | "atanh" | "pow" | "exp" | "log" | "exp2" | "log2" | "sqrt" | "inversesqrt" | "radians" | "degrees"
        | "normalize" | "faceforward" | "reflect" | "refract" | "dFdx" | "dFdy" | "dFdxFine" | "dFdyFine"
        | "dFdxCoarse" | "dFdyCoarse" | "fwidth" | "fwidthFine" | "fwidthCoarse" | "fma" | "bitfieldExtract"
        | "bitfieldInsert" | "bitfieldReverse" | "transpose" | "inverse" | "matrixCompMult" | "subgroupBroadcastFirst"
        | "subgroupAdd" | "subgroupMin" | "subgroupMax" => 0,
        "step" => 1,
        "smoothstep" => 2,
        _ => return None,
    })
}

impl TypeScope<'_> {
    pub(crate) fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub(crate) fn pop(&mut self) {
        self.scopes.pop();
    }

    pub(crate) fn declare(&mut self, name: &str, ty: VarType) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_string(), ty);
        }
    }

    fn lookup(&self, name: &str) -> VarType {
        for s in self.scopes.iter().rev() {
            if let Some(t) = s.get(name) {
                return *t;
            }
        }
        self.globals.get(name).copied().flatten()
    }

    /// The (non-array) type of `e`, if known.
    pub(crate) fn infer(&self, e: &Expr) -> Option<GlslType> {
        let t = self.infer_any(e)?;
        t.array.is_none().then_some(t)
    }

    /// Like [`TypeScope::infer`], but arrays are returned with their length.
    fn infer_any(&self, e: &Expr) -> Option<GlslType> {
        use ScalarKind as K;
        Some(match e {
            Expr::Int(_) => GlslType::INT,
            Expr::UInt(_) => GlslType::UINT,
            Expr::Bool(_) => GlslType::BOOL,
            Expr::Float(_) => GlslType::FLOAT,
            Expr::Double(_) => GlslType::scalar(K::Double),
            Expr::Ident(n) => self.lookup(n)?,
            Expr::Index(a, _) => {
                let t = self.infer_any(a)?;
                if t.array.is_some() {
                    t.element()
                } else if t.is_matrix() {
                    GlslType { cols: 1, ..t }
                } else if t.rows > 1 {
                    GlslType::scalar(t.scalar)
                } else {
                    return None;
                }
            }
            Expr::Field(a, f) => {
                let t = self.infer(a)?;
                if t.is_matrix() || !is_swizzle(f) {
                    return None;
                }
                GlslType::vector(t.scalar, f.len() as u8)
            }
            Expr::Unary(UnaryOp::Not, _) => GlslType::BOOL,
            Expr::Unary(_, a) | Expr::PostInc(a) | Expr::PostDec(a) => self.infer(a)?,
            Expr::Binary(op, a, b) => match op {
                BinaryOp::Or | BinaryOp::Xor | BinaryOp::And => GlslType::BOOL,
                BinaryOp::Equal | BinaryOp::NonEqual | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Lte | BinaryOp::Gte => {
                    GlslType::BOOL
                }
                BinaryOp::LShift | BinaryOp::RShift => self.infer(a)?,
                _ => {
                    let (ta, tb) = (self.infer(a)?, self.infer(b)?);
                    let kind = if kind_rank(ta.scalar) >= kind_rank(tb.scalar) { ta.scalar } else { tb.scalar };
                    let shape = match (ta.is_matrix(), tb.is_matrix(), *op == BinaryOp::Mult) {
                        // mat * vec -> column count must match; result has the matrix's rows.
                        (true, false, true) if tb.rows > 1 => GlslType::vector(kind, ta.rows),
                        // vec * mat -> result has the matrix's columns.
                        (false, true, true) if ta.rows > 1 => GlslType::vector(kind, tb.cols),
                        (true, true, true) => GlslType { scalar: kind, rows: ta.rows, cols: tb.cols, array: None },
                        _ if ta.rows * ta.cols >= tb.rows * tb.cols => ta,
                        _ => tb,
                    };
                    with_kind(shape, kind)
                }
            },
            Expr::Ternary(_, a, b) => self.infer(a).or_else(|| self.infer(b))?,
            Expr::Assign(l, _, _) => self.infer(l)?,
            Expr::Comma(_, b) => self.infer(b)?,
            Expr::Call(Callee::Name(n), args) => self.call(n, args)?,
            Expr::Call(Callee::Method(_, m), _) if m == "length" => GlslType::INT,
            Expr::Call(..) | Expr::Raw(_) => return None,
        })
    }

    fn call(&self, n: &str, args: &[Expr]) -> Option<GlslType> {
        use ScalarKind as K;
        if let Some(t) = GlslType::parse(n) {
            return Some(t);
        }
        if let Some(i) = generic_arg(n) {
            return self.infer(args.get(i)?);
        }
        let arg0 = || args.first().and_then(|a| self.infer(a));
        Some(match n {
            "length" | "distance" | "dot" | "determinant" => GlslType::FLOAT,
            "cross" => GlslType::VEC3,
            "lessThan" | "lessThanEqual" | "greaterThan" | "greaterThanEqual" | "equal" | "notEqual" | "not" => {
                with_kind(arg0()?, K::Bool)
            }
            "any" | "all" | "isnan" | "isinf" if arg0()?.rows == 1 => GlslType::BOOL,
            "isnan" | "isinf" => with_kind(arg0()?, K::Bool),
            "any" | "all" => GlslType::BOOL,
            "floatBitsToInt" | "bitCount" | "findLSB" | "findMSB" => with_kind(arg0()?, K::Int),
            "floatBitsToUint" => with_kind(arg0()?, K::Uint),
            "intBitsToFloat" | "uintBitsToFloat" => with_kind(arg0()?, K::Float),
            "packUnorm2x16" | "packSnorm2x16" | "packUnorm4x8" | "packSnorm4x8" | "packHalf2x16" => GlslType::UINT,
            "unpackUnorm2x16" | "unpackSnorm2x16" | "unpackHalf2x16" => GlslType::VEC2,
            "unpackUnorm4x8" | "unpackSnorm4x8" => GlslType::VEC4,
            "outerProduct" => {
                let (c, r) = (self.infer(args.get(1)?)?, arg0()?);
                GlslType::matrix(c.rows, r.rows)
            }
            _ if crate::names::is_texture_read(n) || n == "imageLoad" || n == "subpassLoad" => {
                let sampler = args.first().and_then(crate::compat::root_ident)?;
                let ty = self.opaque.get(sampler)?;
                let kind = if ty.starts_with("isampler") || ty.starts_with("iimage") {
                    K::Int
                } else if ty.starts_with("usampler") || ty.starts_with("uimage") {
                    K::Uint
                } else {
                    K::Float
                };
                if ty.ends_with("Shadow") && !n.starts_with("textureGather") {
                    GlslType::FLOAT
                } else {
                    GlslType::vector(kind, 4)
                }
            }
            _ => return self.functions.get(n).copied().flatten(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn infer_in(src: &str, expr: &str) -> Option<String> {
        let unit = crate::parse::parse_glsl(src, 460).unwrap();
        let types = UnitTypes::collect(&unit, [("frameTimeCounter".to_string(), GlslType::FLOAT)]);
        let opaque: HashMap<String, String> = [("tex".to_string(), "usampler2D".to_string())].into();
        let mut scope = types.scope(&[], &opaque);
        scope.declare("local", Some(GlslType::IVEC3));
        let e = crate::parse::parse_expr(expr).unwrap();
        scope.infer(&e).map(|t| t.glsl_name())
    }

    #[test]
    fn infers_common_expressions() {
        let src = "uniform mat4 m;\nfloat arr[4];\nstruct S { int a; };\nS s;\nvec3 f(float x) { return vec3(x); }\nuint g(int x) { return 1u; }\nfloat g(float x) { return x; }\n";
        assert_eq!(infer_in(src, "1").as_deref(), Some("int"));
        assert_eq!(infer_in(src, "1u + 2u").as_deref(), Some("uint"));
        assert_eq!(infer_in(src, "1 + 2.0").as_deref(), Some("float"));
        assert_eq!(infer_in(src, "m * vec4(1.0)").as_deref(), Some("vec4"));
        assert_eq!(infer_in(src, "(m * vec4(1.0)).xy").as_deref(), Some("vec2"));
        assert_eq!(infer_in(src, "m[2]").as_deref(), Some("vec4"));
        assert_eq!(infer_in(src, "arr[1] * 2.0").as_deref(), Some("float"));
        assert_eq!(infer_in(src, "arr"), None);
        assert_eq!(infer_in(src, "s.a"), None);
        assert_eq!(infer_in(src, "f(1.0).z").as_deref(), Some("float"));
        assert_eq!(infer_in(src, "g(1)"), None);
        assert_eq!(infer_in(src, "local.xy").as_deref(), Some("ivec2"));
        assert_eq!(infer_in(src, "float(local.x == 1)").as_deref(), Some("float"));
        assert_eq!(infer_in(src, "mix(vec2(0.0), vec2(1.0), 0.5)").as_deref(), Some("vec2"));
        assert_eq!(infer_in(src, "step(0.5, vec3(1.0))").as_deref(), Some("vec3"));
        assert_eq!(infer_in(src, "texelFetch(tex, ivec2(0), 0).r").as_deref(), Some("uint"));
        assert_eq!(infer_in(src, "frameTimeCounter * 2").as_deref(), Some("float"));
        assert_eq!(infer_in(src, "lessThan(vec2(0.0), vec2(1.0))").as_deref(), Some("bvec2"));
        assert_eq!(infer_in(src, "unknownThing + 1.0"), None);
    }
}
