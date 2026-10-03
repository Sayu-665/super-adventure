//! Reversed-Z support (ARCHITECTURE §4, Iris 26.3 `DepthTransformer`): with
//! `DepthMode::ReversedZeroToOne` and `invert_depth_reads`, packs keep seeing
//! GL-style forward depth:
//!
//! * reads of depth textures (`depthtex*`, `dhDepthTex*`, `shadowtex*` sampled without
//!   comparison) become `vec4(1.0 - read.x)` (`vec4(1.0) - read` for gathers);
//! * comparison lookups flip their reference (`1.0 - ref`, including the separate
//!   `refZ` argument of comparison `textureGather*`); the host compares with `GEQUAL`;
//! * user functions that receive a depth sampler are cloned (`sb_depth_<mask>_<name>`)
//!   with their reads rewritten;
//! * fragment `gl_FragCoord.z` reads and `gl_FragDepth` writes are flipped, and with
//!   them the `depth_greater`/`depth_less` conservative-depth layouts.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use sb_core::ShaderStage;
use sb_core::model::{DepthMode, ResourceKind, ResourceRef};

use crate::ast::*;
use crate::program::{Ctx, Section, StageWork};

/// How a sampler name must be treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Depth {
    /// Plain depth read: invert the result.
    Read,
    /// Comparison sampler of the given coordinate shape: flip the reference.
    Compare(CmpShape),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CmpShape {
    /// Reference in `.z` of a vec3 (`sampler1DShadow` uses vec3 too, `sampler2DShadow`).
    Z3,
    /// Reference in `.w` of a vec4 (`samplerCubeShadow`, `sampler2DArrayShadow`).
    W4,
    /// Reference is a separate argument (`samplerCubeArrayShadow`).
    Arg,
}

pub(crate) fn apply(w: &mut StageWork, ctx: &Ctx) {
    if ctx.opts.depth_mode != DepthMode::ReversedZeroToOne || !ctx.opts.invert_depth_reads {
        return;
    }
    let mut samplers: HashMap<String, Depth> = HashMap::new();
    for r in &w.resources {
        let Some(e) = ctx.pack.bindings.get(&r.binding_name) else { continue };
        if !matches!(e.resource, ResourceRef::DepthTex(_) | ResourceRef::DhDepthTex(_) | ResourceRef::ShadowTex(_) | ResourceRef::ShadowTexHw(_)) {
            continue;
        }
        if let ResourceKind::Sampler { dim, shadow, .. } = &e.kind
            && let Some(d) = treatment(dim, *shadow)
        {
            samplers.insert(r.glsl_name.clone(), d);
        }
    }
    if !samplers.is_empty() {
        rewrite_reads(w, &samplers);
    }
    if w.stage == ShaderStage::Fragment {
        fragment(w);
    }
}

/// Treatment of a sampler of dimension `dim` (ResourceKind spelling) bound to a depth
/// resource. Depth textures are 2D: other dimensions are custom textures reusing a
/// depth name and are left alone.
fn treatment(dim: &str, shadow: bool) -> Option<Depth> {
    if shadow {
        return Some(Depth::Compare(match dim {
            "cube" | "2d_array" => CmpShape::W4,
            "cube_array" => CmpShape::Arg,
            _ => CmpShape::Z3,
        }));
    }
    matches!(dim, "2d" | "2d_rect" | "2d_array").then_some(Depth::Read)
}

/// Treatment of a sampler parameter by its declared GLSL type.
fn param_treatment(ty: &TypeSpec) -> Option<Depth> {
    let kind = sb_uniforms::sampler_kind(ty.name()?)?;
    match kind {
        ResourceKind::Sampler { dim, shadow, .. } => treatment(&dim, shadow),
        _ => None,
    }
}

fn fragment(w: &mut StageWork) {
    let mut unsupported = false;
    w.unit.walk_exprs_mut(&mut |e| {
        flip_fragment_depth(e, &mut unsupported);
        Walk::Skip
    });
    if unsupported {
        w.warn("xf.depth-write", "`gl_FragDepth++`/`--` is not converted for reversed depth", 0);
    }
    flip_conservative_depth(&mut w.unit);
}

/// The stored depth is `1 - x` of the depth the pack writes, so a conservative-depth
/// promise about the pack's value is the opposite promise about the stored one:
/// `layout(depth_greater) out float gl_FragDepth;` becomes `depth_less` and vice versa
/// (left unchanged, the device could resolve the depth test early on a promise the
/// written depth breaks). `depth_any` and `depth_unchanged` hold either way.
fn flip_conservative_depth(unit: &mut TranslationUnit) {
    for item in &mut unit.items {
        let ItemKind::Decl(d) = &mut item.kind else { continue };
        if !d.vars.iter().any(|v| v.name == "gl_FragDepth") {
            continue;
        }
        for q in &mut d.ty.quals {
            let Qualifier::Layout(ids) = q else { continue };
            for id in ids.iter_mut() {
                if id.name.eq_ignore_ascii_case("depth_greater") {
                    id.name = "depth_less".into();
                } else if id.name.eq_ignore_ascii_case("depth_less") {
                    id.name = "depth_greater".into();
                }
            }
        }
    }
}

