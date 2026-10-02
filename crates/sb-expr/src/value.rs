//! Runtime values and static types of the expression language.

use sb_core::{GlslType, ScalarKind};
use std::fmt;

/// A value produced or consumed by the expression language.
///
/// Integer and boolean vectors (`ivec2`, `bvec3`, ...) are represented as float
/// vectors; matrices of any size are represented as a column-major `mat4`
/// (`Mat4[c * 4 + r]` is GLSL `m[c][r]`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// `float`
    Float(f32),
    /// `int` (also used for `uint` inputs, wrapping)
    Int(i32),
    /// `bool`
    Bool(bool),
    /// `vec2` (also `ivec2`/`uvec2`/`bvec2` inputs)
    Vec2([f32; 2]),
    /// `vec3` (also `ivec3`/`uvec3`/`bvec3` inputs)
    Vec3([f32; 3]),
    /// `vec4` (also `ivec4`/`uvec4`/`bvec4` inputs)
    Vec4([f32; 4]),
    /// Column-major 4x4 matrix (std140 / GLSL memory order).
    Mat4([f32; 16]),
}

/// The static type of an expression or value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueType {
    /// `float`
    Float,
    /// `int`
    Int,
    /// `bool`
    Bool,
    /// `vec2`
    Vec2,
    /// `vec3`
    Vec3,
    /// `vec4`
    Vec4,
    /// `mat4` (inputs only)
    Mat4,
}

impl ValueType {
    /// The zero value of this type (`false` for booleans, the zero matrix for `Mat4`).
    pub const fn zero(self) -> Value {
        match self {
            ValueType::Float => Value::Float(0.0),
            ValueType::Int => Value::Int(0),
            ValueType::Bool => Value::Bool(false),
            ValueType::Vec2 => Value::Vec2([0.0; 2]),
            ValueType::Vec3 => Value::Vec3([0.0; 3]),
            ValueType::Vec4 => Value::Vec4([0.0; 4]),
            ValueType::Mat4 => Value::Mat4([0.0; 16]),
        }
    }

    /// Map the GLSL type of a builtin *input* uniform to the type it has inside
    /// expressions. This is lenient: `uint` becomes `Int`, `double` becomes `Float`,
    /// integer/boolean vectors become float vectors and every matrix becomes `Mat4`.
    /// Arrays are not supported (`None`).
    pub fn from_glsl(ty: GlslType) -> Option<Self> {
        if ty.array.is_some() {
            return None;
        }
        if ty.cols > 1 {
            return (2..=4).contains(&ty.cols).then_some(ValueType::Mat4);
        }
        match ty.rows {
            1 => Some(match ty.scalar {
                ScalarKind::Float | ScalarKind::Double => ValueType::Float,
                ScalarKind::Int | ScalarKind::Uint => ValueType::Int,
                ScalarKind::Bool => ValueType::Bool,
            }),
            2 => Some(ValueType::Vec2),
            3 => Some(ValueType::Vec3),
            4 => Some(ValueType::Vec4),
            _ => None,
        }
    }

    /// Map the declared type of a custom uniform (`uniform.<type>.<name>`) to a value
    /// type. Only `float`, `int`, `bool`, `vec2`, `vec3` and `vec4` are allowed, as in
    /// OptiFine and Iris.
    pub fn from_declared(ty: GlslType) -> Option<Self> {
        [
            (GlslType::FLOAT, ValueType::Float),
            (GlslType::INT, ValueType::Int),
            (GlslType::BOOL, ValueType::Bool),
            (GlslType::VEC2, ValueType::Vec2),
            (GlslType::VEC3, ValueType::Vec3),
            (GlslType::VEC4, ValueType::Vec4),
        ]
        .into_iter()
        .find_map(|(g, v)| (g == ty).then_some(v))
    }

    /// The canonical GLSL type of this value type.
    pub const fn glsl(self) -> GlslType {
        match self {
            ValueType::Float => GlslType::FLOAT,
            ValueType::Int => GlslType::INT,
            ValueType::Bool => GlslType::BOOL,
            ValueType::Vec2 => GlslType::VEC2,
            ValueType::Vec3 => GlslType::VEC3,
            ValueType::Vec4 => GlslType::VEC4,
            ValueType::Mat4 => GlslType::MAT4,
        }
    }

    /// Number of components of a float vector type (`None` for non-vectors).
    pub const fn vector_len(self) -> Option<usize> {
        match self {
            ValueType::Vec2 => Some(2),
            ValueType::Vec3 => Some(3),
            ValueType::Vec4 => Some(4),
            _ => None,
        }
    }

