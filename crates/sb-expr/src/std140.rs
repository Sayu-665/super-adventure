//! Reading and writing [`Value`]s in std140 uniform-buffer memory (little-endian).
//!
//! These helpers are shared with `sb-runtime` and the JNI layer. `bytes` always
//! starts at the member's offset. Booleans are 32-bit `0`/`1`; matrices are
//! column-major with a 16-byte column stride (32 bytes for `dmat3`/`dmat4`).
//! Arrays are not supported.

use crate::value::{Value, ValueType, lanes};
use sb_core::{GlslType, ScalarKind};

/// Errors of [`read_value`] and [`write_value`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Std140Error {
    /// The byte slice is too short for the type.
    #[error("a std140 {ty} needs {needed} bytes, but only {available} are available")]
    OutOfBounds {
        /// The requested type.
        ty: GlslType,
        /// Bytes the type occupies.
        needed: usize,
        /// Bytes available in the slice.
        available: usize,
    },
    /// Arrays and malformed types (e.g. 5 rows) are not supported.
    #[error("type {0} is not supported by the std140 value helpers")]
    Unsupported(GlslType),
}

fn scalar_size(kind: ScalarKind) -> usize {
    if kind == ScalarKind::Double { 8 } else { 4 }
}

/// Validate `ty` and return (column stride, size in bytes).
fn layout(ty: GlslType, available: usize) -> Result<(usize, usize), Std140Error> {
    if ty.array.is_some()
        || !(1..=4).contains(&ty.rows)
        || !(1..=4).contains(&ty.cols)
        || (ty.cols > 1 && ty.rows < 2)
    {
        return Err(Std140Error::Unsupported(ty));
    }
    let s = scalar_size(ty.scalar);
    let col_stride = if ty.cols > 1 {
        let vec_align = if ty.rows == 2 { 2 * s } else { 4 * s };
        vec_align.div_ceil(16) * 16
    } else {
        0
    };
    let needed = ty.std140_size() as usize;
    if available < needed {
        return Err(Std140Error::OutOfBounds {
            ty,
            needed,
            available,
        });
    }
    Ok((col_stride, needed))
}

fn read_scalar(kind: ScalarKind, b: &[u8]) -> f64 {
    let w4 = |b: &[u8]| [b[0], b[1], b[2], b[3]];
    match kind {
        ScalarKind::Float => f64::from(f32::from_le_bytes(w4(b))),
        ScalarKind::Int => f64::from(i32::from_le_bytes(w4(b))),
        ScalarKind::Uint => f64::from(u32::from_le_bytes(w4(b))),
        ScalarKind::Bool => f64::from(u8::from(u32::from_le_bytes(w4(b)) != 0)),
        ScalarKind::Double => f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
    }
}

/// Read a value of GLSL type `ty` from std140 bytes.
///
/// Scalars map to `Float`/`Int`/`Bool` (`uint` wraps into `Int`, `double` narrows
/// to `Float`); vectors of any scalar kind map to float vectors; matrices of any
/// size map to `Mat4`, embedded in the identity like GLSL `mat4(m)`.
pub fn read_value(ty: GlslType, bytes: &[u8]) -> Result<Value, Std140Error> {
    let (col_stride, _) = layout(ty, bytes.len())?;
    let s = scalar_size(ty.scalar);
    if ty.cols > 1 {
        let mut m = [0.0f32; 16];
        for i in 0..4 {
            m[i * 5] = 1.0;
        }
        for c in 0..usize::from(ty.cols) {
            for r in 0..usize::from(ty.rows) {
                let off = c * col_stride + r * s;
                m[c * 4 + r] = read_scalar(ty.scalar, &bytes[off..]) as f32;
            }
        }
        return Ok(Value::Mat4(m));
    }
    if ty.rows == 1 {
        let b4 = [bytes[0], bytes[1], bytes[2], bytes[3]];
        return Ok(match ty.scalar {
            ScalarKind::Float => Value::Float(f32::from_le_bytes(b4)),
            ScalarKind::Int => Value::Int(i32::from_le_bytes(b4)),
            ScalarKind::Uint => Value::Int(u32::from_le_bytes(b4) as i32),
            ScalarKind::Bool => Value::Bool(u32::from_le_bytes(b4) != 0),
            ScalarKind::Double => Value::Float(read_scalar(ScalarKind::Double, bytes) as f32),
        });
    }
    let mut l = [0.0f32; 4];
    for (i, x) in l.iter_mut().enumerate().take(usize::from(ty.rows)) {
        *x = read_scalar(ty.scalar, &bytes[i * s..]) as f32;
    }
    Ok(crate::value::from_lanes(l, usize::from(ty.rows)))
}

