//! Fixes for code that lenient (NVIDIA) drivers accept and glslang rejects
//! (Iris `CompatibilityTransformer` plus the glslang-verified rejection list).

use std::collections::{BTreeSet, HashMap, HashSet};

use sb_core::GlslType;

use crate::ast::*;
use crate::program::{Ctx, StageWork};

/// Apply every fix to the pack code of `w`.
pub(crate) fn apply(w: &mut StageWork, ctx: &Ctx) {
    remove_dead_functions(&mut w.unit);
    const_from_const_params(&mut w.unit);
    let (consts, structs) = demote_global_consts(&mut w.unit);
    missing_returns(&mut w.unit);
    let n = dynamic_offsets(&mut w.unit, &w.opaque_types, &consts, &structs);
    if n > 0 {
        w.info("xf.dynamic-offset", format!("{n} texture lookup(s) with a non-constant texel offset were rewritten without the offset form"), 0);
    }
    let n = opaque_ternaries(&mut w.unit, &w.opaque_types);
    if n > 0 {
        w.info("xf.opaque-ternary", format!("{n} call(s) taking `cond ? samplerA : samplerB` were split into `cond ? f(samplerA) : f(samplerB)`"), 0);
    }
    let n = narrowing_conversions(w, ctx);
    if n > 0 {
        w.info("xf.implicit-conversion", format!("{n} implicit narrowing conversion(s) (float to int/uint, uint to int, double to float) were made explicit"), 0);
    }
}

// ------------------------------------------------------------ opaque ternaries

/// `f(c ? s1 : s2, ...)` with opaque `s1`/`s2` -> `(c ? f(s1, ...) : f(s2, ...))`.
/// NVIDIA accepts opaque operands of `?:`; GLSL (and SPIR-V) do not.
fn opaque_ternaries(unit: &mut TranslationUnit, opaque_globals: &HashMap<String, String>) -> usize {
    let mut count = 0;
    for f in unit.functions_mut() {
        let params: HashSet<String> = f
            .proto
            .params
            .iter()
            .filter(|p| p.ty.name().is_some_and(sb_uniforms::is_opaque_type))
            .filter_map(|p| p.name.clone())
            .collect();
        crate::scope::walk_function_exprs(f, &mut |root, is_local| {
            let is_opaque = |n: &str| if is_local(n) { params.contains(n) } else { opaque_globals.contains_key(n) };
            hoist_opaque_ternaries(root, &is_opaque, &mut count);
        });
    }
    count
}

fn is_opaque_ternary(e: &Expr, is_opaque: &dyn Fn(&str) -> bool) -> bool {
    match e {
        Expr::Ternary(_, a, b) => {
            let opaque = |x: &Expr| is_opaque_ternary(x, is_opaque) || crate::compat::root_ident(x).is_some_and(is_opaque);
            opaque(a) && opaque(b)
        }
        _ => false,
    }
}

fn hoist_opaque_ternaries(e: &mut Expr, is_opaque: &dyn Fn(&str) -> bool, count: &mut usize) {
    e.walk_children_mut(&mut |c| {
        hoist_opaque_ternaries(c, is_opaque, count);
        Walk::Skip
    });
    let Expr::Call(_, args) = e else { return };
    let Some(i) = args.iter().position(|a| is_opaque_ternary(a, is_opaque)) else { return };
    let Expr::Ternary(cond, a, b) = std::mem::replace(&mut args[i], Expr::Int(0)) else { return };
    let mut first = e.clone();
    let mut second = e.clone();
    if let (Expr::Call(_, x), Expr::Call(_, y)) = (&mut first, &mut second) {
        x[i] = *a;
        y[i] = *b;
    }
    // Further opaque ternary arguments (or nested ones) are split recursively.
    hoist_opaque_ternaries(&mut first, is_opaque, count);
    hoist_opaque_ternaries(&mut second, is_opaque, count);
    *e = Expr::Ternary(cond, Box::new(first), Box::new(second));
    *count += 1;
}

// ------------------------------------------------------- narrowing conversions

