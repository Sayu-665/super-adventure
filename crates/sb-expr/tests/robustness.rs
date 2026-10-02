//! Malformed input must produce errors, never panics: deterministic fuzzing of the
//! parser, compiler and evaluator, the std140 helpers and block evaluation.

use indexmap::IndexMap;
use sb_core::model::{BlockLayout, BlockMember, CustomUniform, UniformSource};
use sb_core::{GlslType, ScalarKind};
use sb_expr::{
    CustomUniforms, Expr, ExprKind, Value, eval_bool, parse, read_value, standard_constants,
    write_value,
};

/// xorshift64* — deterministic and dependency-free.
struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

const TOKENS: &[&str] = &[
    "x",
    "v",
    "m",
    "b",
    "i",
    "pi",
    "true",
    "false",
    "nope",
    "BIOME_PLAINS",
    "CAT_ICY",
    "0",
    "1",
    "2.5",
    ".5",
    "2.",
    "1e-3",
    "1e",
    "0x1F",
    "0x",
    "0b10",
    "010",
    "08",
    "99999999999",
    "1e999",
    "0xFFFFFFFF",
    "0x80000000",
    "2147483648",
    "(",
    ")",
    "(",
    ")",
    ",",
    ".",
    ".x",
    ".xyz",
    ".0",
    ".3",
    ".7",
    ".q",
    "+",
    "-",
    "*",
    "/",
    "%",
    "<",
    ">",
    "<=",
    ">=",
    "==",
    "!=",
    "≠",
    "≤",
    "≥",
    "&&",
    "||",
    "!",
    "&",
    "|",
    "=",
    ";",
    "#",
    "\\",
    "é",
    "\u{0}",
    " ",
    "\t",
    "\n",
    "sin(",
    "atan(",
    "log(",
    "min(",
    "max(",
    "clamp(",
    "mix(",
    "lerp(",
    "edge(",
    "fmod(",
    "random(",
    "randomInt(",
    "if(",
    "ifb(",
    "smooth(",
    "between(",
    "equals(",
    "in(",
    "vec2(",
    "vec3(",
    "vec4(",
    "ivec3(",
    "bvec2(",
    "print(",
    "pow(",
    "abs(",
    "floor(",
    "round(",
];

const SEEDS: &[&str] = &[
    "if(isEyeInWater == 0, 1.0 - smooth(202, if(eyeAltitude < 5.0, eyeBrightness.y / 240.0, 1.0), 6, 12), 0.0)",
    "vec3(gbufferModelViewInverse.0.0 * sunPosition.x + gbufferModelViewInverse.1.0 * sunPosition.y, 1, 2)",
    "smooth(if(in(biome, BIOME_PLAINS, 3), 1, 0), 15, 15) * clamp(x, 0, 1)",
    "frac(1.3247179572 * frameCounter + 0.5) * 2.0 - 1.0",
    "max(blindness, darknessFactor * 0.125 + darknessLightFactor, 3)",
    "(BLOOM || SSAO) && !GODRAYS",
];

fn input_type(name: &str) -> Option<GlslType> {
    Some(match name {
        "v" | "sunPosition" => GlslType::VEC3,
        "eyeBrightness" => GlslType::IVEC2,
        "m" | "gbufferModelViewInverse" => GlslType::MAT4,
        "b" => GlslType::BOOL,
        "i" | "frameCounter" | "biome" | "isEyeInWater" => GlslType::INT,
        "nope" => return None,
        "arr" => GlslType::FLOAT.with_array(3),
        _ => GlslType::FLOAT,
    })
}