/// `1.0 - e`
fn one_minus(e: Expr) -> Expr {
    Expr::Binary(BinaryOp::Sub, Box::new(Expr::Float(1.0)), Box::new(e))
}

/// The pack sees forward depth; the attachment stores reversed depth:
/// * `gl_FragCoord` -> `vec4(gl_FragCoord.xy, 1.0 - gl_FragCoord.z, gl_FragCoord.w)`;
/// * `gl_FragDepth = v` -> `gl_FragDepth = 1.0 - (v)`;
/// * `gl_FragDepth op= v` -> `gl_FragDepth = 1.0 - ((1.0 - gl_FragDepth) op (v))`;
/// * other reads of `gl_FragDepth` (after a write) -> `(1.0 - gl_FragDepth)`.
fn flip_fragment_depth(e: &mut Expr, unsupported: &mut bool) {
    match e {
        Expr::Ident(n) if n == "gl_FragCoord" => {
            *e = Expr::raw("vec4(gl_FragCoord.xy, 1.0 - gl_FragCoord.z, gl_FragCoord.w)");
        }
        Expr::Ident(n) if n == "gl_FragDepth" => *e = one_minus(Expr::ident("gl_FragDepth")),
        Expr::Assign(lhs, op, rhs) if lhs.as_ident() == Some("gl_FragDepth") => {
            flip_fragment_depth(rhs, unsupported);
            let v = std::mem::replace(rhs.as_mut(), Expr::Int(0));
            **rhs = match op.binary() {
                None => one_minus(v),
                Some(b) => one_minus(Expr::Binary(b, Box::new(one_minus(Expr::ident("gl_FragDepth"))), Box::new(v))),
            };
            *op = AssignOp::Equal;
        }
        Expr::PostInc(a) | Expr::PostDec(a) | Expr::Unary(UnaryOp::Inc | UnaryOp::Dec, a) if a.as_ident() == Some("gl_FragDepth") => {
            *unsupported = true;
        }
        _ => e.walk_children_mut(&mut |c| {
            flip_fragment_depth(c, unsupported);
            Walk::Skip
        }),
    }
}

fn clone_name(name: &str, mask: &[usize]) -> String {
    let m: Vec<String> = mask.iter().map(usize::to_string).collect();
    format!("sb_depth_{}_{name}", m.join("_"))
}

/// (function name, arity, depth-sampler argument positions).
type CloneKey = (String, usize, Vec<usize>);

/// Rewrite one expression tree (post-order). `samplers` maps visible names to their
/// depth treatment; calls of user functions with depth sampler arguments are renamed
/// and recorded in `clones`.
fn rewrite_expr(
    e: &mut Expr,
    samplers: &dyn Fn(&str) -> Option<Depth>,
    user: &HashMap<(String, usize), Vec<usize>>,
    clones: &mut Vec<CloneKey>,
    helpers: &mut BTreeSet<&'static str>,
) {
    e.walk_children_mut(&mut |child| {
        rewrite_expr(child, samplers, user, clones, helpers);
        Walk::Skip
    });
    let Expr::Call(Callee::Name(name), args) = e else { return };
    let first = args.first().and_then(crate::compat::root_ident).and_then(samplers);
    if crate::names::is_texture_read(name) {
        match first {
            Some(Depth::Read) => {
                let gather = name.starts_with("textureGather");
                let call = std::mem::replace(e, Expr::Int(0));
                *e = if gather {
                    Expr::Binary(BinaryOp::Sub, Box::new(Expr::call("vec4", vec![Expr::Float(1.0)])), Box::new(call))
                } else {
                    Expr::call(
                        "vec4",
                        vec![Expr::Binary(BinaryOp::Sub, Box::new(Expr::Float(1.0)), Box::new(Expr::Field(Box::new(call), "x".into())))],
                    )
                };
            }
            Some(Depth::Compare(shape)) => {
                let proj = name.starts_with("textureProj");
                match shape {
                    // textureGather[Offset[s]](s, P, refZ[, offset]): the reference is a
                    // separate argument for every comparison sampler type.
                    _ if name.starts_with("textureGather") => {
                        if args.len() >= 3 {
                            let r = std::mem::replace(&mut args[2], Expr::Int(0));
                            args[2] = one_minus(r);
                        }
                    }
                    CmpShape::Arg => {
                        if args.len() >= 3 {
                            let r = std::mem::replace(&mut args[2], Expr::Int(0));
                            args[2] = Expr::Binary(BinaryOp::Sub, Box::new(Expr::Float(1.0)), Box::new(r));
                        }
                    }
                    _ if args.len() >= 2 => {
                        let helper = match (shape, proj) {
                            (_, true) => "sb_revRefProj",
                            (CmpShape::W4, false) => "sb_revRef4",
                            _ => "sb_revRef3",
                        };
                        helpers.insert(helper);
                        let p = std::mem::replace(&mut args[1], Expr::Int(0));
                        args[1] = Expr::call(helper, vec![p]);
                    }
                    _ => {}
                }
            }
            None => {}
        }
        return;
    }
    // User function receiving depth samplers.
    let key = (name.clone(), args.len());
    if !user.contains_key(&key) {
        return;
    }
    let mask: Vec<usize> =
        args.iter().enumerate().filter(|(_, a)| crate::compat::root_ident(a).and_then(samplers).is_some()).map(|(i, _)| i).collect();
    if mask.is_empty() {
        return;
    }
    let new = clone_name(name, &mask);
    clones.push((name.clone(), args.len(), mask));
    *name = new;
}

