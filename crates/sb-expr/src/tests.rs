//! Language-level tests through the public API.

use crate::*;
use indexmap::IndexMap;
use pretty_assertions::assert_eq;
use sb_core::model::{BlockLayout, BlockMember, CustomUniform, UniformSource};
use sb_core::{Diagnostics, GlslType, Severity};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::f32::consts::{E, FRAC_PI_2, FRAC_PI_4, PI};

fn input_type(name: &str) -> Option<GlslType> {
    Some(match name {
        "f" | "frameTimeCounter" | "rainStrength" | "x" | "y" => GlslType::FLOAT,
        "i" | "frameCounter" | "worldTime" => GlslType::INT,
        "b" | "is_hurt" => GlslType::BOOL,
        "v2" => GlslType::VEC2,
        "eyeBrightness" => GlslType::IVEC2,
        "v3" | "sunPosition" => GlslType::VEC3,
        "v4" => GlslType::VEC4,
        "m" | "gbufferModelView" => GlslType::MAT4,
        "m3" => GlslType::MAT3,
        "arr" => GlslType::FLOAT.with_array(4),
        _ => return None,
    })
}

fn matrix() -> [f32; 16] {
    std::array::from_fn(|i| i as f32)
}

fn input_value(name: &str) -> Option<Value> {
    Some(match name {
        "f" => Value::Float(2.5),
        "i" => Value::Int(7),
        "b" => Value::Bool(true),
        "v2" => Value::Vec2([1.0, 2.0]),
        "eyeBrightness" => Value::Vec2([120.0, 240.0]),
        "v3" => Value::Vec3([1.0, 2.0, 3.0]),
        "v4" => Value::Vec4([1.0, 2.0, 3.0, 4.0]),
        "m" => Value::Mat4(matrix()),
        "m3" => Value::Mat4(matrix()),
        _ => return None,
    })
}

fn constants() -> IndexMap<String, Value> {
    let mut c = standard_constants();
    c.insert("BIOME_PLAINS".into(), Value::Int(1));
    c.insert("BIOME_DESERT".into(), Value::Int(14));
    c
}

fn def(name: &str, ty: GlslType, expr: &str) -> CustomUniform {
    CustomUniform {
        name: name.into(),
        ty,
        expression: expr.into(),
        is_variable: false,
        location: None,
    }
}

fn var(name: &str, ty: GlslType, expr: &str) -> CustomUniform {
    CustomUniform {
        name: name.into(),
        ty,
        expression: expr.into(),
        is_variable: true,
        location: None,
    }
}

fn compile(defs: &[CustomUniform]) -> (CustomUniforms, Diagnostics) {
    CustomUniforms::compile(defs, &input_type, &constants())
}

/// Evaluate `expr` as a uniform of type `ty`; `Err` holds the first error code.
fn try_eval(expr: &str, ty: GlslType) -> Result<Value, String> {
    let (mut cu, d) = compile(&[def("out", ty, expr)]);
    if let Some(e) = d.errors().next() {
        return Err(e.code.clone());
    }
    let out = cu.evaluate(&input_value, 0.0);
    assert_eq!(out.len(), 1, "{expr}");
    Ok(out[0].1)
}

fn eval(expr: &str, ty: GlslType) -> Value {
    try_eval(expr, ty).unwrap_or_else(|e| panic!("`{expr}` failed: {e}"))
}

fn float(expr: &str) -> f32 {
    match eval(expr, GlslType::FLOAT) {
        Value::Float(x) => x,
        v => panic!("{expr}: {v:?}"),
    }
}

fn int(expr: &str) -> i32 {
    match eval(expr, GlslType::INT) {
        Value::Int(x) => x,
        v => panic!("{expr}: {v:?}"),
    }
}

fn boolean(expr: &str) -> bool {
    match eval(expr, GlslType::BOOL) {
        Value::Bool(x) => x,
        v => panic!("{expr}: {v:?}"),
    }
}

fn vec(expr: &str, n: u8) -> Vec<f32> {
    match eval(expr, GlslType::vector(sb_core::ScalarKind::Float, n)) {
        Value::Vec2(v) => v.to_vec(),
        Value::Vec3(v) => v.to_vec(),
        Value::Vec4(v) => v.to_vec(),
        v => panic!("{expr}: {v:?}"),
    }
}

fn err(expr: &str, ty: GlslType) -> String {
    try_eval(expr, ty).expect_err(expr)
}

#[track_caller]
fn close(a: f32, b: f32) {
    assert!((a - b).abs() <= 1e-5 * b.abs().max(1.0), "{a} != {b}");
}

#[test]
fn public_types_are_thread_safe() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CustomUniforms>();
    assert_send_sync::<Value>();
    assert_send_sync::<Expr>();
    assert_send_sync::<ExprError>();
    assert_send_sync::<Std140Error>();
}

#[test]
fn arithmetic_precedence_and_associativity() {
    assert_eq!(float("2 + 3 * 4"), 14.0);
    assert_eq!(float("(2 + 3) * 4"), 20.0);
    assert_eq!(float("10 - 4 - 3"), 3.0);
    assert_eq!(float("8 / 2 / 2"), 2.0);
    assert_eq!(float("2 * 3 % 4"), 2.0);
    assert_eq!(float("-2 * 3"), -6.0);
    assert_eq!(float("- -2"), 2.0);
    assert_eq!(float("1 + 2 * 3 - 4 / 2"), 5.0);
    assert!(boolean("1 + 1 == 2 && 3 > 2 || false"));
    assert!(
        boolean("true || false && false"),
        "&& binds tighter than ||"
    );
    assert!(!boolean("(true || false) && false"));
    assert!(boolean("1 < 2 == true"));
}

#[test]
fn int_and_float_promotion() {
    assert_eq!(float("7 / 2"), 3.5, "/ always divides as floats");
    assert_eq!(int("7 / 2"), 3);
    assert_eq!(int("-7 / 2"), -3, "float to int truncates");
    assert_eq!(int("7 % 3"), 1);
    assert_eq!(int("-7 % 3"), -1, "Java remainder");
    assert_eq!(int("i % 0"), 0, "no division-by-zero panic");
    assert_eq!(float("7.5 % 2"), 1.5);
    assert_eq!(
        float("2147483647 + 1"),
        -2_147_483_648.0,
        "int + int stays int (wrapping)"
    );
    assert_eq!(float("2147483647 + 1.0"), 2_147_483_648.0);
    assert_eq!(float("i + 0.5"), 7.5);
    assert_eq!(int("i * 2"), 14);
    assert_eq!(float("1 / 0"), f32::INFINITY);
    assert_eq!(int("2.9"), 2);
    assert_eq!(int("-2.9"), -2);
    assert_eq!(int("1e20"), i32::MAX);
    assert!(boolean("0.5"));
    assert!(!boolean("0"));
    assert!(!boolean("i - 7"));
    assert_eq!(float("true"), 1.0);
    assert_eq!(int("b"), 1);
    assert_eq!(vec("1.5", 3), vec![1.5; 3], "scalar splats into a vector");
    assert_eq!(vec("v4", 2), vec![1.0, 2.0], "longer vector truncates");
    assert_eq!(err("v2", GlslType::VEC3), "expr.type");
    assert_eq!(err("v2", GlslType::FLOAT), "expr.type");
    assert_eq!(err("m", GlslType::FLOAT), "expr.type");
}