/// Structural equality of two trees, ignoring source spans.
fn same_shape(a: &Expr, b: &Expr) -> bool {
    match (&a.kind, &b.kind) {
        (ExprKind::Float(x), ExprKind::Float(y)) => x.to_bits() == y.to_bits(),
        (
            ExprKind::Unary {
                op: o1,
                operand: a1,
            },
            ExprKind::Unary {
                op: o2,
                operand: a2,
            },
        ) => o1 == o2 && same_shape(a1, a2),
        (
            ExprKind::Binary {
                op: o1,
                lhs: l1,
                rhs: r1,
            },
            ExprKind::Binary {
                op: o2,
                lhs: l2,
                rhs: r2,
            },
        ) => o1 == o2 && same_shape(l1, l2) && same_shape(r1, r2),
        (
            ExprKind::Call {
                name: n1, args: a1, ..
            },
            ExprKind::Call {
                name: n2, args: a2, ..
            },
        ) => n1 == n2 && a1.len() == a2.len() && a1.iter().zip(a2).all(|(x, y)| same_shape(x, y)),
        (
            ExprKind::Member {
                base: b1,
                member: m1,
                ..
            },
            ExprKind::Member {
                base: b2,
                member: m2,
                ..
            },
        ) => m1 == m2 && same_shape(b1, b2),
        (x, y) => x == y,
    }
}

fn exercise(src: &str, constants: &IndexMap<String, Value>) {
    // Whatever parses prints as text that parses back to the same tree.
    if let Ok(e) = parse(src) {
        let printed = e.to_string();
        let again = parse(&printed)
            .unwrap_or_else(|err| panic!("{src:?} printed as {printed:?} does not parse: {err}"));
        assert!(
            same_shape(&e, &again),
            "{src:?} printed as {printed:?} parses differently"
        );
    }
    let _ = eval_bool(src, &|n| Some(n.len() % 2 == 0));
    for ty in [
        GlslType::FLOAT,
        GlslType::INT,
        GlslType::BOOL,
        GlslType::VEC2,
        GlslType::VEC4,
    ] {
        let defs = vec![
            CustomUniform {
                name: "a".into(),
                ty,
                expression: src.into(),
                is_variable: false,
                location: None,
            },
            CustomUniform {
                name: "c".into(),
                ty: GlslType::FLOAT,
                expression: "a".into(),
                is_variable: true,
                location: None,
            },
        ];
        let (mut cu, _) = CustomUniforms::compile(&defs, &input_type, constants);
        for dt in [0.016, 0.0, f32::NAN, -1.0, 1e30] {
            let _ = cu.evaluate(&|n: &str| input_type(n).map(|_| Value::Float(0.75)), dt);
        }
    }
}

#[test]
fn random_token_soup_never_panics() {
    let constants = standard_constants();
    let mut g = Gen(0x9E37_79B9_7F4A_7C15);
    for _ in 0..3000 {
        let len = 1 + g.below(14);
        let src: String = (0..len).map(|_| *g.pick(TOKENS)).collect();
        exercise(&src, &constants);
    }
}

#[test]
fn mutated_real_expressions_never_panic() {
    let constants = standard_constants();
    let mut g = Gen(42);
    for _ in 0..3000 {
        let mut s: Vec<char> = g.pick(SEEDS).chars().collect();
        for _ in 0..1 + g.below(4) {
            let pos = g.below(s.len() + 1);
            match g.below(3) {
                0 if pos < s.len() => {
                    s.remove(pos);
                }
                1 => {
                    for c in g.pick(TOKENS).chars().rev() {
                        s.insert(pos, c);
                    }
                }
                _ if pos + 1 < s.len() => s.swap(pos, pos + 1),
                _ => {}
            }
        }
        let src: String = s.into_iter().collect();
        exercise(&src, &constants);
    }
}

#[test]
fn pathological_sizes() {
    let constants = standard_constants();
    for src in [
        "(".repeat(100_000),
        ")".repeat(100_000),
        "-".repeat(100_000),
        "!".repeat(100_000) + "x",
        "x".repeat(100_000),
        "1".repeat(100_000),
        ".".repeat(1000),
        format!("max({})", vec!["x"; 20_000].join(",")),
        format!("if({}1)", "true, 1, ".repeat(10_000)),
        vec!["x"; 20_000].join("+"),
        "v.x".to_string() + &".x".repeat(10_000),
        "smooth(".repeat(10_000),
    ] {
        exercise(&src, &constants);
    }
}

