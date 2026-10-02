//! Fixes for code that lenient (NVIDIA) drivers accept and glslang rejects
//! (Iris `CompatibilityTransformer` plus the glslang-verified rejection list).

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::ast::*;
use crate::program::{Ctx, StageWork};

/// Apply every fix to the pack code of `w`.
pub(crate) fn apply(w: &mut StageWork, _ctx: &Ctx) {
    remove_dead_functions(&mut w.unit);
    const_from_const_params(&mut w.unit);
    let (consts, structs) = demote_global_consts(&mut w.unit);
    missing_returns(&mut w.unit);
    let n = dynamic_offsets(&mut w.unit, &w.opaque_types, &consts, &structs);
    if n > 0 {
        w.info("xf.dynamic-offset", format!("{n} texture lookup(s) with a non-constant texel offset were rewritten without the offset form"), 0);
    }
}

/// `*Offset` lookups whose offset is not a constant expression (NVIDIA accepts `const`
/// parameters) are rewritten to the plain lookup with an adjusted coordinate, for 2D
/// samplers: `textureLodOffset(s, P, l, o)` -> `textureLod(s, P + vec2(o) / vec2(textureSize(s, int(l))), l)`,
/// `texelFetchOffset(s, P, l, o)` -> `texelFetch(s, P + o, l)`, and likewise for
/// `textureOffset`, `textureGradOffset` and `textureGatherOffset` (level-0 size).
fn dynamic_offsets(
    unit: &mut TranslationUnit,
    types: &HashMap<String, String>,
    consts: &HashSet<String>,
    structs: &HashSet<String>,
) -> usize {
    let mut count = 0;
    unit.walk_exprs_mut(&mut |e| {
        let Expr::Call(Callee::Name(name), args) = e else { return Walk::Children };
        let (offset_at, plain) = match name.as_str() {
            "textureOffset" => (2, "texture"),
            "textureLodOffset" => (3, "textureLod"),
            "textureGradOffset" => (4, "textureGrad"),
            "texelFetchOffset" => (3, "texelFetch"),
            "textureGatherOffset" => (2, "textureGather"),
            _ => return Walk::Children,
        };
        let Some(sampler) = args.first().and_then(|a| a.as_ident()).map(str::to_string) else { return Walk::Children };
        let is_2d = types.get(&sampler).is_some_and(|t| matches!(t.as_str(), "sampler2D" | "isampler2D" | "usampler2D"));
        if !is_2d || args.len() <= offset_at || is_const_expr(&args[offset_at], consts, structs) {
            return Walk::Children;
        }
        let offset = args.remove(offset_at);
        let p = std::mem::replace(&mut args[1], Expr::Int(0));
        let lod_level = match plain {
            "textureLod" => Expr::call("int", vec![args[2].clone()]),
            _ => Expr::Int(0),
        };
        args[1] = if plain == "texelFetch" {
            Expr::Binary(BinaryOp::Add, Box::new(p), Box::new(offset))
        } else {
            let size = Expr::call("vec2", vec![Expr::call("textureSize", vec![Expr::ident(sampler), lod_level])]);
            let delta = Expr::Binary(BinaryOp::Div, Box::new(Expr::call("vec2", vec![offset])), Box::new(size));
            Expr::Binary(BinaryOp::Add, Box::new(p), Box::new(delta))
        };
        *name = plain.to_string();
        count += 1;
        Walk::Children
    });
    count
}

fn arity(p: &Prototype) -> usize {
    if p.params.len() == 1 && p.params[0].name.is_none() && p.params[0].ty.name() == Some("void") {
        0
    } else {
        p.params.len()
    }
}

/// Remove functions not reachable from `main` / `sb_user_main` or from global
/// initializers (call graph by name and arity, so overloads are kept conservatively).
pub(crate) fn remove_dead_functions(unit: &mut TranslationUnit) {
    let mut defs: HashMap<(String, usize), Vec<usize>> = HashMap::new();
    let mut roots: Vec<(String, usize)> = Vec::new();
    for (i, item) in unit.items.iter().enumerate() {
        match &item.kind {
            ItemKind::Function(f) => {
                let key = (f.proto.name.clone(), arity(&f.proto));
                if f.proto.name == "main" || f.proto.name == "sb_user_main" {
                    roots.push(key.clone());
                }
                defs.entry(key).or_default().push(i);
            }
            ItemKind::Decl(_) | ItemKind::Block(_) => {
                item.walk_exprs(&mut |e| {
                    if let Expr::Call(Callee::Name(n), args) = e {
                        roots.push((n.clone(), args.len()));
                    }
                    Walk::Children
                });
            }
            _ => {}
        }
    }
    let mut live: HashSet<(String, usize)> = HashSet::new();
    let mut stack = roots;
    while let Some(key) = stack.pop() {
        if !live.insert(key.clone()) {
            continue;
        }
        let Some(idx) = defs.get(&key) else { continue };
        for &i in idx {
            if let ItemKind::Function(f) = &unit.items[i].kind {
                let mut calls = Vec::new();
                for s in &f.body {
                    s.walk_exprs(&mut |e| {
                        if let Expr::Call(Callee::Name(n), args) = e {
                            calls.push((n.clone(), args.len()));
                        }
                        Walk::Children
                    });
                }
                stack.extend(calls.into_iter().filter(|c| !live.contains(c)));
            }
        }
    }
    unit.items.retain(|item| match &item.kind {
        ItemKind::Function(f) => live.contains(&(f.proto.name.clone(), arity(&f.proto))),
        ItemKind::Prototype(p) => live.contains(&(p.name.clone(), arity(p))),
        _ => true,
    });
}

