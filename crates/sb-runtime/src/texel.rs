//! Texel encoding and decoding for the image formats the runtime creates: every
//! `TextureFormat::vk_format_renderable` format, `D32_SFLOAT`, and the GL pixel transfer
//! formats of raw custom textures (`RED`/`RG`/`RGB`/`RGBA`/`BGR(A)` × `UNSIGNED_BYTE` …
//! `FLOAT`).

use ash::vk;

/// How the components of a format are interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Unorm,
    Snorm,
    Uint,
    Sint,
    Float,
}

/// Shader-visible numeric class of a format (what `sampler*` / `isampler*` /
/// `usampler*` it may be bound to).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum NumericClass {
    Float,
    Int,
    Uint,
}

/// Memory layout of a texel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// `comps` components of `bits` bits each, in RGBA order.
    Plain { comps: u8, bits: u8, kind: Kind },
    /// `A2B10G10R10` packed in a little-endian u32 (R in the low bits).
    A2B10G10R10 { uint: bool },
    /// `B10G11R11_UFLOAT` packed in a u32 (R in the low 11 bits).
    B10G11R11,
    /// 32-bit float depth.
    D32,
}

pub(crate) fn layout(format: vk::Format) -> Option<Layout> {
    use Kind::*;
    let plain = |comps, bits, kind| Some(Layout::Plain { comps, bits, kind });
    match format.as_raw() {
        9 => plain(1, 8, Unorm),
        10 => plain(1, 8, Snorm),
        13 => plain(1, 8, Uint),
        14 => plain(1, 8, Sint),
        16 => plain(2, 8, Unorm),
        17 => plain(2, 8, Snorm),
        20 => plain(2, 8, Uint),
        21 => plain(2, 8, Sint),
        37 => plain(4, 8, Unorm),
        38 => plain(4, 8, Snorm),
        41 => plain(4, 8, Uint),
        42 => plain(4, 8, Sint),
        64 => Some(Layout::A2B10G10R10 { uint: false }),
        68 => Some(Layout::A2B10G10R10 { uint: true }),
        70 => plain(1, 16, Unorm),
        71 => plain(1, 16, Snorm),
        74 => plain(1, 16, Uint),
        75 => plain(1, 16, Sint),
        76 => plain(1, 16, Float),
        77 => plain(2, 16, Unorm),
        78 => plain(2, 16, Snorm),
        81 => plain(2, 16, Uint),
        82 => plain(2, 16, Sint),
        83 => plain(2, 16, Float),
        91 => plain(4, 16, Unorm),
        92 => plain(4, 16, Snorm),
        95 => plain(4, 16, Uint),
        96 => plain(4, 16, Sint),
        97 => plain(4, 16, Float),
        98 => plain(1, 32, Uint),
        99 => plain(1, 32, Sint),
        100 => plain(1, 32, Float),
        101 => plain(2, 32, Uint),
        102 => plain(2, 32, Sint),
        103 => plain(2, 32, Float),
        107 => plain(4, 32, Uint),
        108 => plain(4, 32, Sint),
        109 => plain(4, 32, Float),
        122 => Some(Layout::B10G11R11),
        126 => Some(Layout::D32),
        _ => None,
    }
}

/// Bytes per texel, or `None` for an unsupported format.
pub(crate) fn texel_size(format: vk::Format) -> Option<usize> {
    Some(match layout(format)? {
        Layout::Plain { comps, bits, .. } => usize::from(comps) * usize::from(bits / 8),
        Layout::A2B10G10R10 { .. } | Layout::B10G11R11 | Layout::D32 => 4,
    })
}

/// Number of components the format stores.
pub(crate) fn component_count(format: vk::Format) -> Option<u8> {
    Some(match layout(format)? {
        Layout::Plain { comps, .. } => comps,
        Layout::A2B10G10R10 { .. } => 4,
        Layout::B10G11R11 => 3,
        Layout::D32 => 1,
    })
}

