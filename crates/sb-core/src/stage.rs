//! Shader stages and their pack file extensions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShaderStage {
    Vertex,
    TessControl,
    TessEval,
    Geometry,
    Fragment,
    Compute,
}

impl ShaderStage {
    /// Graphics stages in pipeline order.
    pub const GRAPHICS: [ShaderStage; 5] =
        [Self::Vertex, Self::TessControl, Self::TessEval, Self::Geometry, Self::Fragment];

    /// Shader pack file extension (without dot): `vsh`, `tcs`, `tes`, `gsh`, `fsh`, `csh`.
    pub fn pack_extension(self) -> &'static str {
        match self {
            Self::Vertex => "vsh",
            Self::TessControl => "tcs",
            Self::TessEval => "tes",
            Self::Geometry => "gsh",
            Self::Fragment => "fsh",
            Self::Compute => "csh",
        }
    }

    pub fn from_pack_extension(ext: &str) -> Option<Self> {
        Some(match ext {
            "vsh" => Self::Vertex,
            "tcs" => Self::TessControl,
            "tes" => Self::TessEval,
            "gsh" => Self::Geometry,
            "fsh" => Self::Fragment,
            "csh" => Self::Compute,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Vertex => "vertex",
            Self::TessControl => "tess_control",
            Self::TessEval => "tess_eval",
            Self::Geometry => "geometry",
            Self::Fragment => "fragment",
            Self::Compute => "compute",
        }
    }

    /// True for the stages that run before rasterization (VS/TCS/TES/GS).
    pub fn is_pre_raster(self) -> bool {
        matches!(self, Self::Vertex | Self::TessControl | Self::TessEval | Self::Geometry)
    }
}

impl std::fmt::Display for ShaderStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