#[test]
fn comparisons_and_logic() {
    assert!(boolean("1 < 2"));
    assert!(boolean("2 <= 2"));
    assert!(boolean("3 > 2.5"));
    assert!(boolean("3 >= 3.0"));
    assert!(boolean("1 == 1.0"));
    assert!(boolean("1 != 2"));
    assert!(!boolean("1 ≠ 1"));
    assert!(boolean("1 ≤ 1 && 2 ≥ 1"));
    assert!(!boolean("true && false"));
    assert!(boolean("true || false"));
    assert!(!boolean("!true"));
    assert!(boolean("!0"), "numbers are accepted as conditions");
    assert!(boolean("b == true"));
    assert!(boolean("b == 1"), "booleans compare as 0/1");
    assert!(boolean("v2 == vec2(1, 2)"));
    assert!(!boolean("v3 != vec3(1, 2, 3)"));
    assert!(boolean("m == m"));
    assert!(boolean("f > 2 && i == 7"));
    assert_eq!(err("v2 < v2", GlslType::BOOL), "expr.type");
    assert_eq!(err("v2 == 1", GlslType::BOOL), "expr.type");
    assert_eq!(err("v2 && true", GlslType::BOOL), "expr.type");
}

#[test]
fn logic_evaluates_both_operands_like_iris() {
    // Iris' `and`/`or` are plain functions: both operands run every frame. A
    // `smooth()` behind a false `&&` must keep tracking its target (regression: it
    // used to be short-circuited, freezing and later jumping).
    let (mut cu, d) = compile(&[
        def("gate", GlslType::BOOL, "x > 0.5 && smooth(y) > 0.25"),
        var("probe", GlslType::FLOAT, "smooth(y)"),
        def("p", GlslType::FLOAT, "probe"),
    ]);
    assert!(d.is_empty(), "{d:?}");
    let drv = Driver { x: Cell::new(1.0) };
    // y = 1 - x: frame 0 snaps both smooths to 0.
    cu.evaluate(&drv, 0.1);
    // x = 0 (gate's left side false), y = 1 for three frames of one half-life each.
    drv.x.set(0.0);
    for _ in 0..3 {
        cu.evaluate(&drv, 0.1);
    }
    // Open the gate with y just under 0.5. The gated smooth has been tracking (0.875,
    // like the ungated probe), so it lands at (0.875 + 0.499) / 2 = 0.687 > 0.25.
    // Short-circuited, it would still be at 0 and land at 0.2495 < 0.25.
    drv.x.set(0.5 + 1e-3);
    let out = cu.evaluate(&drv, 0.1);
    assert_eq!(out[0].1, Value::Bool(true), "{out:?}");
    close(out[1].1.to_f32(), 0.687);
    // random() on either side is drawn every time (same sequence as two plain calls).
    let seed = 99;
    let (mut a, d) = compile(&[
        def("l", GlslType::BOOL, "false && random() < 2.0"),
        def("r", GlslType::FLOAT, "random()"),
    ]);
    assert!(d.is_empty(), "{d:?}");
    a.set_seed(seed);
    let (mut b, _) = compile(&[
        def("l", GlslType::FLOAT, "random()"),
        def("r", GlslType::FLOAT, "random()"),
    ]);
    b.set_seed(seed);
    assert_eq!(
        a.evaluate(&input_value, 0.0)[1].1,
        b.evaluate(&input_value, 0.0)[1].1
    );
    // Results are unchanged for pure operands.
    assert!(!boolean("false && true"));
    assert!(boolean("true || false"));
}

#[test]
fn vector_members_and_swizzles() {
    assert_eq!(float("v3.y"), 2.0);
    assert_eq!(float("v3.b"), 3.0);
    assert_eq!(float("v4.q"), 4.0);
    assert_eq!(float("v4.w"), 4.0);
    assert_eq!(float("v4.3"), 4.0);
    assert_eq!(float("v4.0"), 1.0);
    assert_eq!(float("v2.t"), 2.0);
    assert_eq!(vec("v4.zyx", 3), vec![3.0, 2.0, 1.0]);
    assert_eq!(vec("v3.xx", 2), vec![1.0, 1.0]);
    assert_eq!(vec("v2.yxyx", 4), vec![2.0, 1.0, 2.0, 1.0]);
    assert_eq!(vec("v4.rgb", 3), vec![1.0, 2.0, 3.0]);
    assert_eq!(float("vec3(1, 2, 3).z"), 3.0);
    assert_eq!(float("(v2 * 2).y"), 4.0);
    assert_eq!(float("v4.xyz.z"), 3.0);
    assert_eq!(
        float("eyeBrightness.y / 240"),
        1.0,
        "ivec2 inputs read as float vectors"
    );
    assert_eq!(err("v2.z", GlslType::FLOAT), "expr.type");
    assert_eq!(err("v3.xr", GlslType::VEC2), "expr.type");
    assert_eq!(err("v4.xyzwx", GlslType::VEC4), "expr.type");
    assert_eq!(err("v4.01", GlslType::VEC2), "expr.type");
    assert_eq!(err("v4.e", GlslType::FLOAT), "expr.type");
    assert_eq!(err("f.x", GlslType::FLOAT), "expr.type");
}

#[test]
fn matrix_element_access() {
    // m.i.j is GLSL m[i][j] = element i*4+j of the column-major array (Iris/OptiFine).
    assert_eq!(float("m.0.0"), 0.0);
    assert_eq!(float("m.2.1"), 9.0);
    assert_eq!(float("m.3.0"), 12.0);
    assert_eq!(float("m.1.3"), 7.0);
    assert_eq!(vec("m.3", 4), vec![12.0, 13.0, 14.0, 15.0]);
    assert_eq!(float("m.x.y"), 1.0);
    assert_eq!(float("m3.1.1"), 5.0, "mat3 inputs are embedded in a mat4");
    // The row-vector product packs write by hand: (M * v).x
    assert_eq!(
        float("m.0.0 * v3.x + m.1.0 * v3.y + m.2.0 * v3.z"),
        0.0 + 4.0 * 2.0 + 8.0 * 3.0
    );
    assert_eq!(err("m.4", GlslType::VEC4), "expr.type");
    assert_eq!(err("m.01", GlslType::VEC4), "expr.type");
    assert_eq!(err("m + m", GlslType::FLOAT), "expr.type");
    assert_eq!(err("-m", GlslType::FLOAT), "expr.type");
}

#[test]
fn trigonometry_and_angles() {
    close(float("sin(0)"), 0.0);
    close(float("sin(pi / 2)"), 1.0);
    close(float("cos(0)"), 1.0);
    close(float("tan(pi / 4)"), 1.0);
    close(float("asin(1)"), FRAC_PI_2);
    close(float("acos(1)"), 0.0);
    close(float("atan(1)"), FRAC_PI_4);
    close(float("atan(1, -1)"), 3.0 * FRAC_PI_4);
    close(float("atan2(1, -1)"), 3.0 * FRAC_PI_4);
    close(float("atan2(-1, 0)"), -FRAC_PI_2);
    close(float("torad(180)"), PI);
    close(float("radians(90)"), FRAC_PI_2);
    close(float("todeg(pi)"), 180.0);
    close(float("degrees(pi / 2)"), 90.0);
}

#[test]
fn exponentials_and_logs() {
    close(float("exp(1)"), E);
    close(float("exp2(3)"), 8.0);
    close(float("exp10(2)"), 100.0);
    close(float("pow(2, 10)"), 1024.0);
    close(float("log(exp(1))"), 1.0);
    close(float("log(2, 8)"), 3.0);
    close(float("log2(8)"), 3.0);
    close(float("log10(1000)"), 3.0);
    close(float("sqrt(16)"), 4.0);
    close(float("inversesqrt(4)"), 0.5);
    assert!(float("sqrt(-1)").is_nan(), "NaN is a value, not a panic");
}