fn rewrite_reads(w: &mut StageWork, globals: &HashMap<String, Depth>) {
    // User function definitions by (name, arity).
    let mut user: HashMap<(String, usize), Vec<usize>> = HashMap::new();
    for (i, item) in w.unit.items.iter().enumerate() {
        if let ItemKind::Function(f) = &item.kind {
            user.entry((f.proto.name.clone(), f.proto.params.len())).or_default().push(i);
        }
    }
    let mut clones: Vec<CloneKey> = Vec::new();
    let mut helpers: BTreeSet<&'static str> = BTreeSet::new();
    let global_lookup = |n: &str| globals.get(n).copied();
    for item in &mut w.unit.items {
        match &mut item.kind {
            ItemKind::Function(f) => {
                for s in &mut f.body {
                    s.walk_exprs_mut(&mut |e| {
                        rewrite_expr(e, &global_lookup, &user, &mut clones, &mut helpers);
                        Walk::Skip
                    });
                }
            }
            ItemKind::Decl(d) => d.walk_exprs_mut(&mut |e| {
                rewrite_expr(e, &global_lookup, &user, &mut clones, &mut helpers);
                Walk::Skip
            }),
            _ => {}
        }
    }
    // Generate clones (worklist; clones may need further clones).
    let mut done: BTreeSet<String> = BTreeSet::new();
    let mut inserts: BTreeMap<usize, Vec<Item>> = BTreeMap::new();
    let mut guard = 0;
    while let Some((name, arity, mask)) = clones.pop() {
        guard += 1;
        if guard > 256 {
            w.warn("xf.depth-clones", "too many depth-sampler function clones; stopped", 0);
            break;
        }
        let cname = clone_name(&name, &mask);
        if !done.insert(format!("{cname}/{arity}")) {
            continue;
        }
        let Some(idx) = user.get(&(name.clone(), arity)) else { continue };
        for &i in idx {
            let ItemKind::Function(f) = &w.unit.items[i].kind else { continue };
            let mut f = f.clone();
            f.proto.name = cname.clone();
            // Each overload treats its parameters by their declared sampler types.
            let params: HashMap<String, Depth> = mask
                .iter()
                .filter_map(|pi| {
                    let p = f.proto.params.get(*pi)?;
                    Some((p.name.clone()?, param_treatment(&p.ty)?))
                })
                .collect();
            let lookup = |n: &str| params.get(n).copied().or_else(|| globals.get(n).copied());
            for s in &mut f.body {
                s.walk_exprs_mut(&mut |e| {
                    rewrite_expr(e, &lookup, &user, &mut clones, &mut helpers);
                    Walk::Skip
                });
            }
            inserts.entry(i).or_default().push(Item { kind: ItemKind::Function(f), line: w.unit.items[i].line });
        }
    }
    // Insert clones after their originals, with prototypes at the first use site's
    // safety point: right after the original definition (definitions precede uses in
    // practice; a prototype is added before the first function item for safety).
    let mut protos: Vec<Item> = Vec::new();
    for items in inserts.values() {
        for it in items {
            if let ItemKind::Function(f) = &it.kind {
                protos.push(Item { kind: ItemKind::Prototype(f.proto.clone()), line: 0 });
            }
        }
    }
    for (i, items) in inserts.into_iter().rev() {
        let at = i + 1;
        for (k, it) in items.into_iter().enumerate() {
            w.unit.items.insert(at + k, it);
        }
    }
    if !protos.is_empty()
        && let Some(first_fn) = w.unit.items.iter().position(|i| matches!(i.kind, ItemKind::Function(_)))
    {
        // Prototypes may reference struct types; place them right before the first function.
        for (k, p) in protos.into_iter().enumerate() {
            w.unit.items.insert(first_fn + k, p);
        }
    }
    for h in helpers {
        let text = match h {
            "sb_revRef3" => "vec3 sb_revRef3(vec3 p) { return vec3(p.xy, 1.0 - p.z); }",
            "sb_revRef4" => "vec4 sb_revRef4(vec4 p) { return vec4(p.xyz, 1.0 - p.w); }",
            _ => "vec4 sb_revRefProj(vec4 p) { return vec4(p.xy, p.w - p.z, p.w); }",
        };
        w.piece(Section::Late, &[h], text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_names_encode_masks() {
        assert_eq!(clone_name("f", &[0, 2]), "sb_depth_0_2_f");
        assert_eq!(treatment("3d", false), None);
        assert_eq!(treatment("2d", false), Some(Depth::Read));
        assert_eq!(treatment("cube", true), Some(Depth::Compare(CmpShape::W4)));
    }
}
