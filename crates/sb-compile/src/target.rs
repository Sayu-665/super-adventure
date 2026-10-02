//! Vulkan and SPIR-V target versions.

use glslang_sys as sys;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Vulkan API version the SPIR-V is generated and validated for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub enum VulkanTarget {
    /// Vulkan 1.0.
    #[serde(rename = "vulkan1.0")]
    Vulkan1_0,
    /// Vulkan 1.1.
    #[serde(rename = "vulkan1.1")]
    Vulkan1_1,
    /// Minecraft 26.3's renderpearl Vulkan backend target (shaderc `vulkan1.2`).
    #[default]
    #[serde(rename = "vulkan1.2")]
    Vulkan1_2,
    /// Vulkan 1.3.
    #[serde(rename = "vulkan1.3")]
    Vulkan1_3,
    /// Vulkan 1.4.
    #[serde(rename = "vulkan1.4")]
    Vulkan1_4,
}

impl VulkanTarget {
    /// All supported targets, oldest first.
    pub const ALL: [VulkanTarget; 5] =
        [Self::Vulkan1_0, Self::Vulkan1_1, Self::Vulkan1_2, Self::Vulkan1_3, Self::Vulkan1_4];

    /// Newest SPIR-V version a driver for this Vulkan version must accept.
    pub fn max_spirv(self) -> SpirvTarget {
        match self {
            Self::Vulkan1_0 => SpirvTarget::Spirv1_0,
            Self::Vulkan1_1 => SpirvTarget::Spirv1_3,
            Self::Vulkan1_2 => SpirvTarget::Spirv1_5,
            Self::Vulkan1_3 | Self::Vulkan1_4 => SpirvTarget::Spirv1_6,
        }
    }

    /// Whether `spirv` may be consumed by this Vulkan version (VK 1.1 also accepts
    /// SPIR-V 1.4 through `VK_KHR_spirv_1_4`).
    pub fn supports(self, spirv: SpirvTarget) -> bool {
        spirv <= self.max_spirv() || (self == Self::Vulkan1_1 && spirv == SpirvTarget::Spirv1_4)
    }

    /// `spirv-val`/`spirv-opt` `--target-env` name for this Vulkan version and a
    /// module of SPIR-V version `spirv`.
    pub fn target_env(self, spirv: SpirvTarget) -> &'static str {
        match self {
            Self::Vulkan1_0 => "vulkan1.0",
            Self::Vulkan1_1 if spirv >= SpirvTarget::Spirv1_4 => "vulkan1.1spv1.4",
            Self::Vulkan1_1 => "vulkan1.1",
            Self::Vulkan1_2 => "vulkan1.2",
            Self::Vulkan1_3 => "vulkan1.3",
            Self::Vulkan1_4 => "vulkan1.4",
        }
    }

    pub(crate) fn to_sys(self) -> sys::glslang_target_client_version_t {
        use sys::glslang_target_client_version_t as V;
        match self {
            Self::Vulkan1_0 => V::Vulkan1_0,
            Self::Vulkan1_1 => V::Vulkan1_1,
            Self::Vulkan1_2 => V::Vulkan1_2,
            Self::Vulkan1_3 => V::Vulkan1_3,
            Self::Vulkan1_4 => V::Vulkan1_4,
        }
    }
}

impl fmt::Display for VulkanTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Vulkan1_0 => "Vulkan 1.0",
            Self::Vulkan1_1 => "Vulkan 1.1",
            Self::Vulkan1_2 => "Vulkan 1.2",
            Self::Vulkan1_3 => "Vulkan 1.3",
            Self::Vulkan1_4 => "Vulkan 1.4",
        })
    }
}

/// SPIR-V version of the generated module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub enum SpirvTarget {
    /// SPIR-V 1.0.
    #[serde(rename = "spv1.0")]
    Spirv1_0,
    /// SPIR-V 1.1.
    #[serde(rename = "spv1.1")]
    Spirv1_1,
    /// SPIR-V 1.2.
    #[serde(rename = "spv1.2")]
    Spirv1_2,
    /// SPIR-V 1.3.
    #[serde(rename = "spv1.3")]
    Spirv1_3,
    /// SPIR-V 1.4.
    #[serde(rename = "spv1.4")]
    Spirv1_4,
    /// The newest version Vulkan 1.2 guarantees (and spirq fully understands).
    #[default]
    #[serde(rename = "spv1.5")]
    Spirv1_5,
    /// SPIR-V 1.6.
    #[serde(rename = "spv1.6")]
    Spirv1_6,
}

