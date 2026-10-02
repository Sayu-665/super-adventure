//! Program identities: geometry programs (gbuffers_*, shadow_*, dh_*) with their
//! Iris fallback chains, composite-style pass groups and program file-name parsing.

use serde::{Deserialize, Serialize};

macro_rules! geometry_programs {
    ($( $variant:ident = $file:literal, fallback = $fb:expr, group = $grp:ident; )*) => {
        /// Geometry ("world") programs. File names are `<file>.<ext>`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum GeometryProgram { $( $variant, )* }

        impl GeometryProgram {
            pub const ALL: &'static [GeometryProgram] = &[ $( GeometryProgram::$variant, )* ];

            /// Program base file name, e.g. `gbuffers_terrain`, `shadow_cutout`, `dh_water`.
            pub fn file_name(self) -> &'static str { match self { $( Self::$variant => $file, )* } }

            /// Next program in the Iris fallback chain, if any.
            pub fn fallback(self) -> Option<GeometryProgram> { match self { $( Self::$variant => $fb, )* } }

            /// Which geometry group the program belongs to.
            pub fn group(self) -> GeometryGroup { match self { $( Self::$variant => GeometryGroup::$grp, )* } }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeometryGroup {
    Gbuffers,
    Shadow,
    DistantHorizons,
}

use GeometryProgram as G;
geometry_programs! {
    Basic = "gbuffers_basic", fallback = None, group = Gbuffers;
    Line = "gbuffers_line", fallback = Some(G::Basic), group = Gbuffers;
    Textured = "gbuffers_textured", fallback = Some(G::Basic), group = Gbuffers;
    TexturedLit = "gbuffers_textured_lit", fallback = Some(G::Textured), group = Gbuffers;
    SkyBasic = "gbuffers_skybasic", fallback = Some(G::Basic), group = Gbuffers;
    SkyTextured = "gbuffers_skytextured", fallback = Some(G::Textured), group = Gbuffers;
    Clouds = "gbuffers_clouds", fallback = Some(G::Textured), group = Gbuffers;
    Terrain = "gbuffers_terrain", fallback = Some(G::TexturedLit), group = Gbuffers;
    TerrainSolid = "gbuffers_terrain_solid", fallback = Some(G::Terrain), group = Gbuffers;
    TerrainCutout = "gbuffers_terrain_cutout", fallback = Some(G::Terrain), group = Gbuffers;
    DamagedBlock = "gbuffers_damagedblock", fallback = Some(G::Terrain), group = Gbuffers;
    Block = "gbuffers_block", fallback = Some(G::Terrain), group = Gbuffers;
    BlockTranslucent = "gbuffers_block_translucent", fallback = Some(G::Block), group = Gbuffers;
    BeaconBeam = "gbuffers_beaconbeam", fallback = Some(G::Textured), group = Gbuffers;
    Item = "gbuffers_item", fallback = Some(G::TexturedLit), group = Gbuffers;
    Entities = "gbuffers_entities", fallback = Some(G::TexturedLit), group = Gbuffers;
    EntitiesTranslucent = "gbuffers_entities_translucent", fallback = Some(G::Entities), group = Gbuffers;
    Lightning = "gbuffers_lightning", fallback = Some(G::Entities), group = Gbuffers;
    Particles = "gbuffers_particles", fallback = Some(G::TexturedLit), group = Gbuffers;
    ParticlesTranslucent = "gbuffers_particles_translucent", fallback = Some(G::Particles), group = Gbuffers;
    EntitiesGlowing = "gbuffers_entities_glowing", fallback = Some(G::Entities), group = Gbuffers;
    ArmorGlint = "gbuffers_armor_glint", fallback = Some(G::Textured), group = Gbuffers;
    SpiderEyes = "gbuffers_spidereyes", fallback = Some(G::Textured), group = Gbuffers;
    Hand = "gbuffers_hand", fallback = Some(G::TexturedLit), group = Gbuffers;
    Weather = "gbuffers_weather", fallback = Some(G::TexturedLit), group = Gbuffers;
    Water = "gbuffers_water", fallback = Some(G::Terrain), group = Gbuffers;
    HandWater = "gbuffers_hand_water", fallback = Some(G::Hand), group = Gbuffers;
    Shadow = "shadow", fallback = None, group = Shadow;
    ShadowSolid = "shadow_solid", fallback = Some(G::Shadow), group = Shadow;
    ShadowCutout = "shadow_cutout", fallback = Some(G::Shadow), group = Shadow;
    ShadowWater = "shadow_water", fallback = Some(G::Shadow), group = Shadow;
    ShadowEntities = "shadow_entities", fallback = Some(G::Shadow), group = Shadow;
    ShadowLightning = "shadow_lightning", fallback = Some(G::ShadowEntities), group = Shadow;
    ShadowBlock = "shadow_block", fallback = Some(G::Shadow), group = Shadow;
    DhTerrain = "dh_terrain", fallback = None, group = DistantHorizons;
    DhWater = "dh_water", fallback = Some(G::DhTerrain), group = DistantHorizons;
    DhGeneric = "dh_generic", fallback = Some(G::DhTerrain), group = DistantHorizons;
    DhShadow = "dh_shadow", fallback = None, group = DistantHorizons;
}

impl GeometryProgram {
    pub fn from_file_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|p| p.file_name() == name)
    }

    /// The full fallback chain starting with `self`.
    pub fn chain(self) -> impl Iterator<Item = GeometryProgram> {
        std::iter::successors(Some(self), |p| p.fallback())
    }

    /// Whether the program draws translucent geometry (rendered after `deferred`).
    pub fn is_translucent(self) -> bool {
        matches!(
            self,
            G::Water
                | G::HandWater
                | G::Weather
                | G::EntitiesTranslucent
                | G::BlockTranslucent
                | G::ParticlesTranslucent
                | G::DhWater
        )
    }

    /// Default alpha test as (comparison, reference) for programs that alpha-test by
    /// default (Iris: cutout geometry uses `GREATER 0.1`). `None` = no alpha test.
    pub fn default_alpha_test(self) -> Option<(AlphaFunc, f32)> {
        match self {
            G::Basic | G::Line | G::SkyBasic | G::Water | G::HandWater | G::DhTerrain | G::DhWater | G::DhGeneric | G::DhShadow
            | G::BeaconBeam | G::TerrainSolid | G::ShadowSolid | G::ShadowWater => None,
            _ => Some((AlphaFunc::Greater, 0.1)),
        }
    }

    /// Default blend mode before `blend.<prog>` overrides.
    pub fn default_blend(self) -> Option<BlendMode> {
        match self.group() {
            GeometryGroup::Shadow => None,
            _ => match self {
                G::SpiderEyes => Some(BlendMode {
                    src_color: BlendFactor::SrcAlpha,
                    dst_color: BlendFactor::One,
                    src_alpha: BlendFactor::Zero,
                    dst_alpha: BlendFactor::One,
                }),
                G::TerrainSolid | G::TerrainCutout | G::DhTerrain | G::DhShadow => None,
                _ => Some(BlendMode::TRANSLUCENT),
            },
        }
    }
}

/// Alpha test comparison (`alphaTest.<prog>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaFunc {
    Never,
    Less,
    Equal,
    LEqual,
    Greater,
    NotEqual,
    GEqual,
    Always,
}