#[test]
fn rounding_and_sign() {
    assert_eq!(float("abs(-2.5)"), 2.5);
    assert_eq!(int("abs(-3)"), 3);
    assert_eq!(
        float("abs(-2147483647 - 1)"),
        -2_147_483_648.0,
        "int abs stays int (Java semantics)"
    );
    assert_eq!(float("sign(-2)"), -1.0);
    assert_eq!(float("sign(0.0)"), 0.0);
    assert_eq!(float("signum(3.5)"), 1.0);
    assert_eq!(float("floor(-1.5)"), -2.0);
    assert_eq!(float("ceil(1.2)"), 2.0);
    assert_eq!(float("frac(1.25)"), 0.25);
    assert_eq!(float("frac(-0.25)"), 0.75);
    assert_eq!(float("round(2.5)"), 3.0);
    assert_eq!(float("round(-2.5)"), -2.0);
    assert_eq!(float("round(1.4)"), 1.0);
    assert_eq!(int("floor(i)"), 7);
}

#[test]
fn range_functions() {
    assert_eq!(float("min(3, 1, 2)"), 1.0);
    assert_eq!(
        float("max(1, 2, 5)"),
        5.0,
        "true variadic max (Iris only compares the first two)"
    );
    assert_eq!(float("max(1, 5, 2, 4)"), 5.0);
    assert_eq!(float("min(1.5, 2)"), 1.5);
    assert_eq!(
        float("max(2147483647, 1) + 1"),
        -2_147_483_648.0,
        "int max stays int"
    );
    assert_eq!(float("clamp(5, 0, 1)"), 1.0);
    assert_eq!(float("clamp(-1, 0, 1)"), 0.0);
    assert_eq!(float("clamp(0.5, 0, 1)"), 0.5);
    assert_eq!(float("clamp(5, 3, 1)"), 3.0, "inverted bounds do not panic");
    assert_eq!(int("clamp(5, 0, 3)"), 3);
    assert_eq!(float("mix(0, 10, 0.25)"), 2.5);
    assert_eq!(float("lerp(0.25, 0, 10)"), 2.5);
    assert_eq!(float("edge(0.5, 0.4)"), 0.0);
    assert_eq!(float("edge(0.5, 0.5)"), 1.0);
    assert_eq!(int("edge(3, 2)"), 0);
    assert_eq!(float("fmod(-1, 3)"), 2.0);
    assert_eq!(int("fmod(-7, 3)"), 2);
    assert_eq!(float("fmod(5.5, 2)"), 1.5);
    assert_eq!(float("fmod(-5.5, 2)"), 0.5);
    assert_eq!(err("min(1)", GlslType::FLOAT), "expr.arity");
    assert_eq!(err("clamp(1, 2)", GlslType::FLOAT), "expr.arity");
    assert_eq!(err("min(true, 1)", GlslType::FLOAT), "expr.type");
}

#[test]
fn vector_math_is_component_wise() {
    assert_eq!(vec("v3 + v3", 3), vec![2.0, 4.0, 6.0]);
    assert_eq!(vec("v3 * 2", 3), vec![2.0, 4.0, 6.0]);
    assert_eq!(vec("2 * v3", 3), vec![2.0, 4.0, 6.0]);
    assert_eq!(vec("v3 / 2", 3), vec![0.5, 1.0, 1.5]);
    assert_eq!(vec("1 - v2", 2), vec![0.0, -1.0]);
    assert_eq!(vec("-v2", 2), vec![-1.0, -2.0]);
    assert_eq!(vec("v4 % 3", 4), vec![1.0, 2.0, 0.0, 1.0]);
    assert_eq!(vec("abs(vec2(-1, 2))", 2), vec![1.0, 2.0]);
    assert_eq!(vec("floor(v3 * 1.5)", 3), vec![1.0, 3.0, 4.0]);
    assert_eq!(vec("min(v3, 2)", 3), vec![1.0, 2.0, 2.0]);
    assert_eq!(vec("max(v2, vec2(0, 5))", 2), vec![1.0, 5.0]);
    assert_eq!(vec("clamp(v3, 1.5, 2.5)", 3), vec![1.5, 2.0, 2.5]);
    assert_eq!(vec("pow(v2, 2)", 2), vec![1.0, 4.0]);
    assert_eq!(vec("mix(v2, vec2(3, 4), 0.5)", 2), vec![2.0, 3.0]);
    assert_eq!(vec("sin(vec2(0, 0))", 2), vec![0.0, 0.0]);
    assert_eq!(vec("fmod(vec2(-1, 4), 3)", 2), vec![2.0, 1.0]);
    assert_eq!(err("v2 + v3", GlslType::VEC2), "expr.type");
    assert_eq!(err("v2 * true", GlslType::VEC2), "expr.type");
}

#[test]
fn constructors() {
    assert_eq!(vec("vec2(1, 2)", 2), vec![1.0, 2.0]);
    assert_eq!(vec("vec3(1)", 3), vec![1.0; 3]);
    assert_eq!(vec("vec4(v2, 3, 4)", 4), vec![1.0, 2.0, 3.0, 4.0]);
    assert_eq!(vec("vec4(v3, i)", 4), vec![1.0, 2.0, 3.0, 7.0]);
    assert_eq!(vec("vec3(f, b, 1)", 3), vec![2.5, 1.0, 1.0]);
    assert_eq!(vec("ivec2(1.7, -1.7)", 2), vec![1.0, -1.0]);
    assert_eq!(vec("bvec2(0, 3)", 2), vec![0.0, 1.0]);
    // GLSL: a single longer vector truncates, and the last argument may be only
    // partly used (regression: these were rejected as arity errors).
    assert_eq!(vec("vec3(v4)", 3), vec![1.0, 2.0, 3.0]);
    assert_eq!(vec("vec2(v3)", 2), vec![1.0, 2.0]);
    assert_eq!(vec("vec3(v2, v2)", 3), vec![1.0, 2.0, 1.0]);
    assert_eq!(vec("vec2(f, v4)", 2), vec![2.5, 1.0]);
    assert_eq!(vec("ivec2(v3 * 1.5)", 2), vec![1.0, 3.0]);
    assert_eq!(err("vec4(v3)", GlslType::VEC4), "expr.arity");
    assert_eq!(
        err("vec2(v2, 1)", GlslType::VEC2),
        "expr.arity",
        "unused argument"
    );
    assert_eq!(err("vec3(1, 2, 3, 4)", GlslType::VEC3), "expr.arity");
    assert_eq!(err("vec2(1, 2, 3)", GlslType::VEC2), "expr.arity");
    assert_eq!(err("vec2()", GlslType::VEC2), "expr.arity");
    assert_eq!(err("vec4(m)", GlslType::VEC4), "expr.type");
}