/// The shader-visible numeric class of a format (unknown formats count as float).
pub(crate) fn numeric_class(format: vk::Format) -> NumericClass {
    match layout(format) {
        Some(Layout::Plain { kind: Kind::Uint, .. }) | Some(Layout::A2B10G10R10 { uint: true }) => NumericClass::Uint,
        Some(Layout::Plain { kind: Kind::Sint, .. }) => NumericClass::Int,
        _ => NumericClass::Float,
    }
}

/// A 4-component format of the given class with 32-bit components (fallback textures).
pub(crate) fn rgba32_of(class: NumericClass) -> vk::Format {
    match class {
        NumericClass::Float => vk::Format::R32G32B32A32_SFLOAT,
        NumericClass::Int => vk::Format::R32G32B32A32_SINT,
        NumericClass::Uint => vk::Format::R32G32B32A32_UINT,
    }
}

fn read_u(bytes: &[u8], bits: u8) -> u64 {
    match bits {
        8 => u64::from(bytes[0]),
        16 => u64::from(u16::from_le_bytes([bytes[0], bytes[1]])),
        _ => u64::from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
    }
}

fn write_u(out: &mut [u8], bits: u8, v: u64) {
    match bits {
        8 => out[0] = v as u8,
        16 => out[..2].copy_from_slice(&(v as u16).to_le_bytes()),
        _ => out[..4].copy_from_slice(&(v as u32).to_le_bytes()),
    }
}

fn decode_component(raw: u64, bits: u8, kind: Kind) -> f64 {
    let max_u = ((1u64 << bits) - 1) as f64;
    let sign_extend = |v: u64| -> i64 {
        let shift = 64 - u32::from(bits);
        ((v << shift) as i64) >> shift
    };
    match kind {
        Kind::Unorm => raw as f64 / max_u,
        Kind::Snorm => {
            let max_s = ((1u64 << (bits - 1)) - 1) as f64;
            (sign_extend(raw) as f64 / max_s).max(-1.0)
        }
        Kind::Uint => raw as f64,
        Kind::Sint => sign_extend(raw) as f64,
        Kind::Float => match bits {
            16 => f64::from(f16_to_f32(raw as u16)),
            32 => f64::from(f32::from_bits(raw as u32)),
            _ => 0.0,
        },
    }
}

fn encode_component(v: f64, bits: u8, kind: Kind) -> u64 {
    let mask = if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 };
    let v = if v.is_nan() { 0.0 } else { v };
    match kind {
        Kind::Unorm => (v.clamp(0.0, 1.0) * mask as f64).round() as u64,
        Kind::Snorm => {
            let max_s = ((1u64 << (bits - 1)) - 1) as f64;
            ((v.clamp(-1.0, 1.0) * max_s).round() as i64 as u64) & mask
        }
        Kind::Uint => v.round().clamp(0.0, mask as f64) as u64,
        Kind::Sint => {
            let lim = (1i64 << (bits - 1)) as f64;
            (v.round().clamp(-lim, lim - 1.0) as i64 as u64) & mask
        }
        Kind::Float => match bits {
            16 => u64::from(f32_to_f16(v as f32)),
            _ => u64::from((v as f32).to_bits()),
        },
    }
}