/// Types of built-in variables the conversion fix reads.
pub(crate) const BUILTIN_VARIABLE_TYPES: &[(&str, GlslType)] = &[
    ("gl_FragCoord", GlslType::VEC4),
    ("gl_Position", GlslType::VEC4),
    ("gl_FragDepth", GlslType::FLOAT),
    ("gl_PointCoord", GlslType::VEC2),
    ("gl_FrontFacing", GlslType::BOOL),
    ("gl_VertexIndex", GlslType::INT),
    ("gl_InstanceIndex", GlslType::INT),
    ("gl_PrimitiveID", GlslType::INT),
    ("gl_Layer", GlslType::INT),
    ("gl_InvocationID", GlslType::INT),
    ("gl_SampleID", GlslType::INT),
    ("gl_TessCoord", GlslType::VEC3),
    ("gl_LocalInvocationIndex", GlslType::UINT),
    ("gl_LocalInvocationID", GlslType::UVEC3),
    ("gl_GlobalInvocationID", GlslType::UVEC3),
    ("gl_WorkGroupID", GlslType::UVEC3),
    ("gl_NumWorkGroups", GlslType::UVEC3),
    ("gl_WorkGroupSize", GlslType::UVEC3),
];

/// Whether assigning a `from` value to a `to` lvalue is an implicit conversion that
/// GLSL forbids but NVIDIA performs (float/double -> int/uint, uint -> int,
/// double -> float), for scalars and vectors of the same size.
fn is_narrowing(to: GlslType, from: GlslType) -> bool {
    use sb_core::ScalarKind as K;
    to.rows == from.rows
        && to.cols == 1
        && from.cols == 1
        && matches!((to.scalar, from.scalar), (K::Int | K::Uint, K::Float | K::Double) | (K::Int, K::Uint) | (K::Float, K::Double))
}

/// Wrap `e` in a constructor of `to` when it is a narrowing conversion.
fn convert_narrowing(e: &mut Expr, to: Option<GlslType>, scope: &crate::types::TypeScope, count: &mut usize) {
    let (Some(to), Some(from)) = (to, scope.infer(e)) else { return };
    if to.array.is_none() && is_narrowing(to, from) {
        let inner = std::mem::replace(e, Expr::Int(0));
        *e = Expr::call(to.glsl_name(), vec![inner]);
        *count += 1;
    }
}

/// Make implicit narrowing conversions explicit in initializers, plain assignments and
/// return statements.
fn narrowing_conversions(w: &mut StageWork, ctx: &Ctx) -> usize {
    let mut extra: Vec<(String, GlslType)> = BUILTIN_VARIABLE_TYPES.iter().map(|(n, t)| ((*n).to_string(), *t)).collect();
    for m in ctx.pack.layout.frame.members.iter().chain(&ctx.pack.layout.draw.members) {
        extra.push((m.name.clone(), m.ty));
    }
    // Generated globals the pack code reads (semantics, attribute globals, ...).
    for p in &w.pieces {
        if let Ok(u) = crate::parse::parse_glsl(&p.text, 460) {
            let env = crate::consteval::ConstEnv::new();
            for item in &u.items {
                if let ItemKind::Decl(d) = &item.kind {
                    for v in &d.vars {
                        if let Some(t) = crate::types::declared_type(&d.ty.ty, &v.array, &env) {
                            extra.push((v.name.clone(), t));
                        }
                    }
                }
            }
        }
    }
    let types = crate::types::UnitTypes::collect(&w.unit, extra);
    let env = crate::consteval::global_consts(&w.unit);
    let opaque = w.opaque_types.clone();
    let mut count = 0;
    for item in &mut w.unit.items {
        match &mut item.kind {
            ItemKind::Function(f) => {
                let ret = if f.proto.ret.ty.array.is_empty() { f.proto.ret.ty.name().and_then(GlslType::parse) } else { None };
                let mut scope = types.scope(&f.proto.params, &opaque);
                for s in &mut f.body {
                    narrow_stmt(s, &mut scope, ret, &env, &mut count);
                }
            }
            ItemKind::Decl(d) => {
                let scope = types.scope(&[], &opaque);
                for v in &mut d.vars {
                    if let Some(Init::Expr(e)) = &mut v.init {
                        convert_narrowing(e, crate::types::declared_type(&d.ty.ty, &v.array, &env), &scope, &mut count);
                    }
                }
            }
            _ => {}
        }
    }
    count
}

