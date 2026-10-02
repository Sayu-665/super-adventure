//! Comparison-sampler emulation (`TransformOptions::emulate_shadow_samplers`) for hosts
//! whose samplers cannot compare, such as Mojang's 26.3 `GpuSampler`.
//!
//! Every `sampler2DShadow` (pack uniforms, function parameters and prototypes) is
//! declared as a plain `sampler2D`, and every lookup through one becomes a call of a
//! generated helper that does what comparison hardware does with linear filtering: a
//! 2×2 percentage-closer filter over the bilinear footprint. The helper gathers the four
//! depth texels with `textureGather`, compares each against the reference (clamped to
//! `[0, 1]`, as GL does for fixed-point depth textures) and weights the four results
//! bilinearly with `fract(p.xy * textureSize - 0.5)`.
//!
//! The comparison follows the depth convention (ARCHITECTURE §4): `LEQUAL`
//! (`ref <= depth` is lit) for forward depth, `GEQUAL` for
//! [`DepthMode::ReversedZeroToOne`], whose references `depth.rs` has already flipped.
//!
//! Covered lookups: `texture` (bias dropped), `textureLod`, `textureGrad`, the
//! `*Offset` forms, the `textureProj*` forms, `textureGather[Offset]` with a reference,
//! and the legacy `shadow2D*` functions (renamed to those earlier). Levels of detail and
//! gradients are ignored (level 0 is sampled), as `textureGather` has no LOD form.

use std::collections::{BTreeSet, HashSet};

use sb_core::model::DepthMode;

use crate::ast::*;
use crate::program::{Ctx, Section, StageWork};

/// GLSL type of the emulated comparison samplers.
const SHADOW_TYPE: &str = "sampler2DShadow";

/// A generated helper function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Helper {
    Compare,
    CompareOffset,
    CompareProj,
    CompareProjOffset,
    Gather,
    GatherOffset,
}

impl Helper {
    fn name(self) -> &'static str {
        match self {
            Helper::Compare => "sb_shadowCompare",
            Helper::CompareOffset => "sb_shadowCompareOffset",
            Helper::CompareProj => "sb_shadowCompareProj",
            Helper::CompareProjOffset => "sb_shadowCompareProjOffset",
            Helper::Gather => "sb_shadowGather",
            Helper::GatherOffset => "sb_shadowGatherOffset",
        }
    }

    /// Helpers this one calls.
    fn deps(self) -> &'static [Helper] {
        match self {
            Helper::Compare | Helper::Gather => &[],
            Helper::CompareOffset | Helper::CompareProj => &[Helper::Compare],
            Helper::CompareProjOffset => &[Helper::Compare, Helper::CompareOffset],
            Helper::GatherOffset => &[Helper::Gather],
        }
    }

    /// GLSL definition; `cmp` is `lessThanEqual` or `greaterThanEqual`.
    fn text(self, cmp: &str) -> String {
        match self {
            Helper::Compare => format!(
                "float sb_shadowCompare(sampler2D sb_s, vec3 sb_p) {{\n    \
                 vec4 sb_lit = vec4({cmp}(vec4(clamp(sb_p.z, 0.0, 1.0)), textureGather(sb_s, sb_p.xy)));\n    \
                 vec2 sb_f = fract(sb_p.xy * vec2(textureSize(sb_s, 0)) - 0.5);\n    \
                 return mix(mix(sb_lit.w, sb_lit.z, sb_f.x), mix(sb_lit.x, sb_lit.y, sb_f.x), sb_f.y);\n}}"
            ),
            Helper::CompareOffset => "float sb_shadowCompareOffset(sampler2D sb_s, vec3 sb_p, ivec2 sb_o) {\n    \
                 return sb_shadowCompare(sb_s, vec3(sb_p.xy + vec2(sb_o) / vec2(textureSize(sb_s, 0)), sb_p.z));\n}"
                .to_string(),
            Helper::CompareProj => {
                "float sb_shadowCompareProj(sampler2D sb_s, vec4 sb_q) {\n    return sb_shadowCompare(sb_s, sb_q.xyz / sb_q.w);\n}"
                    .to_string()
            }
            Helper::CompareProjOffset => "float sb_shadowCompareProjOffset(sampler2D sb_s, vec4 sb_q, ivec2 sb_o) {\n    \
                 return sb_shadowCompareOffset(sb_s, sb_q.xyz / sb_q.w, sb_o);\n}"
                .to_string(),
            Helper::Gather => format!(
                "vec4 sb_shadowGather(sampler2D sb_s, vec2 sb_p, float sb_r) {{\n    \
                 return vec4({cmp}(vec4(clamp(sb_r, 0.0, 1.0)), textureGather(sb_s, sb_p)));\n}}"
            ),
            Helper::GatherOffset => "vec4 sb_shadowGatherOffset(sampler2D sb_s, vec2 sb_p, float sb_r, ivec2 sb_o) {\n    \
                 return sb_shadowGather(sb_s, sb_p + vec2(sb_o) / vec2(textureSize(sb_s, 0)), sb_r);\n}"
                .to_string(),
        }
    }
}

/// Declared GLSL type of an emulated comparison sampler (`sampler2DShadow` becomes
/// `sampler2D` when emulating).
pub(crate) fn declared_type<'a>(ty: &'a str, ctx: &Ctx) -> &'a str {
    if ctx.opts.emulate_shadow_samplers && ty == SHADOW_TYPE { "sampler2D" } else { ty }
}