#[test]
fn if_with_multiple_branches() {
    assert_eq!(float("if(true, 1, 2)"), 1.0);
    assert_eq!(float("if(false, 1, 2)"), 2.0);
    assert_eq!(float("if(i == 1, 10, i == 7, 20, 30)"), 20.0);
    assert_eq!(float("if(i == 1, 10, i == 2, 20, 30)"), 30.0);
    assert_eq!(
        float("if(i == 7, 10, i == 7, 20, 30)"),
        10.0,
        "first true condition wins"
    );
    assert_eq!(float("if(i, 1, 2)"), 1.0, "numeric conditions are accepted");
    assert_eq!(float("if(b, 1, 2.5)"), 1.0);
    assert_eq!(vec("if(b, v3, vec3(0))", 3), vec![1.0, 2.0, 3.0]);
    assert_eq!(
        vec("if(!b, v3, 0)", 3),
        vec![0.0; 3],
        "scalar branch splats"
    );
    assert!(!boolean("if(false, true, false)"));
    assert!(boolean("ifb(false, false, true)"));
    assert!(boolean("ifb(true, 1, 0)"));
    assert_eq!(err("if(true, 1)", GlslType::FLOAT), "expr.arity");
    assert_eq!(err("if(true, 1, 2, 3)", GlslType::FLOAT), "expr.arity");
    assert_eq!(err("if(true, v2, v3)", GlslType::VEC2), "expr.type");
    assert_eq!(err("if(v2, 1, 2)", GlslType::FLOAT), "expr.type");
    assert_eq!(err("if(true, true, 2)", GlslType::FLOAT), "expr.type");
    // 15 branches, as in Bliss' Halton tables.
    let halton = (0..15)
        .map(|k| format!("i == {k}, {k}.5"))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(float(&format!("if({halton}, -1)")), 7.5);
}

#[test]
fn boolean_functions() {
    assert!(boolean("between(5, 1, 10)"));
    assert!(boolean("between(10, 1, 10)"), "inclusive");
    assert!(!boolean("between(0.5, 1, 2)"));
    assert!(boolean("equals(1.0, 1.05, 0.1)"));
    assert!(!boolean("equals(1, 1.2, 0.1)"));
    assert!(boolean("equals(1, 1)"), "two-argument equals is ==");
    assert!(boolean("in(3, 1, 2, 3)"));
    assert!(!boolean("in(4, 1, 2, 3)"));
    assert!(boolean("in(f, 2.5)"));
    assert!(boolean("in(BIOME_DESERT, BIOME_PLAINS, BIOME_DESERT)"));
    assert_eq!(err("in(1)", GlslType::BOOL), "expr.arity");
    assert_eq!(err("between(v2, 1, 2)", GlslType::BOOL), "expr.type");
}

#[test]
fn constants_and_misc_functions() {
    close(float("pi"), PI);
    assert_eq!(int("CAT_DESERT"), 12);
    assert_eq!(int("PPT_SNOW"), 2);
    assert_eq!(int("BIOME_DESERT"), 14);
    assert_eq!(float("print(1, 10, f)"), 2.5);
    assert_eq!(err("print(f)", GlslType::FLOAT), "expr.arity");
}

#[test]
fn random_functions() {
    let (mut cu, d) = compile(&[
        def("r", GlslType::FLOAT, "random()"),
        def("rr", GlslType::FLOAT, "random(5, 6)"),
        def("ri", GlslType::INT, "randomInt()"),
        def("rb", GlslType::INT, "randomInt(10)"),
        def("rir", GlslType::INT, "randomInt(3, 5)"),
        def("re", GlslType::INT, "randomInt(5, 5)"),
    ]);
    assert!(d.is_empty(), "{d:?}");
    let mut seen_r = Vec::new();
    let mut seen_ri = Vec::new();
    for _ in 0..200 {
        let out = cu.evaluate(&input_value, 0.016);
        let Value::Float(r) = out[0].1 else { panic!() };
        let Value::Float(rr) = out[1].1 else { panic!() };
        let Value::Int(ri) = out[2].1 else { panic!() };
        let Value::Int(rb) = out[3].1 else { panic!() };
        let Value::Int(rir) = out[4].1 else { panic!() };
        assert!((0.0..1.0).contains(&r));
        assert!((5.0..6.0).contains(&rr));
        assert!((0..10).contains(&rb));
        assert!((3..5).contains(&rir));
        assert_eq!(out[5].1, Value::Int(5), "empty range yields min");
        seen_r.push(r);
        seen_ri.push(ri);
    }
    seen_r.dedup();
    assert!(seen_r.len() > 150, "a new value every evaluation");
    // Deterministic for a given seed.
    let run = |seed: u64| {
        let (mut cu, _) = compile(&[def("r", GlslType::FLOAT, "random()")]);
        cu.set_seed(seed);
        (0..5)
            .map(|_| cu.evaluate(&input_value, 0.0)[0].1)
            .collect::<Vec<_>>()
    };
    assert_eq!(run(7), run(7));
    assert_ne!(run(7), run(8));
    // reset() restarts the sequence.
    let (mut cu, _) = compile(&[def("r", GlslType::FLOAT, "random()")]);
    cu.set_seed(3);
    let first = cu.evaluate(&input_value, 0.0)[0].1;
    cu.evaluate(&input_value, 0.0);
    cu.reset();
    assert_eq!(cu.evaluate(&input_value, 0.0)[0].1, first);
}

/// Inputs whose `x` value can change between frames.
struct Driver {
    x: Cell<f32>,
}

impl UniformInputs for Driver {
    fn get(&self, name: &str) -> Option<Value> {
        match name {
            "x" => Some(Value::Float(self.x.get())),
            "y" => Some(Value::Float(1.0 - self.x.get())),
            other => input_value(other),
        }
    }
}

fn smooth_run(expr: &str, frames: &[(f32, f32)]) -> Vec<f32> {
    let (mut cu, d) = compile(&[def("s", GlslType::FLOAT, expr)]);
    assert!(d.is_empty(), "{d:?}");
    let drv = Driver { x: Cell::new(0.0) };
    frames
        .iter()
        .map(|&(x, dt)| {
            drv.x.set(x);
            match cu.evaluate(&drv, dt)[0].1 {
                Value::Float(v) => v,
                v => panic!("{v:?}"),
            }
        })
        .collect()
}

#[test]
fn smooth_over_simulated_frames() {
    // Default fade 1s => half-life 0.1s.
    let v = smooth_run(
        "smooth(x)",
        &[(5.0, 0.016), (1.0, 0.1), (1.0, 0.1), (1.0, 0.1)],
    );
    assert_eq!(v[0], 5.0, "first evaluation snaps");
    close(v[1], 3.0);
    close(v[2], 2.0);
    close(v[3], 1.5);
    // fade 2s up => half-life 0.2s; down fade 0 => snaps.
    let v = smooth_run(
        "smooth(x, 2, 0)",
        &[(0.0, 0.1), (1.0, 0.2), (1.0, 0.2), (0.0, 0.1)],
    );
    close(v[1], 0.5);
    close(v[2], 0.75);
    assert_eq!(v[3], 0.0);
    // Asymmetric: fast up, slow down.
    let v = smooth_run("smooth(x, 1, 10)", &[(0.0, 0.1), (1.0, 0.1), (0.0, 1.0)]);
    close(v[1], 0.5);
    close(v[2], 0.5 * 0.5f32.powf(1.0));
    // Converges within 0.1% after the fade time.
    let frames: Vec<(f32, f32)> = std::iter::once((0.0, 0.0))
        .chain(std::iter::repeat_n((1.0, 1.0 / 60.0), 120))
        .collect();
    let v = smooth_run("smooth(x, 2)", &frames);
    assert!((v[120] - 1.0).abs() < 1.5e-3, "{}", v[120]);
    // dt = 0 (or invalid) does not move the value.
    let v = smooth_run(
        "smooth(x)",
        &[(0.0, 0.1), (1.0, 0.0), (1.0, f32::NAN), (1.0, -1.0)],
    );
    assert_eq!(&v[1..], &[0.0, 0.0, 0.0]);
    // Explicit id (number literal first) followed by value and fade.
    let v = smooth_run("smooth(5, x, 1)", &[(0.0, 0.1), (1.0, 0.1)]);
    close(v[1], 0.5);
    let v = smooth_run("smooth(1.0, x)", &[(0.0, 0.1), (1.0, 0.1)]);
    close(v[1], 0.5);
    // Value first, then fade (not an id).
    let v = smooth_run("smooth(x, 2)", &[(0.0, 0.1), (1.0, 0.2)]);
    close(v[1], 0.5);
    let v = smooth_run("smooth(3, x, 2, 0)", &[(0.0, 0.1), (1.0, 0.2), (0.0, 0.1)]);
    close(v[1], 0.5);
    assert_eq!(v[2], 0.0);
    // Dynamic fade time from an expression.
    let v = smooth_run("smooth(x, f - 0.5)", &[(0.0, 0.1), (1.0, 0.2)]);
    close(v[1], 0.5);
}

