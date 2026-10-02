//! GLSL value types used for uniforms and stage interfaces, with std140 rules.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScalarKind {
    Float,
    Int,
    Uint,
    Bool,
    Double,
}

/// A non-opaque GLSL type: scalar, vector or (float) matrix, optionally an array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GlslType {
    pub scalar: ScalarKind,
    /// Number of rows / vector components (1..=4).
    pub rows: u8,
    /// Number of columns (1 for scalars and vectors, 2..=4 for matrices).
    pub cols: u8,
    /// Array length (`None` = not an array).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub array: Option<u32>,
}

impl GlslType {
    pub const fn scalar(s: ScalarKind) -> Self {
        Self { scalar: s, rows: 1, cols: 1, array: None }
    }
    pub const fn vector(s: ScalarKind, n: u8) -> Self {
        Self { scalar: s, rows: n, cols: 1, array: None }
    }
    pub const fn matrix(cols: u8, rows: u8) -> Self {
        Self { scalar: ScalarKind::Float, rows, cols, array: None }
    }
    pub const FLOAT: Self = Self::scalar(ScalarKind::Float);
    pub const INT: Self = Self::scalar(ScalarKind::Int);
    pub const UINT: Self = Self::scalar(ScalarKind::Uint);
    pub const BOOL: Self = Self::scalar(ScalarKind::Bool);
    pub const VEC2: Self = Self::vector(ScalarKind::Float, 2);
    pub const VEC3: Self = Self::vector(ScalarKind::Float, 3);
    pub const VEC4: Self = Self::vector(ScalarKind::Float, 4);
    pub const IVEC2: Self = Self::vector(ScalarKind::Int, 2);
    pub const IVEC3: Self = Self::vector(ScalarKind::Int, 3);
    pub const IVEC4: Self = Self::vector(ScalarKind::Int, 4);
    pub const UVEC2: Self = Self::vector(ScalarKind::Uint, 2);
    pub const UVEC3: Self = Self::vector(ScalarKind::Uint, 3);
    pub const UVEC4: Self = Self::vector(ScalarKind::Uint, 4);
    pub const BVEC2: Self = Self::vector(ScalarKind::Bool, 2);
    pub const BVEC3: Self = Self::vector(ScalarKind::Bool, 3);
    pub const BVEC4: Self = Self::vector(ScalarKind::Bool, 4);
    pub const MAT2: Self = Self::matrix(2, 2);
    pub const MAT3: Self = Self::matrix(3, 3);
    pub const MAT4: Self = Self::matrix(4, 4);

    pub const fn with_array(mut self, len: u32) -> Self {
        self.array = Some(len);
        self
    }
    pub const fn element(mut self) -> Self {
        self.array = None;
        self
    }
    pub fn is_matrix(&self) -> bool {
        self.cols > 1
    }
    pub fn is_scalar(&self) -> bool {
        self.rows == 1 && self.cols == 1
    }

    /// Parse a GLSL type name (`float`, `ivec3`, `mat4`, `mat3x4`, `bvec2`, `uint`, `double`, `dvec3`).
    pub fn parse(name: &str) -> Option<Self> {
        use ScalarKind::*;
        let simple = match name {
            "float" => Some(Self::FLOAT),
            "int" => Some(Self::INT),
            "uint" => Some(Self::UINT),
            "bool" => Some(Self::BOOL),
            "double" => Some(Self::scalar(Double)),
            _ => None,
        };
        if simple.is_some() {
            return simple;
        }
        let (kind, rest) = if let Some(r) = name.strip_prefix("vec") {
            (Float, r)
        } else if let Some(r) = name.strip_prefix("ivec") {
            (Int, r)
        } else if let Some(r) = name.strip_prefix("uvec") {
            (Uint, r)
        } else if let Some(r) = name.strip_prefix("bvec") {
            (Bool, r)
        } else if let Some(r) = name.strip_prefix("dvec") {
            (Double, r)
        } else if let Some(r) = name.strip_prefix("dmat") {
            return parse_mat(r).map(|(c, rw)| Self { scalar: Double, rows: rw, cols: c, array: None });
        } else {
            let r = name.strip_prefix("mat")?;
            return parse_mat(r).map(|(c, rw)| Self::matrix(c, rw));
        };
        match rest {
            "2" => Some(Self::vector(kind, 2)),
            "3" => Some(Self::vector(kind, 3)),
            "4" => Some(Self::vector(kind, 4)),
            _ => None,
        }
    }