fn narrow_expr(e: &mut Expr, scope: &crate::types::TypeScope, count: &mut usize) {
    e.walk_children_mut(&mut |c| {
        narrow_expr(c, scope, count);
        Walk::Skip
    });
    if let Expr::Assign(lhs, AssignOp::Equal, rhs) = e {
        let to = scope.infer(lhs);
        convert_narrowing(rhs, to, scope, count);
    }
}

fn narrow_stmt(s: &mut Stmt, scope: &mut crate::types::TypeScope, ret: Option<GlslType>, env: &crate::consteval::ConstEnv, count: &mut usize) {
    match &mut s.kind {
        StmtKind::Decl(d) => {
            for v in &mut d.vars {
                let ty = crate::types::declared_type(&d.ty.ty, &v.array, env);
                if let Some(Init::Expr(e)) = &mut v.init {
                    narrow_expr(e, scope, count);
                    convert_narrowing(e, ty, scope, count);
                }
                scope.declare(&v.name, ty);
            }
        }
        StmtKind::Expr(e) | StmtKind::Case(e) => narrow_expr(e, scope, count),
        StmtKind::Return(Some(e)) => {
            narrow_expr(e, scope, count);
            convert_narrowing(e, ret, scope, count);
        }
        StmtKind::Block(b) => {
            scope.push();
            for x in b {
                narrow_stmt(x, scope, ret, env, count);
            }
            scope.pop();
        }
        StmtKind::If { cond, then, els } => {
            narrow_expr(cond, scope, count);
            scope.push();
            narrow_stmt(then, scope, ret, env, count);
            scope.pop();
            if let Some(e) = els {
                scope.push();
                narrow_stmt(e, scope, ret, env, count);
                scope.pop();
            }
        }
        StmtKind::Switch { expr, body } => {
            narrow_expr(expr, scope, count);
            scope.push();
            for x in body {
                narrow_stmt(x, scope, ret, env, count);
            }
            scope.pop();
        }
        StmtKind::While { cond, body } => {
            scope.push();
            match cond {
                Condition::Expr(e) => narrow_expr(e, scope, count),
                Condition::Decl { ty, name, init } => {
                    if let Init::Expr(e) = init {
                        narrow_expr(e, scope, count);
                    }
                    scope.declare(name, crate::types::declared_type(&ty.ty, &[], env));
                }
            }
            narrow_stmt(body, scope, ret, env, count);
            scope.pop();
        }
        StmtKind::DoWhile { body, cond } => {
            scope.push();
            narrow_stmt(body, scope, ret, env, count);
            scope.pop();
            narrow_expr(cond, scope, count);
        }
        StmtKind::For { init, cond, step, body } => {
            scope.push();
            if let Some(i) = init {
                narrow_stmt(i, scope, ret, env, count);
            }
            match cond {
                Some(Condition::Expr(e)) => narrow_expr(e, scope, count),
                Some(Condition::Decl { ty, name, init }) => {
                    if let Init::Expr(e) = init {
                        narrow_expr(e, scope, count);
                    }
                    scope.declare(name, crate::types::declared_type(&ty.ty, &[], env));
                }
                None => {}
            }
            if let Some(st) = step {
                narrow_expr(st, scope, count);
            }
            narrow_stmt(body, scope, ret, env, count);
            scope.pop();
        }
        StmtKind::Return(None) | StmtKind::Empty | StmtKind::Default | StmtKind::Break | StmtKind::Continue | StmtKind::Discard => {}
    }
}