fn refs_any(e: &Expr, names: &HashSet<String>) -> bool {
    let mut hit = false;
    e.walk(&mut |x| {
        if let Expr::Ident(n) = x
            && names.contains(n)
        {
            hit = true;
        }
        Walk::Children
    });
    hit
}

/// Strip `const` from locals initialized from `const` parameters, transitively
/// (glslang rejects them below GLSL 4.20; harmless at 4.60).
fn const_from_const_params(unit: &mut TranslationUnit) {
    for f in unit.functions_mut() {
        let mut names: HashSet<String> = f
            .proto
            .params
            .iter()
            .filter(|p| has_storage(&p.quals, &Storage::Const))
            .filter_map(|p| p.name.clone())
            .collect();
        if names.is_empty() {
            continue;
        }
        loop {
            let before = names.len();
            for s in &mut f.body {
                s.walk_stmts_mut(&mut |s| {
                    if let StmtKind::Decl(d) = &mut s.kind
                        && d.ty.has_storage(&Storage::Const)
                    {
                        let tainted = d.vars.iter().any(|v| match &v.init {
                            Some(Init::Expr(e)) => refs_any(e, &names),
                            _ => false,
                        });
                        if tainted {
                            d.ty.quals.retain(|q| !matches!(q, Qualifier::Storage(Storage::Const)));
                            for v in &d.vars {
                                names.insert(v.name.clone());
                            }
                        }
                    }
                });
            }
            if names.len() == before {
                break;
            }
        }
    }
}

/// Built-in functions glslang constant-folds (MachineIndependent/Constant.cpp; not
/// `transpose`, `inverse`, `determinant` or texture functions).
const FOLDABLE: &[&str] = &[
    "abs", "acos", "acosh", "all", "any", "asin", "asinh", "atan", "atanh", "ceil", "clamp", "cos", "cosh", "cross",
    "degrees", "distance", "dot", "exp", "exp2", "faceforward", "floatBitsToInt", "floatBitsToUint", "floor", "fract",
    "intBitsToFloat", "inversesqrt", "isinf", "isnan", "length", "log", "log2", "max", "min", "mix", "mod", "normalize",
    "outerProduct", "packHalf2x16", "packSnorm2x16", "packUnorm2x16", "pow", "radians", "reflect", "refract", "round",
    "roundEven", "sign", "sin", "sinh", "smoothstep", "sqrt", "step", "tan", "tanh", "trunc", "uintBitsToFloat",
    "unpackHalf2x16", "unpackSnorm2x16", "unpackUnorm2x16", "lessThan", "lessThanEqual", "greaterThan",
    "greaterThanEqual", "equal", "notEqual", "not", "matrixCompMult",
];

fn is_const_expr(e: &Expr, consts: &HashSet<String>, structs: &HashSet<String>) -> bool {
    match e {
        Expr::Int(_) | Expr::UInt(_) | Expr::Bool(_) | Expr::Float(_) | Expr::Double(_) => true,
        Expr::Ident(n) => consts.contains(n) || n == "gl_WorkGroupSize" || n.starts_with("gl_Max") || n.starts_with("gl_Min"),
        Expr::Unary(op, a) => !matches!(op, UnaryOp::Inc | UnaryOp::Dec) && is_const_expr(a, consts, structs),
        Expr::Binary(_, a, b) | Expr::Index(a, b) => is_const_expr(a, consts, structs) && is_const_expr(b, consts, structs),
        Expr::Ternary(a, b, c) => {
            is_const_expr(a, consts, structs) && is_const_expr(b, consts, structs) && is_const_expr(c, consts, structs)
        }
        Expr::Field(a, _) => is_const_expr(a, consts, structs),
        Expr::Call(Callee::Name(n), args) => {
            let ctor = crate::rewrite::is_builtin_type(n) || structs.contains(n);
            (ctor || FOLDABLE.contains(&n.as_str())) && args.iter().all(|a| is_const_expr(a, consts, structs))
        }
        Expr::Call(Callee::ArrayCtor(_), args) => args.iter().all(|a| is_const_expr(a, consts, structs)),
        Expr::Call(Callee::Method(r, m), args) => m == "length" && args.is_empty() && matches!(r.as_ref(), Expr::Ident(_)),
        Expr::Assign(..) | Expr::PostInc(_) | Expr::PostDec(_) | Expr::Comma(..) | Expr::Raw(_) => false,
    }
}