impl AlphaFunc {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_uppercase();
        let s = s.strip_prefix("GL_").unwrap_or(&s);
        Some(match s {
            "NEVER" => Self::Never,
            "LESS" => Self::Less,
            "EQUAL" => Self::Equal,
            "LEQUAL" => Self::LEqual,
            "GREATER" => Self::Greater,
            "NOTEQUAL" => Self::NotEqual,
            "GEQUAL" => Self::GEqual,
            "ALWAYS" => Self::Always,
            _ => return None,
        })
    }
    /// GLSL comparison operator (`a <op> ref` passes the test). `None` for Never/Always.
    pub fn glsl_op(self) -> Option<&'static str> {
        Some(match self {
            Self::Less => "<",
            Self::Equal => "==",
            Self::LEqual => "<=",
            Self::Greater => ">",
            Self::NotEqual => "!=",
            Self::GEqual => ">=",
            Self::Never | Self::Always => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendFactor {
    Zero,
    One,
    SrcColor,
    OneMinusSrcColor,
    DstColor,
    OneMinusDstColor,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
    SrcAlphaSaturate,
}

impl BlendFactor {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_uppercase();
        let s = s.strip_prefix("GL_").unwrap_or(&s);
        Some(match s {
            "ZERO" => Self::Zero,
            "ONE" => Self::One,
            "SRC_COLOR" => Self::SrcColor,
            "ONE_MINUS_SRC_COLOR" => Self::OneMinusSrcColor,
            "DST_COLOR" => Self::DstColor,
            "ONE_MINUS_DST_COLOR" => Self::OneMinusDstColor,
            "SRC_ALPHA" => Self::SrcAlpha,
            "ONE_MINUS_SRC_ALPHA" => Self::OneMinusSrcAlpha,
            "DST_ALPHA" => Self::DstAlpha,
            "ONE_MINUS_DST_ALPHA" => Self::OneMinusDstAlpha,
            "SRC_ALPHA_SATURATE" => Self::SrcAlphaSaturate,
            _ => return None,
        })
    }
}

/// Separate color/alpha blend factors with ADD equations (what packs can express).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlendMode {
    pub src_color: BlendFactor,
    pub dst_color: BlendFactor,
    pub src_alpha: BlendFactor,
    pub dst_alpha: BlendFactor,
}