#[test]
fn smooth_state_is_per_call_site() {
    // Complementary reuses id 4 for two different values; they must not interfere.
    let (mut cu, d) = compile(&[
        def("sa", GlslType::FLOAT, "smooth(4, x, 1)"),
        def("sb", GlslType::FLOAT, "smooth(4, y, 1)"),
        def("sc", GlslType::FLOAT, "smooth(x) + smooth(x, 0)"),
    ]);
    assert!(d.is_empty(), "{d:?}");
    let drv = Driver { x: Cell::new(0.0) };
    cu.evaluate(&drv, 0.1);
    drv.x.set(1.0);
    let out = cu.evaluate(&drv, 0.1);
    close(out[0].1.to_f32(), 0.5);
    close(out[1].1.to_f32(), 0.5);
    close(out[2].1.to_f32(), 1.5);
    // reset() makes the next evaluation snap again.
    cu.reset();
    drv.x.set(0.0);
    let out = cu.evaluate(&drv, 0.1);
    assert_eq!(out[0].1, Value::Float(0.0));
}

#[test]
fn smooth_vectors_and_errors() {
    let (mut cu, d) = compile(&[def("s", GlslType::VEC2, "smooth(vec2(x, y), 1)")]);
    assert!(d.is_empty(), "{d:?}");
    let drv = Driver { x: Cell::new(0.0) };
    assert_eq!(cu.evaluate(&drv, 0.1)[0].1, Value::Vec2([0.0, 1.0]));
    drv.x.set(1.0);
    let Value::Vec2([a, b]) = cu.evaluate(&drv, 0.1)[0].1 else {
        panic!("not a vec2")
    };
    close(a, 0.5);
    close(b, 0.5);
    assert_eq!(err("smooth()", GlslType::FLOAT), "expr.arity");
    assert_eq!(err("smooth(1, x, 1, 1, 1)", GlslType::FLOAT), "expr.arity");
    assert_eq!(err("smooth(true)", GlslType::FLOAT), "expr.type");
    assert_eq!(err("smooth(x, v2)", GlslType::FLOAT), "expr.type");
    // A computed id (four arguments) is validated like any other expression
    // (regression: it used to be skipped, hiding unknown identifiers).
    assert_eq!(
        err("smooth(nope, x, 1, 1)", GlslType::FLOAT),
        "expr.unknown-identifier"
    );
    assert_eq!(err("smooth(v2, x, 1, 1)", GlslType::FLOAT), "expr.type");
    let v = smooth_run("smooth(i * 2, x, 1, 1)", &[(0.0, 0.1), (1.0, 0.1)]);
    close(v[1], 0.5);
}

#[test]
fn smooth_inside_untaken_branch_does_not_advance() {
    let v = smooth_run(
        "if(x > 0.5, smooth(x), -1)",
        &[(1.0, 0.1), (0.0, 0.1), (0.0, 0.1), (2.0, 0.1)],
    );
    assert_eq!(v[0], 1.0);
    assert_eq!(&v[1..3], &[-1.0, -1.0]);
    close(v[3], 1.5);
}

#[test]
fn errors_carry_positions() {
    let (_, d) = compile(&[def("u", GlslType::FLOAT, "1 + nope")]);
    let e = d.errors().next().expect("error");
    assert_eq!(e.code, "expr.unknown-identifier");
    assert!(e.message.contains("uniform.float.u"), "{}", e.message);
    assert!(e.message.contains("column 5"), "{}", e.message);
    assert_eq!(err("nope(1)", GlslType::FLOAT), "expr.unknown-function");
    assert_eq!(err("1 +", GlslType::FLOAT), "expr.syntax");
    assert_eq!(err("true + 1", GlslType::FLOAT), "expr.type");
    assert_eq!(err("sin(true)", GlslType::FLOAT), "expr.type");
    assert_eq!(
        err("arr", GlslType::FLOAT),
        "expr.type",
        "array inputs are unsupported"
    );
    let (_, d) = compile(&[def("u", GlslType::FLOAT, "(1 + 2")]);
    assert!(d.errors().next().unwrap().message.contains("column 1"));
}

#[test]
fn definitions_order_variables_and_dependencies() {
    let (mut cu, d) = compile(&[
        // Uses a variable defined later (Iris sorts by dependencies).
        def("out", GlslType::VEC2, "vec2(half, twice)"),
        var("half", GlslType::FLOAT, "f / 2"),
        var("twice", GlslType::FLOAT, "half * 4"),
        def("chain", GlslType::INT, "frameCounter % 8"),
        def("from_uniform", GlslType::FLOAT, "chain + 0.5"),
    ]);
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(
        cu.outputs(),
        vec![
            ("out".to_string(), GlslType::VEC2),
            ("chain".to_string(), GlslType::INT),
            ("from_uniform".to_string(), GlslType::FLOAT),
        ]
    );
    assert_eq!(
        cu.referenced_inputs(),
        vec!["f".to_string(), "frameCounter".to_string()]
    );
    assert_eq!(cu.len(), 5);
    let inputs = |name: &str| match name {
        "frameCounter" => Some(Value::Int(21)),
        other => input_value(other),
    };
    let out = cu.evaluate(&inputs, 0.0);
    assert_eq!(out[0].1, Value::Vec2([1.25, 5.0]));
    assert_eq!(out[1].1, Value::Int(5));
    assert_eq!(out[2].1, Value::Float(5.5));
    assert_eq!(cu.value("half"), Some(Value::Float(1.25)));
    assert_eq!(cu.value("missing"), None);
}

#[test]
fn cycles_and_dropped_dependencies() {
    let (mut cu, d) = compile(&[
        def("a", GlslType::FLOAT, "b + 1"),
        def("b", GlslType::FLOAT, "a + 1"),
        def("self_ref", GlslType::FLOAT, "self_ref * 2"),
        def("uses_cycle", GlslType::FLOAT, "a * 2"),
        def("broken", GlslType::FLOAT, "1 +"),
        var("uses_broken", GlslType::FLOAT, "broken * 2"),
        def("uses_uses_broken", GlslType::FLOAT, "uses_broken"),
        def("ok", GlslType::FLOAT, "f"),
    ]);
    let codes: Vec<(&str, &str)> = d
        .errors()
        .map(|e| (e.code.as_str(), e.message.split(':').next().unwrap_or("")))
        .collect();
    assert!(
        codes.contains(&("expr.cycle", "uniform.float.a")),
        "{codes:?}"
    );
    assert!(codes.contains(&("expr.cycle", "uniform.float.b")));
    assert!(codes.contains(&("expr.cycle", "uniform.float.self_ref")));
    // Regression: a definition that only *uses* a cycle is not part of it.
    assert!(codes.contains(&("expr.dropped-dependency", "uniform.float.uses_cycle")));
    let cycle_msg = d
        .errors()
        .find(|e| e.message.starts_with("uniform.float.a:"))
        .map(|e| e.message.clone())
        .unwrap_or_default();
    assert!(
        cycle_msg.contains("(`a`, `b`)") || cycle_msg.contains("(`b`, `a`)"),
        "only the cycle members are listed: {cycle_msg}"
    );
    assert!(codes.contains(&("expr.syntax", "uniform.float.broken")));
    assert!(codes.contains(&("expr.dropped-dependency", "variable.float.uses_broken")));
    assert!(codes.contains(&("expr.dropped-dependency", "uniform.float.uses_uses_broken")));
    assert_eq!(cu.outputs(), vec![("ok".to_string(), GlslType::FLOAT)]);
    assert_eq!(
        cu.evaluate(&input_value, 0.0),
        vec![("ok".to_string(), Value::Float(2.5))]
    );
}