fn random_type(g: &mut Gen) -> GlslType {
    let scalar = *g.pick(&[
        ScalarKind::Float,
        ScalarKind::Int,
        ScalarKind::Uint,
        ScalarKind::Bool,
        ScalarKind::Double,
    ]);
    let rows = g.below(6) as u8;
    let cols = g.below(6) as u8;
    let array = if g.below(8) == 0 {
        Some(g.below(4) as u32)
    } else {
        None
    };
    GlslType {
        scalar,
        rows,
        cols,
        array,
    }
}

fn random_value(g: &mut Gen) -> Value {
    let x = (g.next() % 2000) as f32 / 7.0 - 100.0;
    match g.below(7) {
        0 => Value::Float(if g.below(10) == 0 { f32::NAN } else { x }),
        1 => Value::Int(g.next() as i32),
        2 => Value::Bool(g.below(2) == 0),
        3 => Value::Vec2([x, -x]),
        4 => Value::Vec3([x, 1e30, f32::INFINITY]),
        5 => Value::Vec4([x; 4]),
        _ => Value::Mat4([x; 16]),
    }
}

#[test]
fn std140_helpers_never_panic() {
    let mut g = Gen(7);
    for _ in 0..20_000 {
        let ty = random_type(&mut g);
        let mut bytes = vec![0x5Au8; g.below(80)];
        let v = random_value(&mut g);
        let w = write_value(ty, v, &mut bytes);
        let r = read_value(ty, &bytes);
        // A successful write of a well-formed type can always be read back.
        if w.is_ok() {
            assert!(r.is_ok(), "{ty:?}");
        }
    }
}

#[test]
fn block_evaluation_with_hostile_layouts_never_panics() {
    let defs = vec![
        CustomUniform {
            name: "a".into(),
            ty: GlslType::FLOAT,
            expression: "x * 2 + v.y".into(),
            is_variable: false,
            location: None,
        },
        CustomUniform {
            name: "bv".into(),
            ty: GlslType::VEC3,
            expression: "smooth(v, 1)".into(),
            is_variable: false,
            location: None,
        },
        CustomUniform {
            name: "flag".into(),
            ty: GlslType::BOOL,
            expression: "b || i > 3".into(),
            is_variable: false,
            location: None,
        },
        CustomUniform {
            name: "mm".into(),
            ty: GlslType::VEC4,
            expression: "m.2".into(),
            is_variable: false,
            location: None,
        },
    ];
    let (mut cu, d) = CustomUniforms::compile(&defs, &input_type, &IndexMap::new());
    assert!(!d.has_errors(), "{d:?}");
    let names = ["x", "v", "b", "i", "m", "a", "bv", "flag", "mm", "zz"];
    let mut g = Gen(1234);
    for _ in 0..3000 {
        let members: Vec<BlockMember> = (0..g.below(12))
            .map(|_| {
                let name = g.pick(&names).to_string();
                let source = match g.below(3) {
                    0 => UniformSource::Builtin(name.clone()),
                    1 => UniformSource::Custom(name.clone()),
                    _ => UniformSource::Unset,
                };
                let offset = if g.below(10) == 0 {
                    u32::MAX - g.below(8) as u32
                } else {
                    (g.below(40) * 4) as u32
                };
                BlockMember {
                    name,
                    ty: random_type(&mut g),
                    offset,
                    source,
                    default: None,
                }
            })
            .collect();
        let layout = BlockLayout {
            name: "sb_Frame".into(),
            set: 0,
            binding: 0,
            size: g.below(256) as u32,
            members,
        };
        let mut block: Vec<u8> = (0..g.below(200)).map(|_| g.next() as u8).collect();
        cu.evaluate_into_block(&layout, &mut block, 0.016);
        cu.evaluate_into_block(&layout, &mut block, f32::NAN);
        let _ = cu.check_block(&layout);
    }
}