/// Decode one texel into RGBA (missing components read as 0, alpha as 1). Returns
/// `None` for unsupported formats or a too-short slice.
pub(crate) fn decode(format: vk::Format, bytes: &[u8]) -> Option<[f64; 4]> {
    let size = texel_size(format)?;
    let bytes = bytes.get(..size)?;
    let mut out = [0.0, 0.0, 0.0, 1.0];
    match layout(format)? {
        Layout::Plain { comps, bits, kind } => {
            let step = usize::from(bits / 8);
            for (i, o) in out.iter_mut().enumerate().take(usize::from(comps)) {
                *o = decode_component(read_u(&bytes[i * step..], bits), bits, kind);
            }
        }
        Layout::A2B10G10R10 { uint } => {
            let w = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let parts = [w & 0x3ff, (w >> 10) & 0x3ff, (w >> 20) & 0x3ff, w >> 30];
            let max = [1023.0, 1023.0, 1023.0, 3.0];
            for i in 0..4 {
                out[i] = if uint { f64::from(parts[i]) } else { f64::from(parts[i]) / max[i] };
            }
        }
        Layout::B10G11R11 => {
            let w = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            out[0] = ufloat_to_f64(w & 0x7ff, 6);
            out[1] = ufloat_to_f64((w >> 11) & 0x7ff, 6);
            out[2] = ufloat_to_f64((w >> 22) & 0x3ff, 5);
        }
        Layout::D32 => out[0] = f64::from(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
    }
    Some(out)
}

/// Encode RGBA into one texel. Returns `false` for unsupported formats or a too-short
/// slice (which is then left untouched).
pub(crate) fn encode(format: vk::Format, v: [f64; 4], out: &mut [u8]) -> bool {
    let Some(size) = texel_size(format) else { return false };
    let Some(out) = out.get_mut(..size) else { return false };
    let Some(layout) = layout(format) else { return false };
    match layout {
        Layout::Plain { comps, bits, kind } => {
            let step = usize::from(bits / 8);
            for (i, value) in v.iter().enumerate().take(usize::from(comps)) {
                write_u(&mut out[i * step..], bits, encode_component(*value, bits, kind));
            }
        }
        Layout::A2B10G10R10 { uint } => {
            let comp = |x: f64, max: f64| -> u32 {
                let x = if x.is_nan() { 0.0 } else { x };
                if uint { x.round().clamp(0.0, max) as u32 } else { (x.clamp(0.0, 1.0) * max).round() as u32 }
            };
            let w = comp(v[0], 1023.0) | (comp(v[1], 1023.0) << 10) | (comp(v[2], 1023.0) << 20) | (comp(v[3], 3.0) << 30);
            out.copy_from_slice(&w.to_le_bytes());
        }
        Layout::B10G11R11 => {
            let w = f64_to_ufloat(v[0], 6) | (f64_to_ufloat(v[1], 6) << 11) | (f64_to_ufloat(v[2], 5) << 22);
            out.copy_from_slice(&w.to_le_bytes());
        }
        Layout::D32 => out.copy_from_slice(&(v[0] as f32).to_le_bytes()),
    }
    true
}

/// IEEE half → f32.
pub(crate) fn f16_to_f32(h: u16) -> f32 {
    let sign = u32::from(h >> 15) << 31;
    let exp = u32::from((h >> 10) & 0x1f);
    let mant = u32::from(h & 0x3ff);
    let bits = match (exp, mant) {
        (0, 0) => sign,
        (0, m) => {
            // Subnormal: normalize.
            let mut e = 127 - 15 + 1;
            let mut m = m;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            sign | (e << 23) | ((m & 0x3ff) << 13)
        }
        (0x1f, 0) => sign | 0x7f80_0000,
        (0x1f, m) => sign | 0x7fc0_0000 | (m << 13),
        (e, m) => sign | ((e + 127 - 15) << 23) | (m << 13),
    };
    f32::from_bits(bits)
}

/// f32 → IEEE half (round to nearest even, overflow to infinity).
pub(crate) fn f32_to_f16(f: f32) -> u16 {
    let bits = f.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if mant != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = mant | 0x80_0000;
        let shift = (14 - e) as u32;
        let half = 1u32 << (shift - 1);
        let rem = m & ((1 << shift) - 1);
        let mut out = m >> shift;
        if rem > half || (rem == half && out & 1 == 1) {
            out += 1;
        }
        return sign | out as u16;
    }
    let mut out = ((e as u32) << 10) | (mant >> 13);
    let rem = mant & 0x1fff;
    if rem > 0x1000 || (rem == 0x1000 && out & 1 == 1) {
        out += 1;
    }
    sign | out as u16
}

/// Unsigned small float (5-bit exponent, `mbits` mantissa) → f64.
fn ufloat_to_f64(v: u32, mbits: u32) -> f64 {
    let e = (v >> mbits) & 0x1f;
    let m = f64::from(v & ((1 << mbits) - 1));
    let scale = f64::from(1u32 << mbits);
    match e {
        0 => m / scale * 2f64.powi(-14),
        31 => {
            if m == 0.0 {
                f64::INFINITY
            } else {
                f64::NAN
            }
        }
        _ => 2f64.powi(e as i32 - 15) * (1.0 + m / scale),
    }
}