#[test]
fn cycle_classification_is_exact() {
    // c1 <-> c2 is one cycle, d1 -> d2 -> d3 -> d1 another; `bridge` depends on the
    // first and is used by the second's member d3 (so it is *on* the second cycle's
    // path only if it is reachable from d3 and reaches d3: it is not).
    let mut defs = vec![
        def("c1", GlslType::FLOAT, "c2"),
        def("c2", GlslType::FLOAT, "c1 + 1"),
        def("d1", GlslType::FLOAT, "d2"),
        def("d2", GlslType::FLOAT, "d3"),
        def("d3", GlslType::FLOAT, "d1 + bridge"),
        def("bridge", GlslType::FLOAT, "c1"),
        def("fine", GlslType::FLOAT, "f"),
    ];
    // A long chain hanging off the cycle: classified without deep recursion.
    defs.push(def("chain0", GlslType::FLOAT, "c2"));
    for k in 1..2000 {
        defs.push(def(
            &format!("chain{k}"),
            GlslType::FLOAT,
            &format!("chain{}", k - 1),
        ));
    }
    let (cu, d) = compile(&defs);
    let code_of = |key: &str| {
        d.errors()
            .find(|e| e.message.starts_with(&format!("{key}:")))
            .map(|e| e.code.clone())
    };
    for name in ["c1", "c2", "d1", "d2", "d3"] {
        assert_eq!(
            code_of(&format!("uniform.float.{name}")).as_deref(),
            Some("expr.cycle"),
            "{name}"
        );
    }
    for name in ["bridge", "chain0", "chain1999"] {
        assert_eq!(
            code_of(&format!("uniform.float.{name}")).as_deref(),
            Some("expr.dropped-dependency"),
            "{name}"
        );
    }
    let d_msg = d
        .errors()
        .find(|e| e.message.starts_with("uniform.float.d1:"))
        .unwrap();
    assert!(
        !d_msg.message.contains("`c1`") && !d_msg.message.contains("`bridge`"),
        "{}",
        d_msg.message
    );
    assert_eq!(cu.outputs(), vec![("fine".to_string(), GlslType::FLOAT)]);
}

#[test]
fn huge_cycle_is_classified_quickly_with_short_messages() {
    // Regression: classification was quadratic and listed every member in every
    // message.
    let n = 20_000;
    let defs: Vec<CustomUniform> = (0..n)
        .map(|k| {
            def(
                &format!("r{k}"),
                GlslType::FLOAT,
                &format!("r{}", (k + 1) % n),
            )
        })
        .collect();
    let started = std::time::Instant::now();
    let (cu, d) = compile(&defs);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
    assert!(cu.is_empty());
    assert_eq!(d.errors().filter(|e| e.code == "expr.cycle").count(), n);
    let first = d.errors().next().unwrap();
    assert!(
        first.message.contains(&format!("and {} more", n - 8)),
        "{}",
        first.message
    );
    assert!(first.message.len() < 300, "{}", first.message);
}