/// `*Offset` lookups whose offset is not a constant expression (NVIDIA accepts `const`
/// parameters) are rewritten to the plain lookup with an adjusted coordinate, for 2D
/// samplers: `textureLodOffset(s, P, l, o)` -> `textureLod(s, P + vec2(o) / vec2(textureSize(s, int(l))), l)`,
/// `texelFetchOffset(s, P, l, o)` -> `texelFetch(s, P + o, l)`, and likewise for
/// `textureOffset`, `textureGradOffset` and `textureGatherOffset` (level-0 size).
///
/// The rewrite is exact only at the size it divides by (an implicit-LOD lookup that
/// lands on a smaller mip level moves by fewer texels), so it is applied only when the
/// offset really is not constant: local `const` variables with constant initializers
/// (in scope) are constant expressions like global ones.
fn dynamic_offsets(
    unit: &mut TranslationUnit,
    types: &HashMap<String, String>,
    consts: &HashSet<String>,
    structs: &HashSet<String>,
) -> usize {
    let mut count = 0;
    let env = OffsetEnv { types, consts, structs };
    for item in &mut unit.items {
        match &mut item.kind {
            ItemKind::Function(f) => {
                // Parameters: never constant expressions; sampler parameters by their type.
                let mut scope = LocalScopes::default();
                scope.push();
                for p in &f.proto.params {
                    if let Some(n) = &p.name {
                        scope.declare(n, false, p.ty.name().filter(|t| sb_uniforms::is_opaque_type(t)).map(str::to_string));
                    }
                }
                for s in &mut f.body {
                    offsets_stmt(s, &mut scope, &env, &mut count);
                }
            }
            ItemKind::Decl(d) => {
                let scope = LocalScopes::default();
                d.walk_exprs_mut(&mut |e| {
                    offsets_expr(e, &scope, &env, &mut count);
                    Walk::Skip
                });
            }
            _ => {}
        }
    }
    count
}

struct OffsetEnv<'a> {
    types: &'a HashMap<String, String>,
    consts: &'a HashSet<String>,
    structs: &'a HashSet<String>,
}

/// Local names in scope: (is a constant expression, opaque type).
#[derive(Default)]
struct LocalScopes {
    scopes: Vec<HashMap<String, (bool, Option<String>)>>,
}

impl LocalScopes {
    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str, constant: bool, opaque: Option<String>) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_string(), (constant, opaque));
        }
    }

    fn get(&self, name: &str) -> Option<&(bool, Option<String>)> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    /// Whether `name` is a constant (a local `const` with a constant initializer, or a
    /// global constant not hidden by a local).
    fn is_const(&self, name: &str, env: &OffsetEnv) -> bool {
        match self.get(name) {
            Some((c, _)) => *c,
            None => env.consts.contains(name),
        }
    }

    /// The opaque type of sampler `name` (a parameter, or a global not hidden by a local).
    fn opaque_type<'a>(&'a self, name: &str, env: &'a OffsetEnv) -> Option<&'a str> {
        match self.get(name) {
            Some((_, t)) => t.as_deref(),
            None => env.types.get(name).map(String::as_str),
        }
    }
}

fn offsets_stmt(s: &mut Stmt, scope: &mut LocalScopes, env: &OffsetEnv, count: &mut usize) {
    match &mut s.kind {
        StmtKind::Decl(d) => {
            let is_const = d.ty.has_storage(&Storage::Const);
            for v in &mut d.vars {
                if let Some(i) = &mut v.init {
                    i.walk_mut(&mut |e| {
                        offsets_expr(e, scope, env, count);
                        Walk::Skip
                    });
                }
                let constant = is_const
                    && v.init.as_ref().is_some_and(|i| init_const_with(i, &|n: &str| scope.is_const(n, env), env.structs));
                scope.declare(&v.name, constant, None);
            }
        }
        StmtKind::Expr(e) | StmtKind::Case(e) | StmtKind::Return(Some(e)) => offsets_expr(e, scope, env, count),
        StmtKind::Block(b) => {
            scope.push();
            for x in b {
                offsets_stmt(x, scope, env, count);
            }
            scope.pop();
        }
        StmtKind::If { cond, then, els } => {
            offsets_expr(cond, scope, env, count);
            scope.push();
            offsets_stmt(then, scope, env, count);
            scope.pop();
            if let Some(e) = els {
                scope.push();
                offsets_stmt(e, scope, env, count);
                scope.pop();
            }
        }
        StmtKind::Switch { expr, body } => {
            offsets_expr(expr, scope, env, count);
            scope.push();
            for x in body {
                offsets_stmt(x, scope, env, count);
            }
            scope.pop();
        }
        StmtKind::While { cond, body } => {
            scope.push();
            offsets_condition(cond, scope, env, count);
            offsets_stmt(body, scope, env, count);
            scope.pop();
        }
        StmtKind::DoWhile { body, cond } => {
            scope.push();
            offsets_stmt(body, scope, env, count);
            scope.pop();
            offsets_expr(cond, scope, env, count);
        }
        StmtKind::For { init, cond, step, body } => {
            scope.push();
            if let Some(i) = init {
                offsets_stmt(i, scope, env, count);
            }
            if let Some(c) = cond {
                offsets_condition(c, scope, env, count);
            }
            if let Some(st) = step {
                offsets_expr(st, scope, env, count);
            }
            offsets_stmt(body, scope, env, count);
            scope.pop();
        }
        StmtKind::Return(None) | StmtKind::Empty | StmtKind::Default | StmtKind::Break | StmtKind::Continue | StmtKind::Discard => {}
    }
}