/// Rewrite the comparison lookups of `w` (see the module docs).
pub(crate) fn apply(w: &mut StageWork, ctx: &Ctx) {
    if !ctx.opts.emulate_shadow_samplers {
        return;
    }
    let globals: HashSet<String> = w.opaque_types.iter().filter(|(_, t)| *t == SHADOW_TYPE).map(|(n, _)| n.clone()).collect();
    let mut others: BTreeSet<String> = w
        .opaque_types
        .iter()
        .filter(|(_, t)| t.ends_with("Shadow") && *t != SHADOW_TYPE)
        .map(|(n, t)| format!("`{n}` ({t})"))
        .collect();
    let mut used: BTreeSet<Helper> = BTreeSet::new();
    let mut unsupported: BTreeSet<String> = BTreeSet::new();
    for item in &mut w.unit.items {
        match &mut item.kind {
            ItemKind::Function(f) => {
                let params = retype_params(&mut f.proto, &mut others);
                crate::scope::walk_function_exprs(f, &mut |root, is_local| {
                    let is_shadow = |n: &str| if is_local(n) { params.contains(n) } else { globals.contains(n) };
                    rewrite(root, &is_shadow, &mut used, &mut unsupported);
                });
            }
            ItemKind::Prototype(p) => {
                retype_params(p, &mut others);
            }
            _ => {}
        }
    }
    for f in unsupported {
        w.error(
            "xf.shadow-emulation",
            format!("`{f}` on a comparison sampler cannot be emulated (the host has no comparison samplers)"),
            0,
        );
    }
    for o in others {
        w.warn(
            "xf.shadow-emulation",
            format!("comparison sampler {o} is not emulated; the host must bind a comparison sampler"),
            0,
        );
    }
    if used.is_empty() {
        return;
    }
    // Close over dependencies, then emit callees first.
    let mut all = used.clone();
    for h in &used {
        all.extend(h.deps().iter().copied());
    }
    let cmp = if ctx.opts.depth_mode == DepthMode::ReversedZeroToOne { "greaterThanEqual" } else { "lessThanEqual" };
    for h in all {
        w.piece(Section::Late, &[h.name()], h.text(cmp));
    }
}

/// Retype `sampler2DShadow` parameters to `sampler2D`; returns their names. Parameters
/// of other comparison-sampler types are reported in `others`.
fn retype_params(p: &mut Prototype, others: &mut BTreeSet<String>) -> HashSet<String> {
    let mut names = HashSet::new();
    for param in &mut p.params {
        match param.ty.name() {
            Some(SHADOW_TYPE) => {
                param.ty.base = TypeBase::Named("sampler2D".into());
                if let Some(n) = &param.name {
                    names.insert(n.clone());
                }
            }
            Some(t) if t.starts_with("sampler") && t.ends_with("Shadow") => {
                others.insert(format!("parameter `{}` of `{}` ({t})", param.name.as_deref().unwrap_or("?"), p.name));
            }
            _ => {}
        }
    }
    names
}

/// Rewrite one expression tree (post-order).
fn rewrite(e: &mut Expr, is_shadow: &dyn Fn(&str) -> bool, used: &mut BTreeSet<Helper>, unsupported: &mut BTreeSet<String>) {
    e.walk_children_mut(&mut |c| {
        rewrite(c, is_shadow, used, unsupported);
        Walk::Skip
    });
    let Expr::Call(Callee::Name(name), args) = e else { return };
    if !args.first().and_then(crate::compat::root_ident).is_some_and(is_shadow) {
        return;
    }
    // (helper, argument positions kept)
    let (helper, keep): (Helper, &[usize]) = match (name.as_str(), args.len()) {
        // texture(s, p [, bias]), textureLod(s, p, lod), textureGrad(s, p, dx, dy)
        ("texture", 2 | 3) | ("textureLod", 3) | ("textureGrad", 4) => (Helper::Compare, &[0, 1]),
        // textureOffset(s, p, off [, bias])
        ("textureOffset", 3 | 4) => (Helper::CompareOffset, &[0, 1, 2]),
        ("textureLodOffset", 4) => (Helper::CompareOffset, &[0, 1, 3]),
        ("textureGradOffset", 5) => (Helper::CompareOffset, &[0, 1, 4]),
        ("textureProj", 2 | 3) | ("textureProjLod", 3) | ("textureProjGrad", 4) => (Helper::CompareProj, &[0, 1]),
        ("textureProjOffset", 3 | 4) => (Helper::CompareProjOffset, &[0, 1, 2]),
        ("textureProjLodOffset", 4) => (Helper::CompareProjOffset, &[0, 1, 3]),
        ("textureProjGradOffset", 5) => (Helper::CompareProjOffset, &[0, 1, 4]),
        ("textureGather", 3) => (Helper::Gather, &[0, 1, 2]),
        ("textureGatherOffset", 4) => (Helper::GatherOffset, &[0, 1, 2, 3]),
        ("textureGatherOffsets", _) => {
            unsupported.insert(name.clone());
            return;
        }
        // textureSize, textureQueryLevels, textureQueryLod and user functions (whose
        // parameters are retyped) work on the plain sampler unchanged.
        _ => return,
    };
    let old = std::mem::take(args);
    *args = old.into_iter().enumerate().filter(|(i, _)| keep.contains(i)).map(|(_, a)| a).collect();
    *name = helper.name().to_string();
    used.insert(helper);
}