impl SpirvTarget {
    /// All supported versions, oldest first.
    pub const ALL: [SpirvTarget; 7] = [
        Self::Spirv1_0,
        Self::Spirv1_1,
        Self::Spirv1_2,
        Self::Spirv1_3,
        Self::Spirv1_4,
        Self::Spirv1_5,
        Self::Spirv1_6,
    ];

    /// `(major, minor)`.
    pub fn version(self) -> (u8, u8) {
        match self {
            Self::Spirv1_0 => (1, 0),
            Self::Spirv1_1 => (1, 1),
            Self::Spirv1_2 => (1, 2),
            Self::Spirv1_3 => (1, 3),
            Self::Spirv1_4 => (1, 4),
            Self::Spirv1_5 => (1, 5),
            Self::Spirv1_6 => (1, 6),
        }
    }

    /// The version word stored in a SPIR-V module header (`0x00MMmm00`).
    pub fn header_word(self) -> u32 {
        let (major, minor) = self.version();
        (u32::from(major) << 16) | (u32::from(minor) << 8)
    }

    /// Parse a SPIR-V header version word.
    pub fn from_header_word(word: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.header_word() == word)
    }

    pub(crate) fn to_sys(self) -> sys::glslang_target_language_version_t {
        use sys::glslang_target_language_version_t as V;
        match self {
            Self::Spirv1_0 => V::SPIRV1_0,
            Self::Spirv1_1 => V::SPIRV1_1,
            Self::Spirv1_2 => V::SPIRV1_2,
            Self::Spirv1_3 => V::SPIRV1_3,
            Self::Spirv1_4 => V::SPIRV1_4,
            Self::Spirv1_5 => V::SPIRV1_5,
            Self::Spirv1_6 => V::SPIRV1_6,
        }
    }
}

impl fmt::Display for SpirvTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (major, minor) = self.version();
        write!(f, "SPIR-V {major}.{minor}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_minecraft() {
        assert_eq!(VulkanTarget::default(), VulkanTarget::Vulkan1_2);
        assert_eq!(SpirvTarget::default(), SpirvTarget::Spirv1_5);
        assert!(VulkanTarget::default().supports(SpirvTarget::default()));
    }

    #[test]
    fn spirv_support_matrix() {
        assert!(VulkanTarget::Vulkan1_0.supports(SpirvTarget::Spirv1_0));
        assert!(!VulkanTarget::Vulkan1_0.supports(SpirvTarget::Spirv1_1));
        assert!(VulkanTarget::Vulkan1_1.supports(SpirvTarget::Spirv1_3));
        assert!(VulkanTarget::Vulkan1_1.supports(SpirvTarget::Spirv1_4));
        assert!(!VulkanTarget::Vulkan1_1.supports(SpirvTarget::Spirv1_5));
        assert!(!VulkanTarget::Vulkan1_2.supports(SpirvTarget::Spirv1_6));
        assert!(VulkanTarget::Vulkan1_3.supports(SpirvTarget::Spirv1_6));
    }

    #[test]
    fn target_env_names() {
        assert_eq!(VulkanTarget::Vulkan1_2.target_env(SpirvTarget::Spirv1_5), "vulkan1.2");
        assert_eq!(VulkanTarget::Vulkan1_1.target_env(SpirvTarget::Spirv1_4), "vulkan1.1spv1.4");
        assert_eq!(VulkanTarget::Vulkan1_1.target_env(SpirvTarget::Spirv1_3), "vulkan1.1");
    }

    #[test]
    fn header_words_roundtrip() {
        assert_eq!(SpirvTarget::Spirv1_5.header_word(), 0x0001_0500);
        for v in SpirvTarget::ALL {
            assert_eq!(SpirvTarget::from_header_word(v.header_word()), Some(v));
        }
        assert_eq!(SpirvTarget::from_header_word(0x0002_0000), None);
    }

    #[test]
    fn serde_and_display_names() {
        assert_eq!(serde_json::to_string(&VulkanTarget::Vulkan1_2).unwrap(), "\"vulkan1.2\"");
        assert_eq!(serde_json::to_string(&SpirvTarget::Spirv1_5).unwrap(), "\"spv1.5\"");
        let v: VulkanTarget = serde_json::from_str("\"vulkan1.3\"").unwrap();
        assert_eq!(v, VulkanTarget::Vulkan1_3);
        assert_eq!(VulkanTarget::Vulkan1_2.to_string(), "Vulkan 1.2");
        assert_eq!(SpirvTarget::Spirv1_5.to_string(), "SPIR-V 1.5");
    }
}
