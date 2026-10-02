//! Render-target / texture formats accepted by shader packs (Iris internal format
//! names) and their Vulkan equivalents.

use serde::{Deserialize, Serialize};

/// Component interpretation of a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentKind {
    Unorm,
    Snorm,
    Float,
    Uint,
    Sint,
}

macro_rules! formats {
    ($( $variant:ident = $name:literal, $vk:expr, $vk_rt:expr, $comps:expr, $kind:ident, $img:expr; )*) => {
        /// Texture formats in Iris/OptiFine spelling.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[allow(non_camel_case_types)]
        pub enum TextureFormat { $( #[serde(rename = $name)] $variant, )* }

        impl TextureFormat {
            pub const ALL: &'static [TextureFormat] = &[ $( TextureFormat::$variant, )* ];

            /// Canonical (upper-case) pack spelling, e.g. `RGBA16F`.
            pub fn name(self) -> &'static str { match self { $( Self::$variant => $name, )* } }

            /// Exact `VkFormat` value.
            pub fn vk_format(self) -> u32 { match self { $( Self::$variant => $vk, )* } }

            /// `VkFormat` to use for color attachments / storage: 3-component and other
            /// rarely-renderable formats are widened (e.g. RGB16F -> RGBA16F).
            pub fn vk_format_renderable(self) -> u32 { match self { $( Self::$variant => $vk_rt, )* } }

            /// Number of components the format stores.
            pub fn components(self) -> u8 { match self { $( Self::$variant => $comps, )* } }

            pub fn component_kind(self) -> ComponentKind { match self { $( Self::$variant => ComponentKind::$kind, )* } }

            /// GLSL storage-image format qualifier (`rgba16f`, ...) for the renderable format,
            /// or `None` if no qualifier exists.
            pub fn glsl_image_format(self) -> Option<&'static str> { match self { $( Self::$variant => $img, )* } }
        }
    };
}

// VkFormat constants used below (from the Vulkan spec):
// R4G4B4A4_UNORM_PACK16=2 R5G6B5_UNORM_PACK16=4 R5G5B5A1_UNORM_PACK16=6
// R8=9/10/13/14 (UNORM/SNORM/UINT/SINT) RG8=16/17/20/21 RGB8=23/24/27/28 RGBA8=37/38/41/42
// A2B10G10R10_UNORM=64 A2B10G10R10_UINT=68
// R16=70/71/74/75/76(SFLOAT) RG16=77/78/81/82/83 RGB16=84/85/88/89/90 RGBA16=91/92/95/96/97
// R32=98(UINT)/99(SINT)/100(SFLOAT) RG32=101/102/103 RGB32=104/105/106 RGBA32=107/108/109
// B10G11R11_UFLOAT=122 E5B9G9R9_UFLOAT=123
formats! {
    RGBA = "RGBA", 37, 37, 4, Unorm, Some("rgba8");
    R8 = "R8", 9, 9, 1, Unorm, Some("r8");
    RG8 = "RG8", 16, 16, 2, Unorm, Some("rg8");
    RGB8 = "RGB8", 23, 37, 3, Unorm, Some("rgba8");
    RGBA8 = "RGBA8", 37, 37, 4, Unorm, Some("rgba8");
    R8_SNORM = "R8_SNORM", 10, 10, 1, Snorm, Some("r8_snorm");
    RG8_SNORM = "RG8_SNORM", 17, 17, 2, Snorm, Some("rg8_snorm");
    RGB8_SNORM = "RGB8_SNORM", 24, 38, 3, Snorm, Some("rgba8_snorm");
    RGBA8_SNORM = "RGBA8_SNORM", 38, 38, 4, Snorm, Some("rgba8_snorm");
    R16 = "R16", 70, 70, 1, Unorm, Some("r16");
    RG16 = "RG16", 77, 77, 2, Unorm, Some("rg16");
    RGB16 = "RGB16", 84, 91, 3, Unorm, Some("rgba16");
    RGBA16 = "RGBA16", 91, 91, 4, Unorm, Some("rgba16");
    R16_SNORM = "R16_SNORM", 71, 71, 1, Snorm, Some("r16_snorm");
    RG16_SNORM = "RG16_SNORM", 78, 78, 2, Snorm, Some("rg16_snorm");
    RGB16_SNORM = "RGB16_SNORM", 85, 92, 3, Snorm, Some("rgba16_snorm");
    RGBA16_SNORM = "RGBA16_SNORM", 92, 92, 4, Snorm, Some("rgba16_snorm");
    R16F = "R16F", 76, 76, 1, Float, Some("r16f");
    RG16F = "RG16F", 83, 83, 2, Float, Some("rg16f");
    RGB16F = "RGB16F", 90, 97, 3, Float, Some("rgba16f");
    RGBA16F = "RGBA16F", 97, 97, 4, Float, Some("rgba16f");
    R32F = "R32F", 100, 100, 1, Float, Some("r32f");
    RG32F = "RG32F", 103, 103, 2, Float, Some("rg32f");
    RGB32F = "RGB32F", 106, 109, 3, Float, Some("rgba32f");
    RGBA32F = "RGBA32F", 109, 109, 4, Float, Some("rgba32f");
    R8I = "R8I", 14, 14, 1, Sint, Some("r8i");
    RG8I = "RG8I", 21, 21, 2, Sint, Some("rg8i");
    RGB8I = "RGB8I", 28, 42, 3, Sint, Some("rgba8i");
    RGBA8I = "RGBA8I", 42, 42, 4, Sint, Some("rgba8i");
    R8UI = "R8UI", 13, 13, 1, Uint, Some("r8ui");
    RG8UI = "RG8UI", 20, 20, 2, Uint, Some("rg8ui");
    RGB8UI = "RGB8UI", 27, 41, 3, Uint, Some("rgba8ui");
    RGBA8UI = "RGBA8UI", 41, 41, 4, Uint, Some("rgba8ui");
    R16I = "R16I", 75, 75, 1, Sint, Some("r16i");
    RG16I = "RG16I", 82, 82, 2, Sint, Some("rg16i");
    RGB16I = "RGB16I", 89, 96, 3, Sint, Some("rgba16i");
    RGBA16I = "RGBA16I", 96, 96, 4, Sint, Some("rgba16i");
    R16UI = "R16UI", 74, 74, 1, Uint, Some("r16ui");
    RG16UI = "RG16UI", 81, 81, 2, Uint, Some("rg16ui");
    RGB16UI = "RGB16UI", 88, 95, 3, Uint, Some("rgba16ui");
    RGBA16UI = "RGBA16UI", 95, 95, 4, Uint, Some("rgba16ui");
    R32I = "R32I", 99, 99, 1, Sint, Some("r32i");
    RG32I = "RG32I", 102, 102, 2, Sint, Some("rg32i");
    RGB32I = "RGB32I", 105, 108, 3, Sint, Some("rgba32i");
    RGBA32I = "RGBA32I", 108, 108, 4, Sint, Some("rgba32i");
    R32UI = "R32UI", 98, 98, 1, Uint, Some("r32ui");
    RG32UI = "RG32UI", 101, 101, 2, Uint, Some("rg32ui");
    RGB32UI = "RGB32UI", 104, 107, 3, Uint, Some("rgba32ui");
    RGBA32UI = "RGBA32UI", 107, 107, 4, Uint, Some("rgba32ui");
    RGBA2 = "RGBA2", 37, 37, 4, Unorm, Some("rgba8");
    RGBA4 = "RGBA4", 2, 37, 4, Unorm, Some("rgba8");
    R3_G3_B2 = "R3_G3_B2", 37, 37, 3, Unorm, Some("rgba8");
    RGB5_A1 = "RGB5_A1", 6, 37, 4, Unorm, Some("rgba8");
    RGB565 = "RGB565", 4, 37, 3, Unorm, Some("rgba8");
    RGB10_A2 = "RGB10_A2", 64, 64, 4, Unorm, Some("rgb10_a2");
    RGB10_A2UI = "RGB10_A2UI", 68, 68, 4, Uint, Some("rgb10_a2ui");
    R11F_G11F_B10F = "R11F_G11F_B10F", 122, 122, 3, Float, Some("r11f_g11f_b10f");
    RGB9_E5 = "RGB9_E5", 123, 97, 3, Float, Some("rgba16f");
}

impl TextureFormat {
    /// Parse a pack format name (case-insensitive). Accepts an optional `GL_` prefix.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s.strip_prefix("GL_").or_else(|| s.strip_prefix("gl_")).unwrap_or(s);
        let upper = s.to_ascii_uppercase();
        Self::ALL.iter().copied().find(|f| f.name() == upper)
    }

    /// Parse a GLSL image format qualifier (`rgba16f`) into the matching format.
    pub fn from_glsl_image_format(q: &str) -> Option<Self> {
        let q = q.trim().to_ascii_lowercase();
        // Prefer the canonical 4/2/1-component formats.
        Self::ALL.iter().copied().find(|f| f.glsl_image_format() == Some(q.as_str()) && f.vk_format() == f.vk_format_renderable())
    }

    /// True for integer formats (sampled with isampler*/usampler*).
    pub fn is_integer(self) -> bool {
        matches!(self.component_kind(), ComponentKind::Uint | ComponentKind::Sint)
    }
}

impl Default for TextureFormat {
    fn default() -> Self {
        TextureFormat::RGBA
    }
}

impl std::fmt::Display for TextureFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Depth formats used by hosts.
pub mod depth {
    pub const VK_FORMAT_D32_SFLOAT: u32 = 126;
    pub const VK_FORMAT_D24_UNORM_S8_UINT: u32 = 129;
    pub const VK_FORMAT_D16_UNORM: u32 = 124;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_names() {
        assert_eq!(TextureFormat::parse("rgba16f"), Some(TextureFormat::RGBA16F));
        assert_eq!(TextureFormat::parse("GL_R11F_G11F_B10F"), Some(TextureFormat::R11F_G11F_B10F));
        assert_eq!(TextureFormat::parse(" RGB10_A2 "), Some(TextureFormat::RGB10_A2));
        assert_eq!(TextureFormat::parse("nope"), None);
        assert_eq!(TextureFormat::ALL.len(), 58);
    }

    #[test]
    fn renderable_widening() {
        assert_eq!(TextureFormat::RGB16F.vk_format_renderable(), TextureFormat::RGBA16F.vk_format());
        assert_eq!(TextureFormat::RGBA16F.vk_format_renderable(), 97);
        assert!(TextureFormat::RGBA32UI.is_integer());
        assert_eq!(TextureFormat::from_glsl_image_format("rgba16f"), Some(TextureFormat::RGBA16F));
        assert_eq!(TextureFormat::from_glsl_image_format("r32ui"), Some(TextureFormat::R32UI));
    }

    #[test]
    fn serde_uses_pack_names() {
        let j = serde_json::to_string(&TextureFormat::R11F_G11F_B10F).unwrap();
        assert_eq!(j, "\"R11F_G11F_B10F\"");
    }
}
