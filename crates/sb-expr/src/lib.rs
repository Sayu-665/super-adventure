//! The expression language of OptiFine/Iris shader packs, evaluated on the CPU.
//!
//! It covers:
//!
//! * custom uniforms and variables from `shaders.properties`
//!   (`uniform.<type>.<name>=<expr>`, `variable.<type>.<name>=<expr>`), compiled
//!   once into a [`CustomUniforms`] set and evaluated every frame, either into a
//!   list of values ([`CustomUniforms::evaluate`]) or directly into the std140
//!   `sb_Frame` block ([`CustomUniforms::evaluate_into_block`]);
//! * one-shot boolean conditions such as `program.<name>.enabled=<expr>`
//!   ([`eval_bool`]);
//! * std140 helpers ([`read_value`], [`write_value`]) shared with `sb-runtime` and
//!   the JNI layer.
//!
//! # Language
//!
//! * Literals: integers (`42`, octal `010`, hex `0x1F`, binary `0b101`), floats
//!   (`1.5`, `.5`, `2.`, `1e-3`, `1.0f`), `true`, `false`.
//! * Identifiers: builtin uniforms, custom uniforms/variables (any definition
//!   order), caller-supplied constants (`BIOME_*`, `CAT_*`, `PPT_*`, see
//!   [`standard_constants`]) and `pi`.
//! * Operators, loosest to tightest: `||`, `&&`, `== != ≠`, `< > <= >= ≤ ≥`,
//!   `+ -`, `* / %`, unary `- ! +`, member access. All binary operators are
//!   left-associative (OptiFine/C precedence).
//! * Member access: `.x .y .z .w`, `.r .g .b .a`, `.s .t .p .q` and `.0`–`.3` on
//!   vectors, including multi-component swizzles such as `.xy`. On matrices, `.i`
//!   selects column `i` and `.i.j` reads GLSL `m[i][j]` (element `i*4+j` of the
//!   column-major array), exactly as Iris and OptiFine do.
//! * Functions: `sin cos tan asin acos atan(y[,x]) atan2 torad radians todeg
//!   degrees exp exp2 exp10 pow log(x) log(base,x) log2 log10 sqrt inversesqrt abs
//!   sign signum floor ceil frac round min max (variadic) clamp mix lerp(k,x,y)
//!   edge fmod random() random(min,max) randomInt() randomInt(n)
//!   randomInt(min,max) if(c,v,[c,v,]...,else) ifb smooth([id,]v[,up[,down]])
//!   between equals(a,b[,eps]) in(x,v...) vec2 vec3 vec4 ivecN bvecN
//!   print(id,n,x)`. Math functions apply component-wise to vectors, with scalar
//!   arguments broadcast.
//!
//! # Types
//!
//! Values are `float`, `int`, `bool`, `vec2`, `vec3`, `vec4` and (inputs only)
//! `mat4`; see [`Value`]. `int` promotes to `float` implicitly; `+ - * %` of two
//! ints stay `int`, `/` always divides as floats (as in Iris). The result of a
//! definition is converted to its declared type (`int` truncates, `bool` is
//! "non-zero").
//!
//! # `smooth()`
//!
//! Exponential smoothing as in Iris' `SmoothFloat`: the half-life is a tenth of the
//! fade time, so the value is within 0.1% of the target after `fade` seconds;
//! `fadeUp` applies when the target is above the current value, `fadeDown`
//! otherwise (both default to 1 second, a fade of 0 disables smoothing). The first
//! evaluation snaps to the target, and the frame delta time drives the decay. Every
//! call site keeps its own state; an explicit id (a number literal as first argument,
//! as in OptiFine) is accepted but, as in Iris, does not merge call sites: real
//! packs (e.g. Complementary) reuse ids for unrelated values.
//!
//! # Robustness
//!
//! Malformed input never panics: it yields an [`ExprError`] (byte offset, message,
//! [`ExprErrorKind`]) or, for [`CustomUniforms::compile`], an error diagnostic that
//! drops the definition. Expressions nested deeper than [`MAX_DEPTH`] are rejected so
//! that compilation and evaluation stay within a small, bounded stack. Accepted
//! leniencies beyond Iris: trailing `;`, numbers used as conditions, booleans compared
//! with numbers (as 0/1), member access on any vector expression, swizzles, GLSL-style
//! vector constructors (`vec4(v.xyz, 1)`, `vec3(1)`, `vec3(v4)`, `vec3(v2, v2)`) and
//! true variadic `min`/`max`. A custom uniform named like a builtin shadows it (with a
//! warning; Iris drops such a definition instead).

#![warn(missing_docs)]

mod boolexpr;
mod compile;
mod custom;
mod node;
mod parse;
mod rng;
pub mod std140;
#[cfg(test)]
mod tests;
mod value;

pub use boolexpr::{eval_bool, eval_bool_with};
pub use custom::{BlockInputs, CustomUniforms, UniformInputs};
pub use parse::{
    BinaryOp, Expr, ExprError, ExprErrorKind, ExprKind, MAX_DEPTH, Span, UnaryOp, parse,
};
pub use rng::DEFAULT_SEED;
pub use std140::{Std140Error, read_value, write_value};
pub use value::{Value, ValueType};

use indexmap::IndexMap;

/// Biome categories in OptiFine/Iris order (`CAT_<NAME>` = index).
const BIOME_CATEGORIES: [&str; 19] = [
    "NONE",
    "TAIGA",
    "EXTREME_HILLS",
    "JUNGLE",
    "MESA",
    "PLAINS",
    "SAVANNA",
    "ICY",
    "THE_END",
    "BEACH",
    "FOREST",
    "OCEAN",
    "DESERT",
    "RIVER",
    "SWAMP",
    "MUSHROOM",
    "NETHER",
    "MOUNTAIN",
    "UNDERGROUND",
];

/// The fixed constants OptiFine and Iris define for expressions: `CAT_*` (biome
/// categories, `CAT_NONE = 0` … `CAT_UNDERGROUND = 18`) and `PPT_NONE/RAIN/SNOW`
/// (`0/1/2`), all as `Int`. `BIOME_*` ids depend on the game's biome registry and
/// must be added by the host.
pub fn standard_constants() -> IndexMap<String, Value> {
    let mut m: IndexMap<String, Value> = BIOME_CATEGORIES
        .iter()
        .enumerate()
        .map(|(i, name)| (format!("CAT_{name}"), Value::Int(i as i32)))
        .collect();
    for (i, name) in ["PPT_NONE", "PPT_RAIN", "PPT_SNOW"].into_iter().enumerate() {
        m.insert(name.to_string(), Value::Int(i as i32));
    }
    m
}