fn init_const(i: &Init, consts: &HashSet<String>, structs: &HashSet<String>) -> bool {
    match i {
        Init::Expr(e) => is_const_expr(e, consts, structs),
        Init::List(l) => l.iter().all(|x| init_const(x, consts, structs)),
    }
}

/// Global `const` declarations whose initializer is not a constant expression lose
/// their `const` (glslang: "global const initializers must be constant").
fn demote_global_consts(unit: &mut TranslationUnit) -> (HashSet<String>, HashSet<String>) {
    let mut consts: HashSet<String> = HashSet::new();
    let mut structs: HashSet<String> = HashSet::new();
    for item in &mut unit.items {
        let ItemKind::Decl(d) = &mut item.kind else { continue };
        if let TypeBase::Struct(s) = &d.ty.ty.base
            && let Some(n) = &s.name
        {
            structs.insert(n.clone());
        }
        if !d.ty.has_storage(&Storage::Const) {
            continue;
        }
        let ok = d.vars.iter().all(|v| v.init.as_ref().is_some_and(|i| init_const(i, &consts, &structs)));
        if ok {
            consts.extend(d.vars.iter().map(|v| v.name.clone()));
        } else {
            d.ty.quals.retain(|q| !matches!(q, Qualifier::Storage(Storage::Const)));
        }
    }
    (consts, structs)
}

fn ends_with_return(body: &[Stmt]) -> bool {
    match body.last().map(|s| &s.kind) {
        Some(StmtKind::Return(_) | StmtKind::Discard) => true,
        Some(StmtKind::Block(b)) => ends_with_return(b),
        Some(StmtKind::If { then, els: Some(els), .. }) => {
            ends_with_return(std::slice::from_ref(then)) && ends_with_return(std::slice::from_ref(els))
        }
        _ => false,
    }
}

/// Non-void functions whose body does not end in a `return` get `return T(0);`.
fn missing_returns(unit: &mut TranslationUnit) {
    let structs: BTreeSet<String> = unit
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::Decl(d) => match &d.ty.ty.base {
                TypeBase::Struct(s) => s.name.clone(),
                _ => None,
            },
            _ => None,
        })
        .collect();
    for f in unit.functions_mut() {
        let Some(ret) = f.proto.ret.ty.name() else { continue };
        if ret == "void" || !f.proto.ret.ty.array.is_empty() || structs.contains(ret) || ends_with_return(&f.body) {
            continue;
        }
        if sb_core::GlslType::parse(ret).is_none() {
            continue;
        }
        let line = f.body.last().map_or(0, |s| s.line);
        f.body.push(Stmt::new(StmtKind::Return(Some(Expr::call(ret, vec![Expr::Int(0)]))), line));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_glsl;

    fn print(u: &TranslationUnit) -> String {
        let mut p = crate::print::Printer::new();
        for i in &u.items {
            p.item(i);
        }
        p.finish().0
    }

    #[test]
    fn dead_functions_are_removed_transitively() {
        let mut u = parse_glsl(
            "float a(float x) { return x; }\nfloat a(vec2 x, float y) { return y; }\nfloat b() { return a(1.0); }\nfloat dead() { vec3 v = vec4(1.0); return b(); }\nfloat g = b();\nvoid main() { a(vec2(0.0), 1.0); }\n",
            120,
        )
        .unwrap();
        remove_dead_functions(&mut u);
        let s = print(&u);
        assert!(!s.contains("dead"), "{s}");
        assert!(s.contains("float b()"), "{s}");
        assert!(s.contains("float a(float x)") && s.contains("float a(vec2 x, float y)"), "{s}");
    }

    #[test]
    fn global_const_demotion() {
        let mut u = parse_glsl(
            "uniform float u;\nconst float a = 2.0 * sqrt(4.0);\nconst float b = a + u;\nconst mat3 m = transpose(mat3(1.0));\nconst float c[2] = float[2](a, 1.0);\nconst float d = b;\n",
            330,
        )
        .unwrap();
        demote_global_consts(&mut u);
        let s = print(&u);
        assert!(s.contains("const float a ="), "{s}");
        assert!(s.contains("\nfloat b ="), "{s}");
        assert!(s.contains("\nmat3 m ="), "{s}");
        assert!(s.contains("const float c[2]"), "{s}");
        assert!(s.contains("\nfloat d ="), "{s}");
    }

    #[test]
    fn const_params_and_returns() {
        let mut u = parse_glsl(
            "float f(const float x) { const float y = x * 2.0; const float z = y; if (x > 0.0) return z; }\nvec3 g(bool c) { if (c) { return vec3(1.0); } else { return vec3(0.0); } }\n",
            330,
        )
        .unwrap();
        const_from_const_params(&mut u);
        missing_returns(&mut u);
        let s = print(&u);
        assert!(s.contains("float y = x * 2.0;") && s.contains("float z = y;"), "{s}");
        assert!(s.contains("return float(0);"), "{s}");
        assert!(!s.contains("return vec3(0);"), "{s}");
    }
}