fn offsets_condition(c: &mut Condition, scope: &mut LocalScopes, env: &OffsetEnv, count: &mut usize) {
    match c {
        Condition::Expr(e) => offsets_expr(e, scope, env, count),
        Condition::Decl { name, init, .. } => {
            init.walk_mut(&mut |e| {
                offsets_expr(e, scope, env, count);
                Walk::Skip
            });
            scope.declare(name, false, None);
        }
    }
}

fn offsets_expr(e: &mut Expr, scope: &LocalScopes, env: &OffsetEnv, count: &mut usize) {
    e.walk_mut(&mut |e| {
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
        let is_2d = scope.opaque_type(&sampler, env).is_some_and(|t| matches!(t, "sampler2D" | "isampler2D" | "usampler2D"));
        if !is_2d || args.len() <= offset_at || is_const_expr_with(&args[offset_at], &|n: &str| scope.is_const(n, env), env.structs) {
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
        *count += 1;
        Walk::Children
    });
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

/// Whether `e` is a constant expression; `is_const` tells whether a variable name is a
/// constant.
fn is_const_expr_with(e: &Expr, is_const: &dyn Fn(&str) -> bool, structs: &HashSet<String>) -> bool {
    let rec = |x: &Expr| is_const_expr_with(x, is_const, structs);
    match e {
        Expr::Int(_) | Expr::UInt(_) | Expr::Bool(_) | Expr::Float(_) | Expr::Double(_) => true,
        Expr::Ident(n) => is_const(n) || n == "gl_WorkGroupSize" || n.starts_with("gl_Max") || n.starts_with("gl_Min"),
        Expr::Unary(op, a) => !matches!(op, UnaryOp::Inc | UnaryOp::Dec) && rec(a),
        Expr::Binary(_, a, b) | Expr::Index(a, b) => rec(a) && rec(b),
        Expr::Ternary(a, b, c) => rec(a) && rec(b) && rec(c),
        Expr::Field(a, _) => rec(a),
        Expr::Call(Callee::Name(n), args) => {
            let ctor = crate::rewrite::is_builtin_type(n) || structs.contains(n);
            (ctor || FOLDABLE.contains(&n.as_str())) && args.iter().all(rec)
        }
        Expr::Call(Callee::ArrayCtor(_), args) => args.iter().all(rec),
        Expr::Call(Callee::Method(r, m), args) => m == "length" && args.is_empty() && matches!(r.as_ref(), Expr::Ident(_)),
        Expr::Assign(..) | Expr::PostInc(_) | Expr::PostDec(_) | Expr::Comma(..) | Expr::Raw(_) => false,
    }
}

fn init_const(i: &Init, consts: &HashSet<String>, structs: &HashSet<String>) -> bool {
    init_const_with(i, &|n: &str| consts.contains(n), structs)
}

fn init_const_with(i: &Init, is_const: &dyn Fn(&str) -> bool, structs: &HashSet<String>) -> bool {
    match i {
        Init::Expr(e) => is_const_expr_with(e, is_const, structs),
        Init::List(l) => l.iter().all(|x| init_const_with(x, is_const, structs)),
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