#[test]
fn many_distinct_identifiers_compile_in_linear_time() {
    // Regression: input slots and `Expr::identifiers` deduplicated with linear scans.
    let n = 50_000;
    let src = format!(
        "max({})",
        (0..n)
            .map(|k| format!("in{k}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let started = std::time::Instant::now();
    let expr = parse(&src).unwrap();
    assert_eq!(expr.identifiers().len(), n);
    let (mut cu, d) = CustomUniforms::compile(
        &[def("u", GlslType::FLOAT, &src)],
        &|name: &str| name.starts_with("in").then_some(GlslType::FLOAT),
        &IndexMap::new(),
    );
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(cu.referenced_inputs().len(), n);
    let out = cu.evaluate(&|name: &str| Some(Value::Float(name.len() as f32)), 0.0);
    assert_eq!(
        out[0].1,
        Value::Float(7.0),
        "the longest name is in10000..in49999"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn duplicates_shadowing_and_invalid_definitions() {
    let (mut cu, d) = compile(&[
        def("u", GlslType::FLOAT, "1"),
        def("u", GlslType::FLOAT, "2"),
        var("u", GlslType::VEC3, "v3"),
        def("frameTimeCounter", GlslType::FLOAT, "100"),
        def("reads_shadowed", GlslType::FLOAT, "frameTimeCounter"),
        def("bad type", GlslType::FLOAT, "1"),
        def("mat", GlslType::MAT4, "m"),
        def("ivec", GlslType::IVEC2, "v2"),
    ]);
    let has = |sev: Severity, code: &str| d.iter().any(|x| x.severity == sev && x.code == code);
    assert!(has(Severity::Info, "expr.redefined"));
    assert!(has(Severity::Warning, "expr.duplicate"));
    assert!(has(Severity::Warning, "expr.shadows-builtin"));
    assert!(has(Severity::Error, "expr.invalid-name"));
    assert_eq!(
        d.errors().filter(|e| e.code == "expr.invalid-type").count(),
        2
    );
    let out = cu.evaluate(&input_value, 0.0);
    assert_eq!(
        out,
        vec![
            ("u".to_string(), Value::Float(2.0)),
            ("frameTimeCounter".to_string(), Value::Float(100.0)),
            ("reads_shadowed".to_string(), Value::Float(100.0)),
        ]
    );
}

#[test]
fn inputs_are_converted_and_default_to_zero() {
    let (mut cu, d) = compile(&[
        def("a", GlslType::FLOAT, "f + i"),
        def("v", GlslType::VEC3, "v3 * 2"),
    ]);
    assert!(d.is_empty());
    // Wrong types are converted; missing inputs read as zero.
    let inputs = |name: &str| match name {
        "f" => Some(Value::Int(2)),
        "v3" => Some(Value::Float(1.0)),
        _ => None,
    };
    let out = cu.evaluate(&inputs, 0.0);
    assert_eq!(out[0].1, Value::Float(2.0));
    assert_eq!(out[1].1, Value::Vec3([2.0; 3]));
}

#[test]
fn uniform_inputs_impls() {
    let (mut cu, _) = compile(&[def("a", GlslType::FLOAT, "f * 2")]);
    let mut hm: HashMap<String, Value> = HashMap::new();
    hm.insert("f".into(), Value::Float(1.0));
    assert_eq!(cu.evaluate(&hm, 0.0)[0].1, Value::Float(2.0));
    let mut bt: BTreeMap<String, Value> = BTreeMap::new();
    bt.insert("f".into(), Value::Float(2.0));
    assert_eq!(cu.evaluate(&bt, 0.0)[0].1, Value::Float(4.0));
    let mut im: IndexMap<String, Value> = IndexMap::new();
    im.insert("f".into(), Value::Float(3.0));
    assert_eq!(cu.evaluate(&im, 0.0)[0].1, Value::Float(6.0));
    assert_eq!(
        cu.evaluate(&|_: &str| Some(Value::Float(4.0)), 0.0)[0].1,
        Value::Float(8.0)
    );
}

#[test]
fn empty_set() {
    let (mut cu, d) = CustomUniforms::compile(&[], &input_type, &IndexMap::new());
    assert!(d.is_empty() && cu.is_empty());
    assert!(cu.evaluate(&input_value, 0.016).is_empty());
    let mut block = vec![0u8; 16];
    cu.evaluate_into_block(&BlockLayout::default(), &mut block, 0.016);
    assert_eq!(block, vec![0u8; 16]);
    assert!(CustomUniforms::default().outputs().is_empty());
}

fn member(name: &str, ty: GlslType, offset: u32, source: UniformSource) -> BlockMember {
    BlockMember {
        name: name.into(),
        ty,
        offset,
        source,
        default: None,
    }
}

fn builtin(name: &str) -> UniformSource {
    UniformSource::Builtin(name.into())
}

fn custom(name: &str) -> UniformSource {
    UniformSource::Custom(name.into())
}

fn frame_layout(shift: u32) -> BlockLayout {
    BlockLayout {
        name: "sb_Frame".into(),
        set: 0,
        binding: 0,
        size: 176 + shift,
        members: vec![
            member(
                "frameTimeCounter",
                GlslType::FLOAT,
                shift,
                builtin("frameTimeCounter"),
            ),
            member(
                "frameCounter",
                GlslType::INT,
                4 + shift,
                builtin("frameCounter"),
            ),
            member("is_hurt", GlslType::BOOL, 8 + shift, builtin("is_hurt")),
            member(
                "sunPosition",
                GlslType::VEC3,
                16 + shift,
                builtin("sunPosition"),
            ),
            member(
                "eyeBrightness",
                GlslType::IVEC2,
                32 + shift,
                builtin("eyeBrightness"),
            ),
            member(
                "gbufferModelView",
                GlslType::MAT4,
                48 + shift,
                builtin("gbufferModelView"),
            ),
            member("u_float", GlslType::FLOAT, 112 + shift, custom("u_float")),
            member("u_bool", GlslType::BOOL, 116 + shift, custom("u_bool")),
            member("u_int", GlslType::INT, 120 + shift, custom("u_int")),
            member(
                "u_float__int",
                GlslType::INT,
                124 + shift,
                custom("u_float"),
            ),
            member("u_vec3", GlslType::VEC3, 128 + shift, custom("u_vec3")),
            member("u_vec2", GlslType::VEC2, 144 + shift, custom("u_vec2")),
            member(
                "unknownThing",
                GlslType::FLOAT,
                152 + shift,
                UniformSource::Unset,
            ),
            member(
                "u_dropped",
                GlslType::VEC4,
                160 + shift,
                custom("u_dropped"),
            ),
        ],
    }
}

fn block_defs() -> Vec<CustomUniform> {
    vec![
        def(
            "u_float",
            GlslType::FLOAT,
            "frameTimeCounter * 2 + gbufferModelView.3.2",
        ),
        def("u_bool", GlslType::BOOL, "is_hurt && frameCounter > 10"),
        def("u_int", GlslType::INT, "frameCounter % 8"),
        def(
            "u_vec3",
            GlslType::VEC3,
            "sunPosition * eyeBrightness.y / 240",
        ),
        def(
            "u_vec2",
            GlslType::VEC2,
            "smooth(vec2(frameTimeCounter, 0), 1)",
        ),
        var("hidden", GlslType::FLOAT, "1"),
        def("u_unused", GlslType::FLOAT, "hidden"),
    ]
}

fn block_input_type(name: &str) -> Option<GlslType> {
    frame_layout(0)
        .members
        .iter()
        .find(|m| m.name == name && matches!(m.source, UniformSource::Builtin(_)))
        .map(|m| m.ty)
}

fn write_builtins(layout: &BlockLayout, block: &mut [u8], t: f32) {
    let vals: [(&str, Value); 6] = [
        ("frameTimeCounter", Value::Float(t)),
        ("frameCounter", Value::Int(13)),
        ("is_hurt", Value::Bool(true)),
        ("sunPosition", Value::Vec3([10.0, 20.0, 30.0])),
        ("eyeBrightness", Value::Vec2([0.0, 120.0])),
        ("gbufferModelView", Value::Mat4(matrix())),
    ];
    for (name, v) in vals {
        let m = layout.member(name).unwrap();
        write_value(m.ty, v, &mut block[m.offset as usize..]).unwrap();
    }
}

#[test]
fn evaluate_into_block_round_trip() {
    let (mut cu, d) = CustomUniforms::compile(&block_defs(), &block_input_type, &IndexMap::new());
    assert!(d.is_empty(), "{d:?}");
    let layout = frame_layout(0);
    let mut block = vec![0xEEu8; layout.size as usize];
    write_builtins(&layout, &mut block, 1.5);
    // Reference values through the map-based API, reading the same bytes.
    let (mut reference, _) =
        CustomUniforms::compile(&block_defs(), &block_input_type, &IndexMap::new());
    let expected = reference.evaluate(&BlockInputs::new(&layout, &block), 0.016);
    cu.evaluate_into_block(&layout, &mut block, 0.016);

    let read_in = |block: &[u8], name: &str| {
        let m = layout.member(name).unwrap();
        read_value(m.ty, &block[m.offset as usize..]).unwrap()
    };
    let read = |name: &str| read_in(&block, name);
    assert_eq!(read("u_float"), Value::Float(1.5 * 2.0 + 14.0));
    assert_eq!(
        read("u_float__int"),
        Value::Int(17),
        "converted to the member's type"
    );
    assert_eq!(read("u_bool"), Value::Bool(true));
    assert_eq!(&block[116..120], &[1, 0, 0, 0], "bool is a u32 1");
    assert_eq!(read("u_int"), Value::Int(5));
    assert_eq!(read("u_vec3"), Value::Vec3([5.0, 10.0, 15.0]));
    assert_eq!(read("u_vec2"), Value::Vec2([1.5, 0.0]));
    for (name, value) in &expected {
        if layout.member(name).is_some() {
            assert_eq!(read(name), *value, "{name}");
        }
    }
    // Unset/unknown members and builtins are untouched.
    assert_eq!(&block[152..156], &[0xEE; 4]);
    assert_eq!(&block[160..176], &[0xEE; 16]);
    assert_eq!(read("frameTimeCounter"), Value::Float(1.5));

    // Next frame: smoothing advances with the frame time.
    write_builtins(&layout, &mut block, 2.5);
    cu.evaluate_into_block(&layout, &mut block, 0.1);
    let Value::Vec2([sx, _]) = read_in(&block, "u_vec2") else {
        panic!()
    };
    close(sx, 2.0);
}

#[test]
fn evaluate_into_block_follows_layout_changes() {
    let (mut cu, _) = CustomUniforms::compile(&block_defs(), &block_input_type, &IndexMap::new());
    let a = frame_layout(0);
    let mut block = vec![0u8; a.size as usize];
    write_builtins(&a, &mut block, 1.0);
    cu.evaluate_into_block(&a, &mut block, 0.0);
    // Same members shifted by 16 bytes: the cached plan must be rebuilt.
    let b = frame_layout(16);
    let mut block_b = vec![0u8; b.size as usize];
    write_builtins(&b, &mut block_b, 1.0);
    cu.evaluate_into_block(&b, &mut block_b, 0.0);
    let m = b.member("u_int").unwrap();
    assert_eq!(
        read_value(m.ty, &block_b[m.offset as usize..]).unwrap(),
        Value::Int(5)
    );
    let mv = b.member("gbufferModelView").unwrap();
    assert_eq!(
        read_value(mv.ty, &block_b[mv.offset as usize..]).unwrap(),
        Value::Mat4(matrix()),
        "builtins untouched"
    );
    // A block that is too short is handled without panicking.
    let mut short = vec![0u8; 20];
    cu.evaluate_into_block(&b, &mut short, 0.0);
}

#[test]
fn block_inputs_prefer_the_member_of_the_builtin_type() {
    // `frameTimeCounter__int` (a pack re-declaration with another type) comes first,
    // but the float member is the one the float input must be read from.
    let layout = BlockLayout {
        name: "sb_Frame".into(),
        set: 0,
        binding: 0,
        size: 16,
        members: vec![
            member(
                "frameTimeCounter__int",
                GlslType::INT,
                0,
                builtin("frameTimeCounter"),
            ),
            member(
                "frameTimeCounter",
                GlslType::FLOAT,
                4,
                builtin("frameTimeCounter"),
            ),
            member("out", GlslType::FLOAT, 8, custom("out")),
        ],
    };
    let (mut cu, d) = compile(&[def("out", GlslType::FLOAT, "frameTimeCounter * 2")]);
    assert!(d.is_empty(), "{d:?}");
    let mut block = vec![0u8; 16];
    write_value(GlslType::INT, Value::Int(1), &mut block[0..]).unwrap();
    write_value(GlslType::FLOAT, Value::Float(1.75), &mut block[4..]).unwrap();
    cu.evaluate_into_block(&layout, &mut block, 0.0);
    assert_eq!(
        read_value(GlslType::FLOAT, &block[8..]),
        Ok(Value::Float(3.5))
    );
    // Without a member of the right type, the other one is converted.
    let mut only_int = layout.clone();
    only_int.members.remove(1);
    cu.evaluate_into_block(&only_int, &mut block, 0.0);
    assert_eq!(
        read_value(GlslType::FLOAT, &block[8..]),
        Ok(Value::Float(2.0))
    );
}

#[test]
fn wide_layouts_build_plans_in_linear_time() {
    // Regression: the member plan and check_block scanned the layout once per
    // input/output.
    let n = 20_000;
    let defs: Vec<CustomUniform> = (0..n)
        .map(|k| def(&format!("o{k}"), GlslType::FLOAT, &format!("in{k} * 2")))
        .collect();
    let (mut cu, d) = CustomUniforms::compile(
        &defs,
        &|name: &str| name.starts_with("in").then_some(GlslType::FLOAT),
        &IndexMap::new(),
    );
    assert!(d.is_empty());
    let mut members = Vec::new();
    for k in 0..n {
        let k32 = k as u32;
        members.push(member(
            &format!("in{k}"),
            GlslType::FLOAT,
            k32 * 8,
            builtin(&format!("in{k}")),
        ));
        members.push(member(
            &format!("o{k}"),
            GlslType::FLOAT,
            k32 * 8 + 4,
            custom(&format!("o{k}")),
        ));
    }
    let layout = BlockLayout {
        name: "sb_Frame".into(),
        set: 0,
        binding: 0,
        size: n as u32 * 8,
        members,
    };
    let mut block = vec![0u8; layout.size as usize];
    for k in 0..n {
        write_value(GlslType::FLOAT, Value::Float(k as f32), &mut block[k * 8..]).unwrap();
    }
    let started = std::time::Instant::now();
    cu.evaluate_into_block(&layout, &mut block, 0.0);
    assert!(cu.check_block(&layout).is_empty());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
    let k = n - 1;
    assert_eq!(
        read_value(GlslType::FLOAT, &block[k * 8 + 4..]),
        Ok(Value::Float(2.0 * k as f32))
    );
}

#[test]
fn check_block_reports_layout_problems() {
    let (cu, _) = CustomUniforms::compile(&block_defs(), &block_input_type, &IndexMap::new());
    let mut layout = frame_layout(0);
    let d = cu.check_block(&layout);
    let codes: Vec<&str> = d.iter().map(|x| x.code.as_str()).collect();
    assert_eq!(
        codes,
        vec!["expr.block-unused-output", "expr.block-unknown-custom"],
        "{d:?}"
    );
    layout.members.retain(|m| m.name != "sunPosition");
    layout.members[0].offset = 400;
    let d = cu.check_block(&layout);
    assert!(
        d.iter()
            .any(|x| x.code == "expr.block-missing-input" && x.message.contains("sunPosition"))
    );
    assert!(
        d.iter()
            .any(|x| x.code == "expr.block-member" && x.message.contains("frameTimeCounter"))
    );
}

#[test]
fn block_inputs_reads_builtins() {
    let layout = frame_layout(0);
    let mut block = vec![0u8; layout.size as usize];
    write_builtins(&layout, &mut block, 4.0);
    let inputs = BlockInputs::new(&layout, &block);
    assert_eq!(inputs.get("frameTimeCounter"), Some(Value::Float(4.0)));
    assert_eq!(inputs.get("eyeBrightness"), Some(Value::Vec2([0.0, 120.0])));
    assert_eq!(inputs.get("u_float"), None, "custom members are not inputs");
    assert_eq!(inputs.get("nope"), None);
    assert_eq!(
        BlockInputs::new(&layout, &block[..8]).get("sunPosition"),
        None
    );
}

#[test]
fn maximum_depth_compiles_and_evaluates() {
    // Every nesting kind at the limit must work on a default (2 MiB) test thread in
    // debug builds; one level more must fail gracefully.
    let d = MAX_DEPTH - 2;
    let cases = [
        format!("{}x{}", "(".repeat(d), ")".repeat(d)),
        vec!["x"; d].join(" + "),
        format!("{}x", "-".repeat(d - 1)),
        format!("{}x{}", "abs(".repeat(d - 1), ")".repeat(d - 1)),
        format!("{}x{}", "smooth(".repeat(d - 1), ")".repeat(d - 1)),
        format!("{}x{}", "if(true, ".repeat(d - 1), ", 0)".repeat(d - 1)),
        // Call + member access: two tree levels per nesting level.
        format!(
            "{}x{}",
            "vec2(".repeat((d - 1) / 2),
            ", 0).x".repeat((d - 1) / 2)
        ),
    ];
    for src in &cases {
        let (mut cu, diags) = compile(&[def("u", GlslType::FLOAT, src)]);
        assert!(diags.is_empty(), "{diags:?}");
        let out = cu.evaluate(&|_: &str| Some(Value::Float(1.0)), 0.016);
        assert_eq!(out.len(), 1);
    }
    let too_deep = [
        format!(
            "{}x{}",
            "(".repeat(MAX_DEPTH + 1),
            ")".repeat(MAX_DEPTH + 1)
        ),
        vec!["x"; MAX_DEPTH + 2].join(" + "),
        format!(
            "{}x{}",
            "abs(".repeat(MAX_DEPTH + 1),
            ")".repeat(MAX_DEPTH + 1)
        ),
    ];
    for src in &too_deep {
        assert_eq!(err(src, GlslType::FLOAT), "expr.too-complex");
    }
}