    /// GLSL spelling of the element type (without array suffix).
    pub fn glsl_name(&self) -> String {
        use ScalarKind::*;
        if self.cols > 1 {
            let p = if self.scalar == Double { "dmat" } else { "mat" };
            return if self.cols == self.rows {
                format!("{p}{}", self.cols)
            } else {
                format!("{p}{}x{}", self.cols, self.rows)
            };
        }
        if self.rows == 1 {
            return match self.scalar {
                Float => "float",
                Int => "int",
                Uint => "uint",
                Bool => "bool",
                Double => "double",
            }
            .to_string();
        }
        let p = match self.scalar {
            Float => "vec",
            Int => "ivec",
            Uint => "uvec",
            Bool => "bvec",
            Double => "dvec",
        };
        format!("{p}{}", self.rows)
    }

    fn scalar_size(&self) -> u32 {
        if self.scalar == ScalarKind::Double { 8 } else { 4 }
    }

    /// std140 base alignment of one element (not considering the array).
    fn std140_element_align(&self) -> u32 {
        let n = self.scalar_size();
        if self.cols > 1 {
            // matrix = array of column vectors, each rounded up to vec4
            return round_up(vec_align(n, self.rows), 16);
        }
        vec_align(n, self.rows)
    }

    /// std140 base alignment, taking arrays into account.
    pub fn std140_align(&self) -> u32 {
        let a = self.std140_element_align();
        if self.array.is_some() { round_up(a, 16) } else { a }
    }

    /// std140 size in bytes (for arrays: stride * length). Saturates at `u32::MAX` for
    /// absurd array lengths from malformed input.
    pub fn std140_size(&self) -> u32 {
        let n = self.scalar_size();
        let elem = if self.cols > 1 {
            let col_stride = round_up(vec_align(n, self.rows), 16);
            col_stride * self.cols as u32
        } else {
            n * self.rows as u32
        };
        match self.array {
            Some(len) => self.std140_array_stride().saturating_mul(len.max(1)),
            None => elem,
        }
    }

    /// std140 array stride (only meaningful for arrays).
    pub fn std140_array_stride(&self) -> u32 {
        let n = self.scalar_size();
        let elem = if self.cols > 1 {
            round_up(vec_align(n, self.rows), 16) * self.cols as u32
        } else {
            n * self.rows as u32
        };
        round_up(elem.max(self.std140_element_align()), 16)
    }
}

fn parse_mat(r: &str) -> Option<(u8, u8)> {
    let digit = |s: &str| -> Option<u8> {
        match s {
            "2" => Some(2),
            "3" => Some(3),
            "4" => Some(4),
            _ => None,
        }
    };
    if let Some((c, r2)) = r.split_once('x') {
        Some((digit(c)?, digit(r2)?))
    } else {
        let d = digit(r)?;
        Some((d, d))
    }
}

fn vec_align(scalar: u32, n: u8) -> u32 {
    match n {
        1 => scalar,
        2 => 2 * scalar,
        _ => 4 * scalar,
    }
}

pub(crate) fn round_up(v: u32, a: u32) -> u32 {
    v.div_ceil(a) * a
}

impl std::fmt::Display for GlslType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.glsl_name())?;
        if let Some(n) = self.array {
            write!(f, "[{n}]")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_print_roundtrip() {
        for n in ["float", "int", "uint", "bool", "vec2", "vec3", "vec4", "ivec2", "uvec4", "bvec3", "mat2", "mat3", "mat4", "mat3x4", "mat4x2", "double", "dvec3"] {
            let t = GlslType::parse(n).unwrap_or_else(|| panic!("parse {n}"));
            assert_eq!(t.glsl_name(), n);
        }
        assert!(GlslType::parse("sampler2D").is_none());
        assert!(GlslType::parse("vec5").is_none());
    }

    #[test]
    fn std140_sizes() {
        assert_eq!(GlslType::FLOAT.std140_size(), 4);
        assert_eq!(GlslType::VEC2.std140_align(), 8);
        assert_eq!(GlslType::VEC3.std140_align(), 16);
        assert_eq!(GlslType::VEC3.std140_size(), 12);
        assert_eq!(GlslType::VEC4.std140_size(), 16);
        assert_eq!(GlslType::MAT4.std140_size(), 64);
        assert_eq!(GlslType::MAT3.std140_size(), 48);
        assert_eq!(GlslType::MAT3.std140_align(), 16);
        assert_eq!(GlslType::FLOAT.with_array(4).std140_size(), 64);
        assert_eq!(GlslType::FLOAT.with_array(4).std140_align(), 16);
        assert_eq!(GlslType::VEC3.with_array(2).std140_array_stride(), 16);
        assert_eq!(GlslType::MAT4.with_array(2).std140_size(), 128);
        assert_eq!(GlslType::MAT4.with_array(u32::MAX).std140_size(), u32::MAX);
    }
}