/// f64 → unsigned small float (5-bit exponent, `mbits` mantissa), clamping negatives
/// and NaN to 0 and large values to the largest finite value.
fn f64_to_ufloat(v: f64, mbits: u32) -> u32 {
    if v.is_nan() || v <= 0.0 {
        return 0;
    }
    let max_mant = (1u32 << mbits) - 1;
    let max_val = 2f64.powi(15) * (1.0 + f64::from(max_mant) / f64::from(1u32 << mbits));
    if v >= max_val {
        return (30 << mbits) | max_mant;
    }
    let min_normal = 2f64.powi(-14);
    if v < min_normal {
        let m = (v / min_normal * f64::from(1u32 << mbits)).round() as u32;
        return m.min(max_mant + 1); // rounding up to the first normal is fine
    }
    let mut e = v.log2().floor() as i32;
    let mut m = ((v / 2f64.powi(e) - 1.0) * f64::from(1u32 << mbits)).round() as u32;
    if m > max_mant {
        m = 0;
        e += 1;
    }
    (((e + 15) as u32) << mbits) | m
}

/// A GL pixel transfer description of raw texture data (`pixel_format`, `pixel_type`
/// of `texture.<stage>.<name>=<file> TEXTURE_xD <internal> <size> <format> <type>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixelTransfer {
    /// Component order in memory: indices into RGBA.
    pub order: [usize; 4],
    pub comps: usize,
    /// `_INTEGER` formats are not normalized.
    pub integer: bool,
    pub ty: PixelType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PixelType {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    F16,
    F32,
}

impl PixelType {
    fn size(self) -> usize {
        match self {
            PixelType::U8 | PixelType::I8 => 1,
            PixelType::U16 | PixelType::I16 | PixelType::F16 => 2,
            PixelType::U32 | PixelType::I32 | PixelType::F32 => 4,
        }
    }
}

impl PixelTransfer {
    /// Parse GL names (case-insensitive, optional `GL_` prefix). `None` if unsupported.
    pub(crate) fn parse(format: &str, ty: &str) -> Option<Self> {
        let norm = |s: &str| {
            let s = s.trim().to_ascii_uppercase();
            s.strip_prefix("GL_").map(str::to_string).unwrap_or(s)
        };
        let format = norm(format);
        let (base, integer) = match format.strip_suffix("_INTEGER") {
            Some(b) => (b.to_string(), true),
            None => (format, false),
        };
        let (order, comps) = match base.as_str() {
            "RED" | "R" | "LUMINANCE" | "ALPHA" => ([0, 1, 2, 3], 1),
            "GREEN" => ([1, 0, 2, 3], 1),
            "BLUE" => ([2, 0, 1, 3], 1),
            "RG" | "LUMINANCE_ALPHA" => ([0, 1, 2, 3], 2),
            "RGB" => ([0, 1, 2, 3], 3),
            "BGR" => ([2, 1, 0, 3], 3),
            "RGBA" => ([0, 1, 2, 3], 4),
            "BGRA" => ([2, 1, 0, 3], 4),
            _ => return None,
        };
        let ty = match norm(ty).as_str() {
            "UNSIGNED_BYTE" => PixelType::U8,
            "BYTE" => PixelType::I8,
            "UNSIGNED_SHORT" => PixelType::U16,
            "SHORT" => PixelType::I16,
            "UNSIGNED_INT" => PixelType::U32,
            "INT" => PixelType::I32,
            "HALF_FLOAT" => PixelType::F16,
            "FLOAT" => PixelType::F32,
            _ => return None,
        };
        Some(Self { order, comps, integer, ty })
    }

    /// Bytes per pixel of the source data.
    pub(crate) fn pixel_size(&self) -> usize {
        self.comps * self.ty.size()
    }