impl BlendMode {
    /// Iris default for translucent geometry: SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ONE_MINUS_SRC_ALPHA.
    pub const TRANSLUCENT: BlendMode = BlendMode {
        src_color: BlendFactor::SrcAlpha,
        dst_color: BlendFactor::OneMinusSrcAlpha,
        src_alpha: BlendFactor::One,
        dst_alpha: BlendFactor::OneMinusSrcAlpha,
    };
}

/// Composite-style (fullscreen / compute) pass groups and the geometry pass slots, in
/// execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassGroup {
    Setup,
    Begin,
    Shadow,
    ShadowComp,
    Prepare,
    GbuffersOpaque,
    Deferred,
    GbuffersTranslucent,
    Composite,
    Final,
}

impl PassGroup {
    /// Base file name for composite-style groups (`composite`, `deferred`, ...).
    pub fn base_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Setup => "setup",
            Self::Begin => "begin",
            Self::ShadowComp => "shadowcomp",
            Self::Prepare => "prepare",
            Self::Deferred => "deferred",
            Self::Composite => "composite",
            Self::Final => "final",
            Self::Shadow | Self::GbuffersOpaque | Self::GbuffersTranslucent => return None,
        })
    }

    /// Composite-style groups that are arrays (base + 1..=99).
    pub const ARRAYS: [PassGroup; 6] =
        [Self::Setup, Self::Begin, Self::ShadowComp, Self::Prepare, Self::Deferred, Self::Composite];

    /// Name of the virtual flip-only program that runs before this group, if any
    /// (`begin_pre`, `prepare_pre`, `deferred_pre`, `composite_pre`).
    pub fn pre_flip_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Begin => "begin_pre",
            Self::Prepare => "prepare_pre",
            Self::Deferred => "deferred_pre",
            Self::Composite => "composite_pre",
            _ => return None,
        })
    }

    /// Texture stage name used by `texture.<stage>.<name>`.
    pub fn texture_stage(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Begin => "begin",
            Self::Shadow | Self::GbuffersOpaque | Self::GbuffersTranslucent => "gbuffers",
            Self::ShadowComp => "shadowcomp",
            Self::Prepare => "prepare",
            Self::Deferred => "deferred",
            Self::Composite | Self::Final => "composite",
        }
    }
}

/// A parsed program file base name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ProgramName {
    /// A geometry program (`gbuffers_*`, `shadow*`, `dh_*`). `compute_letter` is set for
    /// geometry computes such as `shadow_a.csh` (`Some(0)` = `shadow.csh`, letters 1..=26).
    Geometry { program: GeometryProgram },
    /// A composite-style program: `group` + `index` (0 = base name, 1..=99 = numbered).
    Composite { group: PassGroup, index: u8 },
}

