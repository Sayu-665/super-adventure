//! Rectangle samplers (`sampler2DRect`, `isampler2DRect`, `usampler2DRect`,
//! `sampler2DRectShadow`) do not exist in Vulkan (`SampledRect` is a GL-only SPIR-V
//! capability). They are declared as the 2D equivalents and their lookups converted
//! from texel to normalized coordinates:
//!
//! * coordinates `P` -> `sb_rect(P, textureSize(s, 0))` (x and y divided by the size;
//!   the projective and reference components are kept), gradients divided by the size;
//! * `texelFetch(s, P)` / `texelFetchOffset(s, P, o)` gain the level-of-detail argument;
//! * `textureSize(s)` -> `textureSize(s, 0)`.
//!
//! Offsets stay in texels, which is what the 2D `*Offset` functions take.

use std::collections::HashSet;

use crate::ast::*;
use crate::program::{Section, StageWork};

/// The Vulkan type replacing a rectangle sampler type, if `ty` is one.
pub(crate) fn lowered_type(ty: &str) -> Option<&'static str> {
    Some(match ty {
        "sampler2DRect" => "sampler2D",
        "isampler2DRect" => "isampler2D",
        "usampler2DRect" => "usampler2D",
        "sampler2DRectShadow" => "sampler2DShadow",
        _ => return None,
    })
}

const HELPER: &str = "vec2 sb_rect(vec2 p, ivec2 s) { return p / vec2(s); }\n\
vec3 sb_rect(vec3 p, ivec2 s) { return vec3(p.xy / vec2(s), p.z); }\n\
vec4 sb_rect(vec4 p, ivec2 s) { return vec4(p.xy / vec2(s), p.zw); }";

/// Rewrite the lookups through rectangle samplers (see the module docs).
pub(crate) fn apply(w: &mut StageWork) {
    let globals = w.rect_samplers.clone();
    let mut used = false;
    for item in &mut w.unit.items {
        match &mut item.kind {
            ItemKind::Function(f) => {
                let params = retype_params(&mut f.proto);
                if globals.is_empty() && params.is_empty() {
                    continue;
                }
                crate::scope::walk_function_exprs(f, &mut |root, is_local| {
                    let is_rect = |n: &str| if is_local(n) { params.contains(n) } else { globals.contains(n) };
                    rewrite(root, &is_rect, &mut used);
                });
            }
            ItemKind::Prototype(p) => {
                retype_params(p);
            }
            _ => {}
        }
    }
    if used {
        w.piece(Section::Late, &["sb_rect"], HELPER);
    }
}

fn retype_params(p: &mut Prototype) -> HashSet<String> {
    let mut names = HashSet::new();
    for param in &mut p.params {
        if let Some(lowered) = param.ty.name().and_then(lowered_type) {
            param.ty.base = TypeBase::Named(lowered.into());
            if let Some(n) = &param.name {
                names.insert(n.clone());
            }
        }
    }
    names
}

fn rewrite(e: &mut Expr, is_rect: &dyn Fn(&str) -> bool, used: &mut bool) {
    e.walk_children_mut(&mut |c| {
        rewrite(c, is_rect, used);
        Walk::Skip
    });
    let Expr::Call(Callee::Name(name), args) = e else { return };
    let Some(sampler) = args.first().filter(|a| crate::compat::root_ident(a).is_some_and(is_rect)).cloned() else { return };
    let size = || Expr::call("textureSize", vec![sampler.clone(), Expr::Int(0)]);
    match (name.as_str(), args.len()) {
        ("textureSize", 1) | ("texelFetch", 2) => args.push(Expr::Int(0)),
        ("texelFetchOffset", 3) => args.insert(2, Expr::Int(0)),
        (
            "texture" | "textureProj" | "textureOffset" | "textureProjOffset" | "textureGather" | "textureGatherOffset"
            | "textureGatherOffsets" | "textureGrad" | "textureGradOffset" | "textureProjGrad" | "textureProjGradOffset",
            n,
        ) if n >= 2 => {
            let p = std::mem::replace(&mut args[1], Expr::Int(0));
            args[1] = Expr::call("sb_rect", vec![p, size()]);
            *used = true;
            if name.contains("Grad") && args.len() >= 4 {
                for i in [2, 3] {
                    let d = std::mem::replace(&mut args[i], Expr::Int(0));
                    args[i] = Expr::Binary(BinaryOp::Div, Box::new(d), Box::new(Expr::call("vec2", vec![size()])));
                }
            }
        }
        _ => {}
    }
}