    /// Decode one source pixel to RGBA (GL rules: missing G/B read 0, missing A reads 1;
    /// non-integer formats normalize integer types).
    pub(crate) fn decode(&self, bytes: &[u8]) -> [f64; 4] {
        let mut out = [0.0, 0.0, 0.0, 1.0];
        let s = self.ty.size();
        for i in 0..self.comps {
            let Some(b) = bytes.get(i * s..(i + 1) * s) else { break };
            let v = match self.ty {
                PixelType::U8 => decode_component(u64::from(b[0]), 8, if self.integer { Kind::Uint } else { Kind::Unorm }),
                PixelType::I8 => decode_component(u64::from(b[0]), 8, if self.integer { Kind::Sint } else { Kind::Snorm }),
                PixelType::U16 => decode_component(read_u(b, 16), 16, if self.integer { Kind::Uint } else { Kind::Unorm }),
                PixelType::I16 => decode_component(read_u(b, 16), 16, if self.integer { Kind::Sint } else { Kind::Snorm }),
                PixelType::U32 => decode_component(read_u(b, 32), 32, if self.integer { Kind::Uint } else { Kind::Unorm }),
                PixelType::I32 => decode_component(read_u(b, 32), 32, if self.integer { Kind::Sint } else { Kind::Snorm }),
                PixelType::F16 => decode_component(read_u(b, 16), 16, Kind::Float),
                PixelType::F32 => decode_component(read_u(b, 32), 32, Kind::Float),
            };
            if let Some(slot) = out.get_mut(self.order[i]) {
                *slot = v;
            }
        }
        out
    }
}