    /// The float vector type with `n` components (`n` in `2..=4`).
    pub const fn vector(n: usize) -> Option<Self> {
        match n {
            2 => Some(ValueType::Vec2),
            3 => Some(ValueType::Vec3),
            4 => Some(ValueType::Vec4),
            _ => None,
        }
    }

    /// `true` for `Float` and `Int`.
    pub const fn is_numeric_scalar(self) -> bool {
        matches!(self, ValueType::Float | ValueType::Int)
    }

    /// The GLSL spelling of the type.
    pub const fn name(self) -> &'static str {
        match self {
            ValueType::Float => "float",
            ValueType::Int => "int",
            ValueType::Bool => "bool",
            ValueType::Vec2 => "vec2",
            ValueType::Vec3 => "vec3",
            ValueType::Vec4 => "vec4",
            ValueType::Mat4 => "mat4",
        }
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Value {
    /// The type of this value.
    pub const fn ty(&self) -> ValueType {
        match self {
            Value::Float(_) => ValueType::Float,
            Value::Int(_) => ValueType::Int,
            Value::Bool(_) => ValueType::Bool,
            Value::Vec2(_) => ValueType::Vec2,
            Value::Vec3(_) => ValueType::Vec3,
            Value::Vec4(_) => ValueType::Vec4,
            Value::Mat4(_) => ValueType::Mat4,
        }
    }

    /// The value as a float. Vectors and matrices yield their first component,
    /// booleans yield `0.0`/`1.0`.
    pub fn to_f32(&self) -> f32 {
        match *self {
            Value::Float(f) => f,
            Value::Int(i) => i as f32,
            Value::Bool(b) => f32::from(u8::from(b)),
            Value::Vec2(v) => v[0],
            Value::Vec3(v) => v[0],
            Value::Vec4(v) => v[0],
            Value::Mat4(m) => m[0],
        }
    }

    /// The value as an integer. Floats are truncated toward zero (saturating, NaN
    /// becomes 0, like a Java `(int)` cast); vectors use their first component.
    pub fn to_i32(&self) -> i32 {
        match *self {
            Value::Int(i) => i,
            Value::Bool(b) => i32::from(b),
            other => other.to_f32() as i32,
        }
    }

    /// The value as a boolean: numbers are `true` when non-zero; vectors and
    /// matrices use their first component.
    pub fn to_bool(&self) -> bool {
        match *self {
            Value::Bool(b) => b,
            Value::Int(i) => i != 0,
            other => other.to_f32() != 0.0,
        }
    }

    /// Convert to `to`, following GLSL constructor rules where possible. This is total
    /// (never fails): scalars splat into vectors, longer vectors are truncated, shorter
    /// ones are zero-extended, numbers become booleans when non-zero, booleans become
    /// 0/1, a scalar becomes a diagonal matrix and a matrix yields its first elements.
    pub fn convert(self, to: ValueType) -> Value {
        if self.ty() == to {
            return self;
        }
        match to {
            ValueType::Float => Value::Float(self.to_f32()),
            ValueType::Int => Value::Int(self.to_i32()),
            ValueType::Bool => Value::Bool(self.to_bool()),
            ValueType::Vec2 | ValueType::Vec3 | ValueType::Vec4 => {
                let n = to.vector_len().unwrap_or(4);
                let l = match self {
                    Value::Mat4(m) => [m[0], m[1], m[2], m[3]],
                    _ => {
                        let (l, len) = lanes(&self);
                        if len == 1 {
                            [l[0]; 4]
                        } else {
                            let mut out = [0.0; 4];
                            out[..len].copy_from_slice(&l[..len]);
                            out
                        }
                    }
                };
                from_lanes(l, n)
            }
            ValueType::Mat4 => {
                let mut m = [0.0; 16];
                if matches!(self, Value::Float(_) | Value::Int(_) | Value::Bool(_)) {
                    let d = self.to_f32();
                    for i in 0..4 {
                        m[i * 5] = d;
                    }
                }
                Value::Mat4(m)
            }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn list(f: &mut fmt::Formatter<'_>, name: &str, v: &[f32]) -> fmt::Result {
            write!(f, "{name}(")?;
            for (i, x) in v.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{x:?}")?;
            }
            f.write_str(")")
        }
        match self {
            Value::Float(x) => write!(f, "{x:?}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Vec2(v) => list(f, "vec2", v),
            Value::Vec3(v) => list(f, "vec3", v),
            Value::Vec4(v) => list(f, "vec4", v),
            Value::Mat4(m) => list(f, "mat4", m),
        }
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<[f32; 2]> for Value {
    fn from(v: [f32; 2]) -> Self {
        Value::Vec2(v)
    }
}
impl From<[f32; 3]> for Value {
    fn from(v: [f32; 3]) -> Self {
        Value::Vec3(v)
    }
}
impl From<[f32; 4]> for Value {
    fn from(v: [f32; 4]) -> Self {
        Value::Vec4(v)
    }
}
impl From<[f32; 16]> for Value {
    fn from(v: [f32; 16]) -> Self {
        Value::Mat4(v)
    }
}

/// The value as up to four float lanes plus the lane count (1 for scalars).
/// Matrices yield their first column.
pub(crate) fn lanes(v: &Value) -> ([f32; 4], usize) {
    match *v {
        Value::Vec2([x, y]) => ([x, y, 0.0, 0.0], 2),
        Value::Vec3([x, y, z]) => ([x, y, z, 0.0], 3),
        Value::Vec4(l) => (l, 4),
        Value::Mat4(m) => ([m[0], m[1], m[2], m[3]], 4),
        other => ([other.to_f32(), 0.0, 0.0, 0.0], 1),
    }
}

/// Build a float scalar (`n == 1`) or vector from lanes.
pub(crate) fn from_lanes(l: [f32; 4], n: usize) -> Value {
    match n {
        2 => Value::Vec2([l[0], l[1]]),
        3 => Value::Vec3([l[0], l[1], l[2]]),
        4 => Value::Vec4(l),
        _ => Value::Float(l[0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(Value::Int(3).convert(ValueType::Float), Value::Float(3.0));
        assert_eq!(Value::Float(-2.7).convert(ValueType::Int), Value::Int(-2));
        assert_eq!(
            Value::Float(f32::NAN).convert(ValueType::Int),
            Value::Int(0)
        );
        assert_eq!(
            Value::Float(1e20).convert(ValueType::Int),
            Value::Int(i32::MAX)
        );
        assert_eq!(
            Value::Float(0.5).convert(ValueType::Bool),
            Value::Bool(true)
        );
        assert_eq!(Value::Int(0).convert(ValueType::Bool), Value::Bool(false));
        assert_eq!(
            Value::Bool(true).convert(ValueType::Float),
            Value::Float(1.0)
        );
        assert_eq!(
            Value::Float(2.0).convert(ValueType::Vec3),
            Value::Vec3([2.0; 3])
        );
        assert_eq!(
            Value::Vec4([1.0, 2.0, 3.0, 4.0]).convert(ValueType::Vec2),
            Value::Vec2([1.0, 2.0])
        );
        assert_eq!(
            Value::Vec2([1.0, 2.0]).convert(ValueType::Vec4),
            Value::Vec4([1.0, 2.0, 0.0, 0.0])
        );
        let Value::Mat4(m) = Value::Float(2.0).convert(ValueType::Mat4) else {
            panic!()
        };
        assert_eq!((m[0], m[5], m[10], m[15], m[1]), (2.0, 2.0, 2.0, 2.0, 0.0));
        assert_eq!(
            Value::Mat4([7.0; 16]).convert(ValueType::Float),
            Value::Float(7.0)
        );
    }

    #[test]
    fn glsl_mapping() {
        assert_eq!(ValueType::from_glsl(GlslType::IVEC2), Some(ValueType::Vec2));
        assert_eq!(ValueType::from_glsl(GlslType::UINT), Some(ValueType::Int));
        assert_eq!(ValueType::from_glsl(GlslType::MAT3), Some(ValueType::Mat4));
        assert_eq!(
            ValueType::from_glsl(GlslType::scalar(ScalarKind::Double)),
            Some(ValueType::Float)
        );
        assert_eq!(ValueType::from_glsl(GlslType::FLOAT.with_array(2)), None);
        assert_eq!(
            ValueType::from_declared(GlslType::VEC3),
            Some(ValueType::Vec3)
        );
        assert_eq!(ValueType::from_declared(GlslType::MAT4), None);
        assert_eq!(ValueType::from_declared(GlslType::IVEC2), None);
        for t in [
            ValueType::Float,
            ValueType::Int,
            ValueType::Bool,
            ValueType::Vec2,
            ValueType::Vec3,
            ValueType::Vec4,
            ValueType::Mat4,
        ] {
            assert_eq!(ValueType::from_glsl(t.glsl()), Some(t));
            assert_eq!(t.zero().ty(), t);
        }
    }

    #[test]
    fn display() {
        assert_eq!(Value::Float(1.0).to_string(), "1.0");
        assert_eq!(Value::Int(-4).to_string(), "-4");
        assert_eq!(Value::Vec2([0.5, 2.0]).to_string(), "vec2(0.5, 2.0)");
    }
}