/// Write `value` as GLSL type `ty` into std140 bytes, converting it like a GLSL
/// constructor would (see [`Value::convert`]): e.g. a `Float` written to an `int`
/// member is truncated, a scalar written to a vector is splatted, a `Mat4` written
/// to a `mat3` keeps its upper-left 3x3 block.
pub fn write_value(ty: GlslType, value: Value, bytes: &mut [u8]) -> Result<(), Std140Error> {
    let (col_stride, _) = layout(ty, bytes.len())?;
    let s = scalar_size(ty.scalar);
    if ty.cols > 1 {
        let Value::Mat4(m) = value.convert(ValueType::Mat4) else {
            return Err(Std140Error::Unsupported(ty));
        };
        for c in 0..usize::from(ty.cols) {
            for r in 0..usize::from(ty.rows) {
                let off = c * col_stride + r * s;
                write_f32(ty.scalar, m[c * 4 + r], &mut bytes[off..]);
            }
        }
        return Ok(());
    }
    if ty.rows == 1 {
        // Exact paths for integer and boolean sources.
        match (ty.scalar, value) {
            (ScalarKind::Int, Value::Int(i)) => bytes[..4].copy_from_slice(&i.to_le_bytes()),
            (ScalarKind::Uint, Value::Int(i)) => {
                bytes[..4].copy_from_slice(&(i as u32).to_le_bytes())
            }
            (ScalarKind::Int | ScalarKind::Uint | ScalarKind::Bool, Value::Bool(v)) => {
                bytes[..4].copy_from_slice(&u32::from(v).to_le_bytes());
            }
            (ScalarKind::Bool, v) => {
                bytes[..4].copy_from_slice(&u32::from(v.to_bool()).to_le_bytes())
            }
            (kind, v) => write_f32(kind, v.to_f32(), bytes),
        }
        return Ok(());
    }
    let n = usize::from(ty.rows);
    // Scalars splat; vectors (and a matrix's first column) are truncated or zero-extended.
    let (l, len) = match value {
        Value::Int(_) | Value::Bool(_) | Value::Float(_) => ([value.to_f32(); 4], 4),
        _ => lanes(&value),
    };
    for i in 0..n {
        let x = if i < len { l[i] } else { 0.0 };
        write_f32(ty.scalar, x, &mut bytes[i * s..]);
    }
    // Exact integer splat for large ints (f32 loses precision above 2^24).
    if let (Value::Int(v), ScalarKind::Int | ScalarKind::Uint) = (value, ty.scalar) {
        for i in 0..n {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    Ok(())
}

/// Encode one float lane as a scalar of `kind` (ints truncate, bools are `!= 0`).
fn write_f32(kind: ScalarKind, x: f32, b: &mut [u8]) {
    match kind {
        ScalarKind::Float => b[..4].copy_from_slice(&x.to_le_bytes()),
        ScalarKind::Int => b[..4].copy_from_slice(&(x as i32).to_le_bytes()),
        ScalarKind::Uint => b[..4].copy_from_slice(&(x as u32).to_le_bytes()),
        ScalarKind::Bool => b[..4].copy_from_slice(&u32::from(x != 0.0).to_le_bytes()),
        ScalarKind::Double => b[..8].copy_from_slice(&f64::from(x).to_le_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn roundtrip(ty: GlslType, v: Value) -> Value {
        let mut buf = vec![0xAAu8; ty.std140_size() as usize];
        write_value(ty, v, &mut buf).unwrap();
        read_value(ty, &buf).unwrap()
    }

    #[test]
    fn scalars() {
        assert_eq!(
            roundtrip(GlslType::FLOAT, Value::Float(1.5)),
            Value::Float(1.5)
        );
        assert_eq!(roundtrip(GlslType::INT, Value::Int(-7)), Value::Int(-7));
        assert_eq!(
            roundtrip(GlslType::INT, Value::Int(i32::MAX)),
            Value::Int(i32::MAX)
        );
        assert_eq!(roundtrip(GlslType::INT, Value::Float(2.9)), Value::Int(2));
        assert_eq!(roundtrip(GlslType::UINT, Value::Int(5)), Value::Int(5));
        assert_eq!(
            roundtrip(GlslType::BOOL, Value::Bool(true)),
            Value::Bool(true)
        );
        assert_eq!(
            roundtrip(GlslType::BOOL, Value::Float(0.0)),
            Value::Bool(false)
        );
        assert_eq!(
            roundtrip(GlslType::FLOAT, Value::Bool(true)),
            Value::Float(1.0)
        );
        assert_eq!(
            roundtrip(GlslType::scalar(ScalarKind::Double), Value::Float(0.25)),
            Value::Float(0.25)
        );
        let mut b = [0u8; 4];
        write_value(GlslType::BOOL, Value::Bool(true), &mut b).unwrap();
        assert_eq!(b, [1, 0, 0, 0]);
    }

    #[test]
    fn vectors() {
        assert_eq!(
            roundtrip(GlslType::VEC3, Value::Vec3([1.0, 2.0, 3.0])),
            Value::Vec3([1.0, 2.0, 3.0])
        );
        assert_eq!(
            roundtrip(GlslType::VEC4, Value::Float(2.0)),
            Value::Vec4([2.0; 4])
        );
        assert_eq!(
            roundtrip(GlslType::VEC2, Value::Vec4([1.0, 2.0, 3.0, 4.0])),
            Value::Vec2([1.0, 2.0])
        );
        assert_eq!(
            roundtrip(GlslType::IVEC2, Value::Vec2([240.0, -3.7])),
            Value::Vec2([240.0, -3.0])
        );
        assert_eq!(
            roundtrip(GlslType::BVEC3, Value::Vec3([0.0, 2.0, 0.0])),
            Value::Vec3([0.0, 1.0, 0.0])
        );
        let mut b = [0u8; 8];
        write_value(GlslType::IVEC2, Value::Int(16_777_217), &mut b).unwrap();
        assert_eq!(i32::from_le_bytes([b[4], b[5], b[6], b[7]]), 16_777_217);
        // vec3 needs 12 bytes, not 16.
        let mut b = [0u8; 12];
        write_value(GlslType::VEC3, Value::Vec3([1.0, 2.0, 3.0]), &mut b).unwrap();
        assert_eq!(f32::from_le_bytes([b[8], b[9], b[10], b[11]]), 3.0);
    }

    #[test]
    fn matrices() {
        let mut m = [0.0f32; 16];
        for (i, x) in m.iter_mut().enumerate() {
            *x = i as f32;
        }
        assert_eq!(roundtrip(GlslType::MAT4, Value::Mat4(m)), Value::Mat4(m));
        // mat3: column stride 16, upper-left block, identity elsewhere.
        let Value::Mat4(r) = roundtrip(GlslType::MAT3, Value::Mat4(m)) else {
            panic!()
        };
        assert_eq!(&r[0..4], &[0.0, 1.0, 2.0, 0.0]);
        assert_eq!(&r[4..8], &[4.0, 5.0, 6.0, 0.0]);
        assert_eq!(&r[8..12], &[8.0, 9.0, 10.0, 0.0]);
        assert_eq!(&r[12..16], &[0.0, 0.0, 0.0, 1.0]);
        let mut b = vec![0u8; 48];
        write_value(GlslType::MAT3, Value::Mat4(m), &mut b).unwrap();
        assert_eq!(f32::from_le_bytes([b[16], b[17], b[18], b[19]]), 4.0);
        // Raw column-major layout of a mat4.
        let mut b = vec![0u8; 64];
        write_value(GlslType::MAT4, Value::Mat4(m), &mut b).unwrap();
        assert_eq!(f32::from_le_bytes([b[36], b[37], b[38], b[39]]), 9.0);
        // dmat2 has a 16-byte column stride, dmat3 32.
        assert_eq!(
            roundtrip(
                GlslType {
                    scalar: ScalarKind::Double,
                    rows: 2,
                    cols: 2,
                    array: None
                },
                Value::Float(3.0)
            ),
            {
                let mut e = [0.0; 16];
                e[0] = 3.0;
                e[5] = 3.0;
                e[10] = 1.0;
                e[15] = 1.0;
                Value::Mat4(e)
            }
        );
    }

    #[test]
    fn errors() {
        let mut small = [0u8; 8];
        assert_eq!(
            write_value(GlslType::VEC3, Value::Float(1.0), &mut small),
            Err(Std140Error::OutOfBounds {
                ty: GlslType::VEC3,
                needed: 12,
                available: 8
            })
        );
        assert!(matches!(
            read_value(GlslType::MAT4, &small),
            Err(Std140Error::OutOfBounds { .. })
        ));
        assert!(matches!(
            read_value(GlslType::FLOAT.with_array(2), &[0u8; 64]),
            Err(Std140Error::Unsupported(_))
        ));
        let bogus = GlslType {
            scalar: ScalarKind::Float,
            rows: 7,
            cols: 1,
            array: None,
        };
        assert!(matches!(
            read_value(bogus, &[0u8; 64]),
            Err(Std140Error::Unsupported(_))
        ));
        assert!(matches!(
            read_value(GlslType::FLOAT, &[]),
            Err(Std140Error::OutOfBounds { .. })
        ));
    }
}