/// Convert `texels` source pixels into `format` (tightly packed). Missing source data
/// reads as zero.
pub(crate) fn convert(src: &[u8], transfer: &PixelTransfer, texels: usize, format: vk::Format) -> Option<Vec<u8>> {
    let dst_size = texel_size(format)?;
    let src_size = transfer.pixel_size();
    let mut out = vec![0u8; texels.checked_mul(dst_size)?];
    for i in 0..texels {
        let v = match src.get(i * src_size..(i + 1) * src_size) {
            Some(b) => transfer.decode(b),
            None => [0.0, 0.0, 0.0, 1.0],
        };
        encode(format, v, &mut out[i * dst_size..(i + 1) * dst_size]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(format: vk::Format, v: [f64; 4], eps: f64) {
        let mut buf = vec![0u8; texel_size(format).unwrap()];
        assert!(encode(format, v, &mut buf));
        let back = decode(format, &buf).unwrap();
        let comps = usize::from(component_count(format).unwrap());
        for i in 0..comps {
            assert!((back[i] - v[i]).abs() <= eps, "{format:?} comp {i}: {} vs {}", back[i], v[i]);
        }
    }

    #[test]
    fn every_renderable_pack_format_roundtrips() {
        for f in sb_core::TextureFormat::ALL {
            let vk_format = vk::Format::from_raw(f.vk_format_renderable() as i32);
            assert!(layout(vk_format).is_some(), "{f}: {vk_format:?} has no texel layout");
            let v = match numeric_class(vk_format) {
                NumericClass::Float => match f.component_kind() {
                    sb_core::format::ComponentKind::Snorm => [-0.5, 0.25, 1.0, -1.0],
                    _ => [0.5, 0.25, 1.0, 0.0],
                },
                NumericClass::Uint => [3.0, 1.0, 2.0, 0.0],
                NumericClass::Int => [-3.0, 1.0, 2.0, 0.0],
            };
            let eps = match layout(vk_format).unwrap() {
                Layout::B10G11R11 => 0.02,
                Layout::A2B10G10R10 { .. } => 0.001,
                Layout::Plain { bits: 8, .. } => 1.0 / 127.0,
                Layout::Plain { bits: 16, .. } => 1e-3,
                _ => 1e-6,
            };
            roundtrip(vk_format, v, eps);
        }
        assert_eq!(texel_size(vk::Format::D32_SFLOAT), Some(4));
        roundtrip(vk::Format::D32_SFLOAT, [0.75, 0.0, 0.0, 1.0], 0.0);
    }

    #[test]
    fn half_floats() {
        for v in [0.0f32, 1.0, -2.5, 0.000_06, 65504.0, 1.0e-7, 3.140_625] {
            let h = f32_to_f16(v);
            let back = f16_to_f32(h);
            let tol = (v.abs() * 1e-3).max(1e-7);
            assert!((back - v).abs() <= tol, "{v} -> {h:#x} -> {back}");
        }
        assert!(f16_to_f32(f32_to_f16(1.0e6)).is_infinite());
        assert!(f16_to_f32(f32_to_f16(f32::NAN)).is_nan());
        assert_eq!(f32_to_f16(1.0), 0x3c00);
    }

    #[test]
    fn packed_formats() {
        let mut buf = [0u8; 4];
        encode(vk::Format::A2B10G10R10_UNORM_PACK32, [1.0, 0.0, 0.0, 1.0], &mut buf);
        assert_eq!(u32::from_le_bytes(buf), 0x3ff | (3 << 30));
        encode(vk::Format::B10G11R11_UFLOAT_PACK32, [1.0, 0.5, 2.0, 1.0], &mut buf);
        let d = decode(vk::Format::B10G11R11_UFLOAT_PACK32, &buf).unwrap();
        assert_eq!(&d[..3], &[1.0, 0.5, 2.0]);
        encode(vk::Format::B10G11R11_UFLOAT_PACK32, [-1.0, 1e9, f64::NAN, 1.0], &mut buf);
        let d = decode(vk::Format::B10G11R11_UFLOAT_PACK32, &buf).unwrap();
        assert_eq!(d[0], 0.0);
        assert!(d[1] > 60000.0 && d[1].is_finite());
    }

    #[test]
    fn integer_clamping_and_snorm() {
        let mut b = [0u8; 1];
        encode(vk::Format::R8_UINT, [300.0, 0.0, 0.0, 0.0], &mut b);
        assert_eq!(b[0], 255);
        encode(vk::Format::R8_SINT, [-300.0, 0.0, 0.0, 0.0], &mut b);
        assert_eq!(b[0] as i8, -128);
        encode(vk::Format::R8_SNORM, [-1.0, 0.0, 0.0, 0.0], &mut b);
        assert_eq!(b[0] as i8, -127);
        assert_eq!(decode(vk::Format::R8_SNORM, &[0x80]).unwrap()[0], -1.0);
        assert!(decode(vk::Format::R8_UNORM, &[]).is_none());
        assert!(!encode(vk::Format::UNDEFINED, [0.0; 4], &mut b));
    }

    #[test]
    fn pixel_transfer_conversion() {
        // RGB UNSIGNED_BYTE into RGBA8: alpha defaults to 1.
        let t = PixelTransfer::parse("RGB", "UNSIGNED_BYTE").unwrap();
        let out = convert(&[255, 0, 128, 10, 20, 30], &t, 2, vk::Format::R8G8B8A8_UNORM).unwrap();
        assert_eq!(out, vec![255, 0, 128, 255, 10, 20, 30, 255]);
        // RG HALF_FLOAT into RGBA16F.
        let t = PixelTransfer::parse("gl_rg", "half_float").unwrap();
        let src: Vec<u8> = [f32_to_f16(0.5), f32_to_f16(2.0)].iter().flat_map(|h| h.to_le_bytes()).collect();
        let out = convert(&src, &t, 1, vk::Format::R16G16B16A16_SFLOAT).unwrap();
        let d = decode(vk::Format::R16G16B16A16_SFLOAT, &out).unwrap();
        assert_eq!(d, [0.5, 2.0, 0.0, 1.0]);
        // BGRA swizzles; RED_INTEGER keeps integers.
        let t = PixelTransfer::parse("BGRA", "UNSIGNED_BYTE").unwrap();
        assert_eq!(t.decode(&[0, 0, 255, 255])[0], 1.0);
        let t = PixelTransfer::parse("RED_INTEGER", "UNSIGNED_SHORT").unwrap();
        assert_eq!(t.decode(&[0x10, 0x27])[0], 10000.0);
        // Short source data reads zero instead of panicking.
        let t = PixelTransfer::parse("RGBA", "FLOAT").unwrap();
        let out = convert(&[0, 0], &t, 3, vk::Format::R32_SFLOAT).unwrap();
        assert_eq!(out.len(), 12);
        assert!(PixelTransfer::parse("YUV", "FLOAT").is_none());
        assert!(PixelTransfer::parse("RGBA", "DOUBLE").is_none());
    }
}