impl ProgramName {
    /// Parse a program base name (no extension, no compute letter), e.g. `composite12`,
    /// `deferred`, `gbuffers_water`, `final`, `setup3`.
    pub fn parse(base: &str) -> Option<Self> {
        if let Some(p) = GeometryProgram::from_file_name(base) {
            return Some(ProgramName::Geometry { program: p });
        }
        if base == "final" {
            return Some(ProgramName::Composite { group: PassGroup::Final, index: 0 });
        }
        for g in PassGroup::ARRAYS {
            let b = g.base_name().unwrap();
            if let Some(rest) = base.strip_prefix(b) {
                if rest.is_empty() {
                    return Some(ProgramName::Composite { group: g, index: 0 });
                }
                if rest.len() <= 2 && rest.bytes().all(|c| c.is_ascii_digit()) && !rest.starts_with('0') {
                    let n: u8 = rest.parse().ok()?;
                    if (1..=99).contains(&n) {
                        return Some(ProgramName::Composite { group: g, index: n });
                    }
                }
            }
        }
        None
    }

    /// Program base file name.
    pub fn file_name(&self) -> String {
        match self {
            ProgramName::Geometry { program } => program.file_name().to_string(),
            ProgramName::Composite { group, index } => {
                let b = group.base_name().unwrap_or("?");
                if *index == 0 { b.to_string() } else { format!("{b}{index}") }
            }
        }
    }

    /// Split a compute file stem like `composite3_b` into (`composite3`, Some('b')).
    pub fn split_compute_letter(stem: &str) -> (&str, Option<char>) {
        if let Some((base, letter)) = stem.rsplit_once('_')
            && let [c] = letter.as_bytes()
            && c.is_ascii_lowercase()
            && Self::parse(base).is_some()
        {
            return (base, Some(*c as char));
        }
        (stem, None)
    }
}

impl std::fmt::Display for ProgramName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.file_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_chains() {
        let chain: Vec<_> = GeometryProgram::HandWater.chain().map(|p| p.file_name()).collect();
        assert_eq!(chain, ["gbuffers_hand_water", "gbuffers_hand", "gbuffers_textured_lit", "gbuffers_textured", "gbuffers_basic"]);
        let chain: Vec<_> = GeometryProgram::ShadowLightning.chain().map(|p| p.file_name()).collect();
        assert_eq!(chain, ["shadow_lightning", "shadow_entities", "shadow"]);
        assert_eq!(GeometryProgram::DhWater.fallback(), Some(GeometryProgram::DhTerrain));
        assert_eq!(GeometryProgram::ALL.len(), 38);
    }

    #[test]
    fn parse_program_names() {
        assert_eq!(ProgramName::parse("composite"), Some(ProgramName::Composite { group: PassGroup::Composite, index: 0 }));
        assert_eq!(ProgramName::parse("composite15"), Some(ProgramName::Composite { group: PassGroup::Composite, index: 15 }));
        assert_eq!(ProgramName::parse("deferred99"), Some(ProgramName::Composite { group: PassGroup::Deferred, index: 99 }));
        assert_eq!(ProgramName::parse("deferred100"), None);
        assert_eq!(ProgramName::parse("composite01"), None);
        assert_eq!(ProgramName::parse("final"), Some(ProgramName::Composite { group: PassGroup::Final, index: 0 }));
        assert_eq!(ProgramName::parse("gbuffers_water"), Some(ProgramName::Geometry { program: GeometryProgram::Water }));
        assert_eq!(ProgramName::parse("shadowcomp2"), Some(ProgramName::Composite { group: PassGroup::ShadowComp, index: 2 }));
        assert_eq!(ProgramName::parse("shadow"), Some(ProgramName::Geometry { program: GeometryProgram::Shadow }));
        assert_eq!(ProgramName::parse("nonsense"), None);
        assert_eq!(ProgramName::split_compute_letter("composite3_b"), ("composite3", Some('b')));
        assert_eq!(ProgramName::split_compute_letter("shadow_a"), ("shadow", Some('a')));
        assert_eq!(ProgramName::split_compute_letter("shadow_cutout"), ("shadow_cutout", None));
        assert_eq!(ProgramName::split_compute_letter("gbuffers_terrain"), ("gbuffers_terrain", None));
    }
}
