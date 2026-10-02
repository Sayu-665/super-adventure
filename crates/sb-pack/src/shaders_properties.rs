//! Typed `shaders.properties`.
//!
//! The file is read twice, as Iris does:
//!
//! * the **preprocessed** entries (options and environment macros applied by the caller)
//!   drive every functional key;
//! * the **raw** entries drive the GUI layout keys (`screen`, `screen.<NAME>`,
//!   `screen.columns`, `screen.<NAME>.columns`, `sliders`, `profile.<NAME>`) and the
//!   feature flags (`iris.features.required` / `iris.features.optional`).
//!
//! Unset keys stay `None`; [`ShadersProperties::settings`] applies the Iris defaults.
//! Malformed values never abort parsing: they produce diagnostics and are skipped.

use crate::properties::PropEntry;
use crate::vfs::clean_path;
use indexmap::IndexMap;
use sb_core::model::{
    AlphaTest, CustomImage, CustomTexture, CustomUniform, ImageSize, PackSettings, Screen,
    ScreenEntry, ShadowSettings, StorageBuffer, TargetSize, TextureSource, ViewportScale,
};
use sb_core::program::{AlphaFunc, BlendFactor, BlendMode};
use sb_core::{Diagnostic, Diagnostics, GlslType, ProgramName, SourceLocation, TextureFormat};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// File name used in diagnostics.
pub const FILE: &str = "shaders.properties";

/// Maximum number of `image.<name>` custom images (Iris).
pub const MAX_CUSTOM_IMAGES: usize = 16;

/// Highest `bufferObject.<N>` index (Iris rejects larger ones).
pub const MAX_BUFFER_OBJECT_INDEX: u32 = 12;

/// Number of colortex buffers (Iris `MAX_COLOR_BUFFERS`).
pub const MAX_COLOR_BUFFERS: u32 = 32;

/// Legacy colortex aliases in index order (`gcolor` = colortex0, ...).
pub const LEGACY_BUFFER_NAMES: [&str; 8] = [
    "gcolor",
    "gdepth",
    "gnormal",
    "composite",
    "gaux1",
    "gaux2",
    "gaux3",
    "gaux4",
];

/// Pixel formats accepted by raw textures and images.
pub const PIXEL_FORMATS: [&str; 12] = [
    "RED",
    "RG",
    "RGB",
    "BGR",
    "RGBA",
    "BGRA",
    "RED_INTEGER",
    "RG_INTEGER",
    "RGB_INTEGER",
    "BGR_INTEGER",
    "RGBA_INTEGER",
    "BGRA_INTEGER",
];

/// Pixel types accepted by raw textures and images.
pub const PIXEL_TYPES: [&str; 22] = [
    "BYTE",
    "SHORT",
    "INT",
    "HALF_FLOAT",
    "FLOAT",
    "UNSIGNED_BYTE",
    "UNSIGNED_BYTE_3_3_2",
    "UNSIGNED_BYTE_2_3_3_REV",
    "UNSIGNED_SHORT",
    "UNSIGNED_SHORT_5_6_5",
    "UNSIGNED_SHORT_5_6_5_REV",
    "UNSIGNED_SHORT_4_4_4_4",
    "UNSIGNED_SHORT_4_4_4_4_REV",
    "UNSIGNED_SHORT_5_5_5_1",
    "UNSIGNED_SHORT_1_5_5_5_REV",
    "UNSIGNED_INT",
    "UNSIGNED_INT_8_8_8_8",
    "UNSIGNED_INT_8_8_8_8_REV",
    "UNSIGNED_INT_10_10_10_2",
    "UNSIGNED_INT_2_10_10_10_REV",
    "UNSIGNED_INT_10F_11F_11F_REV",
    "UNSIGNED_INT_5_9_9_9_REV",
];

/// `clouds=` / `dhClouds=` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudSetting {
    Fast,
    Fancy,
    Off,
}

impl CloudSetting {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Fancy => "fancy",
            Self::Off => "off",
        }
    }
}

/// `shadow.culling=` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowCulling {
    /// `false`: distance-based culling only.
    Distance,
    /// `true`: advanced (frustum) culling.
    Advanced,
    /// `reversed` / `safe_zone`.
    SafeZone,
}

impl ShadowCulling {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Distance => "distance",
            Self::Advanced => "advanced",
            Self::SafeZone => "safe_zone",
        }
    }
}

/// `particles.ordering=` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticleOrdering {
    Before,
    Mixed,
    After,
}

impl ParticleOrdering {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::Mixed => "mixed",
            Self::After => "after",
        }
    }
}

/// Texture stage of `texture.<stage>.<name>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureStage {
    Setup,
    Begin,
    ShadowComp,
    Prepare,
    /// gbuffers and shadow programs.
    Gbuffers,
    Deferred,
    /// composite and final programs.
    Composite,
}

impl TextureStage {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "setup" => Self::Setup,
            "begin" => Self::Begin,
            "shadowcomp" => Self::ShadowComp,
            "prepare" => Self::Prepare,
            "gbuffers" => Self::Gbuffers,
            "deferred" => Self::Deferred,
            "composite" => Self::Composite,
            _ => return None,
        })
    }

    /// The spelling used in `shaders.properties` and in [`CustomTexture::stage`].
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Begin => "begin",
            Self::ShadowComp => "shadowcomp",
            Self::Prepare => "prepare",
            Self::Gbuffers => "gbuffers",
            Self::Deferred => "deferred",
            Self::Composite => "composite",
        }
    }
}

/// Target type of a raw texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawTextureType {
    Texture1D,
    Texture2D,
    Texture3D,
    /// `TEXTURE_RECTANGLE` (sampled with unnormalized coordinates via `sampler2DRect`).
    Rectangle,
}

impl RawTextureType {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_uppercase().as_str() {
            "TEXTURE_1D" => Self::Texture1D,
            "TEXTURE_2D" => Self::Texture2D,
            "TEXTURE_3D" => Self::Texture3D,
            "TEXTURE_RECTANGLE" => Self::Rectangle,
            _ => return None,
        })
    }

    /// Texture target name used by the model (`1d`, `2d`, `3d`, `2d_rect`).
    pub fn target_name(self) -> &'static str {
        match self {
            Self::Texture1D => "1d",
            Self::Texture2D => "2d",
            Self::Texture3D => "3d",
            Self::Rectangle => "2d_rect",
        }
    }

    /// Number of size components (`TEXTURE_RECTANGLE` is 2D).
    pub fn dimensions(self) -> u8 {
        match self {
            Self::Texture1D => 1,
            Self::Texture2D | Self::Rectangle => 2,
            Self::Texture3D => 3,
        }
    }
}

/// A `texture.<stage>.<name>` or `customTexture.<name>` directive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextureDirective {
    /// Original key.
    pub key: String,
    /// `None` for `customTexture.<name>` (available in every program).
    pub stage: Option<TextureStage>,
    /// Sampler name (any `.N` suffix of `texture.` keys stripped).
    pub sampler: String,
    pub source: TextureSource,
    /// Target type for raw textures.
    pub raw_type: Option<RawTextureType>,
    pub line: u32,
}

impl TextureDirective {
    /// Stage string for [`CustomTexture::stage`] (`custom` for `customTexture.`).
    pub fn stage_name(&self) -> &'static str {
        self.stage.map(TextureStage::as_str).unwrap_or("custom")
    }

    /// Default filtering: PNG textures are nearest + repeat, raw textures linear + clamp.
    pub fn default_blur_clamp(&self) -> (bool, bool) {
        match self.source {
            TextureSource::Raw { .. } => (true, true),
            _ => (false, false),
        }
    }

    /// Pack file backing the texture, if any.
    pub fn pack_path(&self) -> Option<&str> {
        match &self.source {
            TextureSource::PackImage { path } | TextureSource::Raw { path, .. } => Some(path),
            _ => None,
        }
    }
}

/// One component of a `size.buffer.<buf>` override.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum SizeValue {
    /// Integer: pixels.
    Absolute(u32),
    /// Contains a `.`: fraction of the screen size.
    Relative(f32),
}

impl SizeValue {
    /// Convert to the model's per-axis size.
    pub fn to_axis(self) -> sb_core::model::AxisSize {
        match self {
            SizeValue::Absolute(p) => sb_core::model::AxisSize::Absolute(p),
            SizeValue::Relative(f) => sb_core::model::AxisSize::Relative(f),
        }
    }
}

/// `size.buffer.<buf>=<w> <h>`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BufferSizeOverride {
    pub width: SizeValue,
    pub height: SizeValue,
}

impl BufferSizeOverride {
    /// The model representation, if both components have the same kind.
    pub fn to_target_size(self) -> Option<TargetSize> {
        match (self.width, self.height) {
            (SizeValue::Relative(x), SizeValue::Relative(y)) => Some(TargetSize::Relative { x, y }),
            (SizeValue::Absolute(width), SizeValue::Absolute(height)) => {
                Some(TargetSize::Absolute { width, height })
            }
            (x, y) => Some(TargetSize::PerAxis { x: x.to_axis(), y: y.to_axis() }),
        }
    }
}

/// `indirect.<prog>=<ssbo index> <byte offset>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndirectDispatch {
    pub buffer: u32,
    pub offset: u32,
}

/// Parsed `shaders.properties`. `None` means "not set" (the default applies).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShadersProperties {
    // ----- global switches (preprocessed) -----
    pub clouds: Option<CloudSetting>,
    pub dh_clouds: Option<CloudSetting>,
    pub old_hand_light: Option<bool>,
    pub dynamic_hand_light: Option<bool>,
    pub old_lighting: Option<bool>,
    pub separate_ao: Option<bool>,
    pub shadow_terrain: Option<bool>,
    pub shadow_translucent: Option<bool>,
    pub shadow_entities: Option<bool>,
    pub shadow_player: Option<bool>,
    pub shadow_block_entities: Option<bool>,
    pub shadow_light_block_entities: Option<bool>,
    pub shadow_enabled: Option<bool>,
    pub dh_shadow_enabled: Option<bool>,
    pub underwater_overlay: Option<bool>,
    pub vignette: Option<bool>,
    pub sun: Option<bool>,
    pub moon: Option<bool>,
    pub stars: Option<bool>,
    pub sky: Option<bool>,
    /// `weather=<geometry> [<particles>]`: first value.
    pub weather: Option<bool>,
    /// `weather=<geometry> [<particles>]`: second value.
    pub weather_particles: Option<bool>,
    /// `backFace.solid|cutout|cutoutMipped|translucent`.
    pub back_face: IndexMap<String, bool>,
    pub rain_depth: Option<bool>,
    pub beacon_beam_depth: Option<bool>,
    pub frustum_culling: Option<bool>,
    pub occlusion_culling: Option<bool>,
    pub shadow_culling: Option<ShadowCulling>,
    pub allow_concurrent_compute: Option<bool>,
    pub separate_entity_draws: Option<bool>,
    pub particles_before_deferred: Option<bool>,
    /// Effective explicit ordering after applying `particles.ordering`,
    /// `separateEntityDraws` (forces mixed) and `particles.before.deferred` in file
    /// order, as Iris does.
    pub particles_ordering: Option<ParticleOrdering>,
    pub breaks_anisotropy: Option<bool>,
    pub voxelize_light_blocks: Option<bool>,
    pub skip_all_rendering: Option<bool>,
    pub end_flash_shadows: Option<bool>,
    pub supports_color_correction: Option<bool>,
    pub prepare_before_shadow: Option<bool>,
    pub fallback_tex: Option<u32>,
    /// `texture.noise=` (path relative to the shaders root).
    pub noise_texture: Option<String>,

    // ----- per-program / per-buffer (preprocessed) -----
    /// `scale.<prog>`.
    pub scale: IndexMap<String, ViewportScale>,
    /// `size.buffer.<buf>` by colortex index.
    pub buffer_sizes: IndexMap<u32, BufferSizeOverride>,
    /// `alphaTest.<prog>`; `off`/`false` is `ALWAYS`.
    pub alpha_test: IndexMap<String, AlphaTest>,
    /// `blend.<prog>`; `None` = `off`.
    pub blend: IndexMap<String, Option<BlendMode>>,
    /// `blend.<prog>.<buf>`: program -> colortex index -> blend (`None` = `off`).
    pub buffer_blend: IndexMap<String, IndexMap<u32, Option<BlendMode>>>,
    /// `flip.<prog>.<buf>`: program (may be `*_pre`) -> colortex index -> flip.
    pub flips: IndexMap<String, IndexMap<u32, bool>>,
    /// `program.<[dim/]prog>.enabled=<expr>`: program path -> boolean expression.
    pub program_enabled: IndexMap<String, String>,
    /// `indirect.<prog>`.
    pub indirect: IndexMap<String, IndirectDispatch>,

    // ----- resources (preprocessed) -----
    /// `texture.<stage>.<name>` in file order.
    pub textures: Vec<TextureDirective>,
    /// `customTexture.<name>`.
    pub custom_textures: IndexMap<String, TextureDirective>,
    /// `image.<name>` in file order.
    pub images: Vec<CustomImage>,
    /// `bufferObject.<N>` (disabled ones, size < 1, are omitted).
    pub buffer_objects: BTreeMap<u32, StorageBuffer>,
    /// `uniform.*` / `variable.*` in declaration order (first definition of a name wins).
    pub custom_uniforms: Vec<CustomUniform>,

    // ----- raw keys -----
    /// `screen=`; `None` if absent (all options on one screen).
    pub main_screen: Option<Vec<ScreenEntry>>,
    pub main_screen_columns: Option<u32>,
    /// `screen.<NAME>=`.
    pub screens: IndexMap<String, Vec<ScreenEntry>>,
    /// `screen.<NAME>.columns=`.
    pub screen_columns: IndexMap<String, u32>,
    pub sliders: Vec<String>,
    /// `profile.<NAME>=` raw token lists (see [`crate::options::resolve_profiles`]).
    pub profiles: IndexMap<String, Vec<String>>,
    pub features_required: Vec<String>,
    pub features_optional: Vec<String>,

    /// Every preprocessed key/value except the raw-only GUI and feature keys.
    pub functional: IndexMap<String, String>,
}

/// Parse a boolean value: `true`/`false`/`1`/`0`/`on`/`off` (case-insensitive).
pub fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "on" => Some(true),
        "false" | "0" | "off" => Some(false),
        _ => None,
    }
}

/// Resolve a buffer name (`colortex0`..`colortex31` or a legacy alias `gcolor`..`gaux4`)
/// to its colortex index.
pub fn parse_buffer_name(name: &str) -> Option<u32> {
    if let Some(i) = LEGACY_BUFFER_NAMES.iter().position(|n| *n == name) {
        return Some(i as u32);
    }
    let digits = name.strip_prefix("colortex")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<u32>()
        .ok()
        .filter(|i| *i < MAX_COLOR_BUFFERS)
}

fn is_raw_only_key(key: &str) -> bool {
    key == "screen"
        || key.starts_with("screen.")
        || key == "sliders"
        || key.starts_with("profile.")
        || key == "iris.features.required"
        || key == "iris.features.optional"
}

/// The path of `program` in program folder `folder` (`""` = pack root) as used by
/// `program.<path>.enabled` and profile `!program.<path>` entries: `composite2` for the
/// root, `world0/composite2` for a world folder.
pub fn program_path(folder: &str, program: &str) -> String {
    let folder = folder.trim_matches('/');
    if folder.is_empty() {
        program.to_string()
    } else {
        format!("{folder}/{program}")
    }
}

/// Program names accepted in per-program keys: real programs, compute programs
/// (`deferred4_a`) and the virtual flip-only `*_pre` programs.
fn is_known_program(name: &str) -> bool {
    ProgramName::parse(name).is_some()
        || ProgramName::split_compute_letter(name).1.is_some()
        || matches!(
            name,
            "begin_pre" | "prepare_pre" | "deferred_pre" | "composite_pre"
        )
}

struct Ctx<'a> {
    diags: &'a mut Diagnostics,
    /// colortex indices whose `size.buffer` was set through a legacy name (`gaux1`).
    legacy_sizes: std::collections::HashSet<u32>,
}

impl Ctx<'_> {
    fn at(&mut self, e: &PropEntry, d: Diagnostic) {
        self.diags.push(d.at(SourceLocation::new(FILE, e.line)));
    }
    fn bad(&mut self, e: &PropEntry, msg: impl Into<String>) {
        let msg = msg.into();
        self.at(
            e,
            Diagnostic::warning("props.bad-value", format!("`{}`: {msg}", e.key)),
        );
    }
    fn unknown_program(&mut self, e: &PropEntry, prog: &str) {
        if !is_known_program(prog) {
            self.at(
                e,
                Diagnostic::info(
                    "props.unknown-program",
                    format!("`{}` refers to unknown program `{prog}`", e.key),
                ),
            );
        }
    }
}

/// Parse `shaders.properties`. `preprocessed` are the entries of the preprocessed text,
/// `raw` those of the original text (both from [`crate::properties`]).
pub fn parse(preprocessed: &[PropEntry], raw: &[PropEntry]) -> (ShadersProperties, Diagnostics) {
    let mut diags = Diagnostics::new();
    let mut p = ShadersProperties::default();
    {
        let mut ctx = Ctx {
            diags: &mut diags,
            legacy_sizes: Default::default(),
        };
        for e in preprocessed {
            if is_raw_only_key(&e.key) {
                continue;
            }
            p.functional
                .insert(e.key.clone(), e.value.trim().to_string());
            parse_functional(&mut p, e, &mut ctx);
        }
        for e in raw {
            parse_raw(&mut p, e, &mut ctx);
        }
    }
    (p, diags)
}

fn bool_slot<'a>(p: &'a mut ShadersProperties, key: &str) -> Option<&'a mut Option<bool>> {
    Some(match key {
        "oldHandLight" => &mut p.old_hand_light,
        "dynamicHandLight" => &mut p.dynamic_hand_light,
        "oldLighting" => &mut p.old_lighting,
        "separateAo" => &mut p.separate_ao,
        "shadowTerrain" => &mut p.shadow_terrain,
        "shadowTranslucent" => &mut p.shadow_translucent,
        "shadowEntities" => &mut p.shadow_entities,
        "shadowPlayer" => &mut p.shadow_player,
        "shadowBlockEntities" => &mut p.shadow_block_entities,
        "shadowLightBlockEntities" => &mut p.shadow_light_block_entities,
        "shadow.enabled" => &mut p.shadow_enabled,
        "dhShadow.enabled" => &mut p.dh_shadow_enabled,
        "underwaterOverlay" => &mut p.underwater_overlay,
        "vignette" => &mut p.vignette,
        "sun" => &mut p.sun,
        "moon" => &mut p.moon,
        "stars" => &mut p.stars,
        "sky" => &mut p.sky,
        "rain.depth" => &mut p.rain_depth,
        "beacon.beam.depth" => &mut p.beacon_beam_depth,
        "frustum.culling" => &mut p.frustum_culling,
        "occlusion.culling" => &mut p.occlusion_culling,
        "allowConcurrentCompute" => &mut p.allow_concurrent_compute,
        "breaksAnisotropy" => &mut p.breaks_anisotropy,
        "voxelizeLightBlocks" => &mut p.voxelize_light_blocks,
        "skipAllRendering" => &mut p.skip_all_rendering,
        "endFlashShadows" => &mut p.end_flash_shadows,
        "supportsColorCorrection" => &mut p.supports_color_correction,
        "prepareBeforeShadow" => &mut p.prepare_before_shadow,
        _ => return None,
    })
}

fn parse_functional(p: &mut ShadersProperties, e: &PropEntry, ctx: &mut Ctx<'_>) {
    let key = e.key.as_str();
    let value = e.value.trim();

    if let Some(slot) = bool_slot(p, key) {
        match parse_bool(value) {
            Some(b) => *slot = Some(b),
            None => ctx.bad(e, format!("expected true or false, got `{value}`")),
        }
        return;
    }

    match key {
        "texture.noise" => {
            match clean_path(value).filter(|p| !p.is_empty()) {
                Some(path) => p.noise_texture = Some(path),
                None => ctx.bad(e, format!("invalid path `{value}`")),
            }
            return;
        }
        "clouds" => {
            p.clouds = match value.to_ascii_lowercase().as_str() {
                "fast" => Some(CloudSetting::Fast),
                "fancy" => Some(CloudSetting::Fancy),
                "off" => Some(CloudSetting::Off),
                _ => {
                    ctx.bad(e, format!("expected fast, fancy or off, got `{value}`"));
                    p.clouds
                }
            };
            return;
        }
        "dhClouds" => {
            p.dh_clouds = match value.to_ascii_lowercase().as_str() {
                "off" => Some(CloudSetting::Off),
                "on" | "fancy" => Some(CloudSetting::Fancy),
                _ => {
                    ctx.bad(e, format!("expected on or off, got `{value}`"));
                    p.dh_clouds
                }
            };
            return;
        }
        "shadow.culling" => {
            p.shadow_culling = match value.to_ascii_lowercase().as_str() {
                "false" => Some(ShadowCulling::Distance),
                "true" => Some(ShadowCulling::Advanced),
                "reversed" | "safe_zone" => Some(ShadowCulling::SafeZone),
                _ => {
                    ctx.bad(
                        e,
                        format!("expected true, false, reversed or safe_zone, got `{value}`"),
                    );
                    p.shadow_culling
                }
            };
            return;
        }
        "weather" => {
            let mut parts = value.split_whitespace();
            let geometry = parts.next().unwrap_or("");
            p.weather = Some(parse_bool(geometry).unwrap_or_else(|| {
                ctx.bad(e, format!("expected true or false, got `{geometry}`"));
                false
            }));
            if let Some(particles) = parts.next() {
                p.weather_particles = Some(parse_bool(particles).unwrap_or_else(|| {
                    ctx.bad(e, format!("expected true or false, got `{particles}`"));
                    false
                }));
            }
            return;
        }
        "separateEntityDraws" => {
            match parse_bool(value) {
                Some(b) => {
                    p.separate_entity_draws = Some(b);
                    p.particles_ordering = Some(ParticleOrdering::Mixed);
                }
                None => ctx.bad(e, format!("expected true or false, got `{value}`")),
            }
            return;
        }
        "particles.before.deferred" => {
            match parse_bool(value) {
                Some(b) => {
                    p.particles_before_deferred = Some(b);
                    if b && p.particles_ordering.is_none() {
                        p.particles_ordering = Some(ParticleOrdering::Before);
                    }
                }
                None => ctx.bad(e, format!("expected true or false, got `{value}`")),
            }
            return;
        }
        "particles.ordering" => {
            match value.to_ascii_lowercase().as_str() {
                "before" => p.particles_ordering = Some(ParticleOrdering::Before),
                "mixed" => p.particles_ordering = Some(ParticleOrdering::Mixed),
                "after" => p.particles_ordering = Some(ParticleOrdering::After),
                _ => {
                    // Iris resets to "unset" on an invalid value.
                    p.particles_ordering = None;
                    ctx.bad(e, format!("expected before, mixed or after, got `{value}`"));
                }
            }
            return;
        }
        "fallbackTex" => {
            match value.parse::<u32>() {
                Ok(i) if i < MAX_COLOR_BUFFERS => p.fallback_tex = Some(i),
                _ => ctx.bad(
                    e,
                    format!(
                        "expected a colortex index 0..{}, got `{value}`",
                        MAX_COLOR_BUFFERS - 1
                    ),
                ),
            }
            return;
        }
        _ => {}
    }

    if let Some(face) = key.strip_prefix("backFace.") {
        if !matches!(face, "solid" | "cutout" | "cutoutMipped" | "translucent") {
            ctx.at(
                e,
                Diagnostic::info("props.unknown-key", format!("unknown key `{key}`")),
            );
            return;
        }
        match parse_bool(value) {
            Some(b) => {
                p.back_face.insert(face.to_string(), b);
            }
            None => ctx.bad(e, format!("expected true or false, got `{value}`")),
        }
    } else if let Some(prog) = key.strip_prefix("scale.") {
        parse_scale(p, e, prog, value, ctx);
    } else if let Some(buf) = key.strip_prefix("size.buffer.") {
        parse_buffer_size(p, e, buf, value, ctx);
    } else if let Some(prog) = key.strip_prefix("alphaTest.") {
        parse_alpha_test(p, e, prog, value, ctx);
    } else if let Some(rest) = key.strip_prefix("blend.") {
        parse_blend(p, e, rest, value, ctx);
    } else if let Some(rest) = key.strip_prefix("flip.") {
        parse_flip(p, e, rest, value, ctx);
    } else if let Some(rest) = key.strip_prefix("program.") {
        match rest.strip_suffix(".enabled") {
            Some(path) if !path.is_empty() => {
                let prog = path.rsplit('/').next().unwrap_or(path);
                ctx.unknown_program(e, prog);
                p.program_enabled
                    .insert(path.to_string(), value.to_string());
            }
            _ => ctx.at(
                e,
                Diagnostic::warning(
                    "props.unknown-key",
                    format!("unknown key `{key}` (expected program.<[dim/]program>.enabled)"),
                ),
            ),
        }
    } else if let Some(prog) = key.strip_prefix("indirect.") {
        parse_indirect(p, e, prog, value, ctx);
    } else if let Some(index) = key.strip_prefix("bufferObject.") {
        parse_buffer_object(p, e, index, value, ctx);
    } else if let Some(rest) = key.strip_prefix("texture.") {
        parse_stage_texture(p, e, rest, value, ctx);
    } else if let Some(name) = key.strip_prefix("customTexture.") {
        if name.is_empty() {
            ctx.bad(e, "missing sampler name");
            return;
        }
        if let Some(d) = parse_texture_directive(e, None, name, value, ctx) {
            p.custom_textures.insert(name.to_string(), d);
        }
    } else if let Some(name) = key.strip_prefix("image.") {
        parse_image(p, e, name, value, ctx);
    } else if let Some(rest) = key.strip_prefix("uniform.") {
        parse_custom_uniform(p, e, rest, value, false, ctx);
    } else if let Some(rest) = key.strip_prefix("variable.") {
        parse_custom_uniform(p, e, rest, value, true, ctx);
    } else {
        ctx.at(
            e,
            Diagnostic::info(
                "props.unknown-key",
                format!("unknown or unsupported key `{key}`"),
            ),
        );
    }
}

fn parse_f32(s: &str) -> Option<f32> {
    let s = s.trim();
    let s = s.strip_suffix(['f', 'F']).unwrap_or(s);
    s.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn parse_scale(
    p: &mut ShadersProperties,
    e: &PropEntry,
    prog: &str,
    value: &str,
    ctx: &mut Ctx<'_>,
) {
    ctx.unknown_program(e, prog);
    let parts: Vec<&str> = value.split_whitespace().collect();
    let parsed = match parts.as_slice() {
        [s] => parse_f32(s).map(|scale| ViewportScale {
            scale,
            offset_x: 0.0,
            offset_y: 0.0,
        }),
        [s, x, y, ..] => match (parse_f32(s), parse_f32(x), parse_f32(y)) {
            (Some(scale), Some(offset_x), Some(offset_y)) => Some(ViewportScale {
                scale,
                offset_x,
                offset_y,
            }),
            _ => None,
        },
        _ => None,
    };
    match parsed {
        Some(v) => {
            p.scale.insert(prog.to_string(), v);
        }
        None => ctx.bad(
            e,
            format!("expected `<scale> [<offsetX> <offsetY>]`, got `{value}`"),
        ),
    }
}

fn parse_size_value(s: &str) -> Option<SizeValue> {
    if s.contains('.') {
        parse_f32(s).filter(|v| *v >= 0.0).map(SizeValue::Relative)
    } else {
        s.parse::<u32>().ok().map(SizeValue::Absolute)
    }
}

fn parse_buffer_size(
    p: &mut ShadersProperties,
    e: &PropEntry,
    buf: &str,
    value: &str,
    ctx: &mut Ctx<'_>,
) {
    let Some(index) = parse_buffer_name(buf) else {
        ctx.bad(e, format!("unknown buffer `{buf}`"));
        return;
    };
    let parts: Vec<&str> = value.split_whitespace().collect();
    let [w, h] = parts.as_slice() else {
        ctx.bad(e, format!("expected `<width> <height>`, got `{value}`"));
        return;
    };
    match (parse_size_value(w), parse_size_value(h)) {
        (Some(width), Some(height)) => {
            let o = BufferSizeOverride { width, height };
            // Iris looks the legacy name up first: `size.buffer.gaux1` beats
            // `size.buffer.colortex4` regardless of their order in the file.
            let legacy = LEGACY_BUFFER_NAMES.contains(&buf);
            if !legacy && ctx.legacy_sizes.contains(&index) {
                ctx.at(
                    e,
                    Diagnostic::info(
                        "props.size-overridden",
                        format!(
                            "`{}` is ignored: `size.buffer.{}` takes precedence",
                            e.key, LEGACY_BUFFER_NAMES[index as usize]
                        ),
                    ),
                );
                return;
            }
            if legacy {
                ctx.legacy_sizes.insert(index);
            }
            if o.to_target_size().is_none() {
                ctx.at(
                    e,
                    Diagnostic::warning(
                        "props.mixed-size",
                        format!("`{}` mixes absolute and relative sizes, which the pipeline model cannot represent", e.key),
                    ),
                );
            }
            p.buffer_sizes.insert(index, o);
        }
        _ => ctx.bad(
            e,
            format!("expected integer pixels or fractional scales, got `{value}`"),
        ),
    }
}

fn parse_alpha_test(
    p: &mut ShadersProperties,
    e: &PropEntry,
    prog: &str,
    value: &str,
    ctx: &mut Ctx<'_>,
) {
    ctx.unknown_program(e, prog);
    if value == "off" || value == "false" {
        p.alpha_test.insert(
            prog.to_string(),
            AlphaTest {
                func: AlphaFunc::Always,
                reference: 0.0,
            },
        );
        return;
    }
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() < 2 {
        ctx.bad(
            e,
            format!("expected `off` or `<function> <reference>`, got `{value}`"),
        );
        return;
    }
    if parts.len() > 2 {
        ctx.at(
            e,
            Diagnostic::warning(
                "props.extra-values",
                format!("`{}`: ignoring extra values in `{value}`", e.key),
            ),
        );
    }
    let Some(func) = AlphaFunc::parse(parts[0]) else {
        ctx.bad(e, format!("unknown alpha test function `{}`", parts[0]));
        return;
    };
    let Some(reference) = parse_f32(parts[1]) else {
        ctx.bad(e, format!("invalid reference value `{}`", parts[1]));
        return;
    };
    p.alpha_test
        .insert(prog.to_string(), AlphaTest { func, reference });
}

/// Parse a blend value: `off` -> `Ok(None)`, four factors -> `Ok(Some(..))`.
pub fn parse_blend_value(value: &str) -> Result<Option<BlendMode>, String> {
    let value = value.trim();
    if value == "off" {
        return Ok(None);
    }
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() != 4 {
        return Err(format!(
            "expected `off` or four blend factors, got `{value}`"
        ));
    }
    let mut f = [BlendFactor::Zero; 4];
    for (slot, part) in f.iter_mut().zip(&parts) {
        *slot = BlendFactor::parse(part).ok_or_else(|| format!("unknown blend factor `{part}`"))?;
    }
    Ok(Some(BlendMode {
        src_color: f[0],
        dst_color: f[1],
        src_alpha: f[2],
        dst_alpha: f[3],
    }))
}

fn parse_blend(
    p: &mut ShadersProperties,
    e: &PropEntry,
    rest: &str,
    value: &str,
    ctx: &mut Ctx<'_>,
) {
    let mode = match parse_blend_value(value) {
        Ok(m) => m,
        Err(msg) => {
            ctx.bad(e, msg);
            return;
        }
    };
    match rest.split_once('.') {
        None => {
            ctx.unknown_program(e, rest);
            p.blend.insert(rest.to_string(), mode);
        }
        Some((prog, buf)) => {
            ctx.unknown_program(e, prog);
            let buf = buf.split('.').next().unwrap_or(buf);
            match parse_buffer_name(buf) {
                Some(index) => {
                    p.buffer_blend
                        .entry(prog.to_string())
                        .or_default()
                        .insert(index, mode);
                }
                None => ctx.bad(e, format!("unknown buffer `{buf}`")),
            }
        }
    }
}

fn parse_flip(
    p: &mut ShadersProperties,
    e: &PropEntry,
    rest: &str,
    value: &str,
    ctx: &mut Ctx<'_>,
) {
    let Some((prog, buf)) = rest.split_once('.') else {
        ctx.bad(e, "expected flip.<program>.<buffer>");
        return;
    };
    ctx.unknown_program(e, prog);
    let Some(index) = parse_buffer_name(buf) else {
        ctx.bad(e, format!("unknown buffer `{buf}`"));
        return;
    };
    match parse_bool(value) {
        Some(b) => {
            p.flips
                .entry(prog.to_string())
                .or_default()
                .insert(index, b);
        }
        None => ctx.bad(e, format!("expected true or false, got `{value}`")),
    }
}

fn parse_indirect(
    p: &mut ShadersProperties,
    e: &PropEntry,
    prog: &str,
    value: &str,
    ctx: &mut Ctx<'_>,
) {
    ctx.unknown_program(e, prog);
    let parts: Vec<&str> = value.split_whitespace().collect();
    match parts.as_slice() {
        [b, o, ..] => match (b.parse::<u32>(), o.parse::<u32>()) {
            (Ok(buffer), Ok(offset)) if buffer <= MAX_BUFFER_OBJECT_INDEX => {
                p.indirect
                    .insert(prog.to_string(), IndirectDispatch { buffer, offset });
            }
            _ => ctx.bad(
                e,
                format!(
                    "expected `<buffer 0..{MAX_BUFFER_OBJECT_INDEX}> <byte offset>`, got `{value}`"
                ),
            ),
        },
        _ => ctx.bad(
            e,
            format!("expected `<buffer> <byte offset>`, got `{value}`"),
        ),
    }
}

fn parse_buffer_object(
    p: &mut ShadersProperties,
    e: &PropEntry,
    index: &str,
    value: &str,
    ctx: &mut Ctx<'_>,
) {
    let Ok(index) = index.parse::<u32>() else {
        ctx.bad(e, format!("invalid buffer index `{index}`"));
        return;
    };
    if index > MAX_BUFFER_OBJECT_INDEX {
        ctx.at(
            e,
            Diagnostic::error(
                "props.bad-value",
                format!(
                    "`{}`: buffer objects above index {MAX_BUFFER_OBJECT_INDEX} are reserved",
                    e.key
                ),
            ),
        );
        return;
    }
    let parts: Vec<&str> = value.split_whitespace().collect();
    let Some(size) = parts.first().and_then(|s| s.parse::<i64>().ok()) else {
        ctx.bad(e, format!("expected a size in bytes, got `{value}`"));
        return;
    };
    if size < 1 {
        // The pack disabled the buffer.
        return;
    }
    let size = size as u64;
    let buffer = if parts.len() <= 2 {
        let file = match parts.get(1) {
            Some(f) => match clean_path(f).filter(|p| !p.is_empty()) {
                Some(path) => Some(path),
                None => {
                    ctx.bad(e, format!("invalid file path `{f}`"));
                    return;
                }
            },
            None => None,
        };
        StorageBuffer {
            index,
            size,
            relative: None,
            file,
        }
    } else {
        let (Some(rel), Some(sx), Some(sy)) = (
            parts.get(1),
            parts.get(2).and_then(|s| parse_f32(s)),
            parts.get(3).and_then(|s| parse_f32(s)),
        ) else {
            ctx.bad(
                e,
                format!("expected `<bytes per pixel> <relative> <scaleX> <scaleY>`, got `{value}`"),
            );
            return;
        };
        let relative = rel.eq_ignore_ascii_case("true").then_some([sx, sy]);
        StorageBuffer {
            index,
            size,
            relative,
            file: None,
        }
    };
    p.buffer_objects.insert(index, buffer);
}

fn parse_stage_texture(
    p: &mut ShadersProperties,
    e: &PropEntry,
    rest: &str,
    value: &str,
    ctx: &mut Ctx<'_>,
) {
    let Some((stage, sampler)) = rest.split_once('.') else {
        ctx.bad(e, "expected texture.<stage>.<sampler>");
        return;
    };
    let Some(stage) = TextureStage::parse(stage) else {
        ctx.at(
            e,
            Diagnostic::warning(
                "props.bad-texture",
                format!("unknown texture stage `{stage}` in `{}`; ignored", e.key),
            ),
        );
        return;
    };
    // `.0`-`.9` (any `.suffix`) distinguishes several definitions of one sampler.
    let sampler = sampler.split('.').next().unwrap_or(sampler);
    if sampler.is_empty() {
        ctx.bad(e, "missing sampler name");
        return;
    }
    if let Some(d) = parse_texture_directive(e, Some(stage), sampler, value, ctx) {
        p.textures.push(d);
    }
}

fn valid_pixel_format(s: &str) -> Option<String> {
    let up = s.to_ascii_uppercase();
    PIXEL_FORMATS.contains(&up.as_str()).then_some(up)
}

fn valid_pixel_type(s: &str) -> Option<String> {
    let up = s.to_ascii_uppercase();
    PIXEL_TYPES.contains(&up.as_str()).then_some(up)
}

/// Parse a texture value (`<path>`, `<ns>:<path>`, `minecraft:dynamic/lightmap_1`, or
/// the raw form `<path> <type> <internalFormat> <dims...> <pixelFormat> <pixelType>`).
pub fn parse_texture_source(
    value: &str,
) -> Result<(TextureSource, Option<RawTextureType>), String> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    match parts.len() {
        0 => Err("empty texture path".into()),
        1 => {
            let v = parts[0];
            if let Some((ns, path)) = v.split_once(':') {
                if ns == "minecraft"
                    && (path == "dynamic/lightmap_1" || path == "dynamic/light_map_1")
                {
                    return Ok((
                        TextureSource::Dynamic {
                            name: "lightmap".into(),
                        },
                        None,
                    ));
                }
                return Ok((
                    TextureSource::Resource {
                        location: v.to_string(),
                    },
                    None,
                ));
            }
            let path = clean_path(v)
                .filter(|p| !p.is_empty())
                .ok_or_else(|| format!("invalid path `{v}`"))?;
            Ok((TextureSource::PackImage { path }, None))
        }
        6..=8 => {
            let path = clean_path(parts[0])
                .filter(|p| !p.is_empty())
                .ok_or_else(|| format!("invalid path `{}`", parts[0]))?;
            let ty = RawTextureType::parse(parts[1])
                .ok_or_else(|| format!("unknown texture type `{}`", parts[1]))?;
            let dims = parts.len() - 5;
            if dims != usize::from(ty.dimensions()) {
                return Err(format!(
                    "{} needs {} size value(s), got {dims}",
                    parts[1],
                    ty.dimensions()
                ));
            }
            let format = TextureFormat::parse(parts[2])
                .ok_or_else(|| format!("unknown internal format `{}`", parts[2]))?;
            let mut size = [0u32; 3];
            for (i, s) in parts[3..3 + dims].iter().enumerate() {
                size[i] = s
                    .parse::<u32>()
                    .map_err(|_| format!("invalid size `{s}`"))?;
            }
            let pf = parts[parts.len() - 2];
            let pt = parts[parts.len() - 1];
            let pixel_format =
                valid_pixel_format(pf).ok_or_else(|| format!("unknown pixel format `{pf}`"))?;
            let pixel_type =
                valid_pixel_type(pt).ok_or_else(|| format!("unknown pixel type `{pt}`"))?;
            Ok((
                TextureSource::Raw {
                    path,
                    target: ty.target_name().to_string(),
                    dimensions: ty.dimensions(),
                    format,
                    size,
                    pixel_format,
                    pixel_type,
                },
                Some(ty),
            ))
        }
        _ => Err(format!(
            "unrecognized texture value `{value}` (paths with spaces are not supported)"
        )),
    }
}

fn parse_texture_directive(
    e: &PropEntry,
    stage: Option<TextureStage>,
    sampler: &str,
    value: &str,
    ctx: &mut Ctx<'_>,
) -> Option<TextureDirective> {
    match parse_texture_source(value) {
        Ok((source, raw_type)) => Some(TextureDirective {
            key: e.key.clone(),
            stage,
            sampler: sampler.to_string(),
            source,
            raw_type,
            line: e.line,
        }),
        Err(msg) => {
            ctx.at(
                e,
                Diagnostic::warning("props.bad-texture", format!("`{}`: {msg}", e.key)),
            );
            None
        }
    }
}

fn parse_image(
    p: &mut ShadersProperties,
    e: &PropEntry,
    name: &str,
    value: &str,
    ctx: &mut Ctx<'_>,
) {
    if name.is_empty() {
        ctx.bad(e, "missing image name");
        return;
    }
    if p.images.len() >= MAX_CUSTOM_IMAGES {
        ctx.at(
            e,
            Diagnostic::error(
                "props.too-many-images",
                format!(
                    "only {MAX_CUSTOM_IMAGES} custom images are allowed; `{}` ignored",
                    e.key
                ),
            ),
        );
        return;
    }
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() < 7 {
        ctx.bad(e, format!("expected `<sampler|none> <pixelFormat> <internalFormat> <pixelType> <clear> <relative> <size...>`, got `{value}`"));
        return;
    }
    let sampler_name = (parts[0] != "none").then(|| parts[0].to_string());
    let Some(pixel_format) = valid_pixel_format(parts[1]) else {
        ctx.bad(e, format!("unknown pixel format `{}`", parts[1]));
        return;
    };
    let Some(format) = TextureFormat::parse(parts[2]) else {
        ctx.bad(e, format!("unknown internal format `{}`", parts[2]));
        return;
    };
    let Some(pixel_type) = valid_pixel_type(parts[3]) else {
        ctx.bad(e, format!("unknown pixel type `{}`", parts[3]));
        return;
    };
    let clear = parts[4].eq_ignore_ascii_case("true");
    let relative = parts[5].eq_ignore_ascii_case("true");
    let size = if relative {
        match (
            parts.get(6).and_then(|s| parse_f32(s)),
            parts.get(7).and_then(|s| parse_f32(s)),
        ) {
            (Some(x), Some(y)) => ImageSize::Relative { x, y },
            _ => {
                ctx.bad(
                    e,
                    format!("relative images need `<scaleX> <scaleY>`, got `{value}`"),
                );
                return;
            }
        }
    } else {
        let dims: Option<Vec<u32>> = parts[6..].iter().map(|s| s.parse::<u32>().ok()).collect();
        match dims.as_deref() {
            Some([w]) => ImageSize::Absolute1D { width: *w },
            Some([w, h]) => ImageSize::Absolute2D {
                width: *w,
                height: *h,
            },
            Some([w, h, d]) => ImageSize::Absolute3D {
                width: *w,
                height: *h,
                depth: *d,
            },
            _ => {
                ctx.bad(
                    e,
                    format!("absolute images need 1 to 3 integer sizes, got `{value}`"),
                );
                return;
            }
        }
    };
    p.images.push(CustomImage {
        name: name.to_string(),
        sampler_name,
        format,
        pixel_format,
        pixel_type,
        clear,
        size,
    });
}

/// Types accepted by `uniform.<type>.<name>` / `variable.<type>.<name>` (Iris).
pub fn custom_uniform_type(name: &str) -> Option<GlslType> {
    Some(match name {
        "bool" => GlslType::BOOL,
        "float" => GlslType::FLOAT,
        "int" => GlslType::INT,
        "vec2" => GlslType::VEC2,
        "vec3" => GlslType::VEC3,
        "vec4" => GlslType::VEC4,
        _ => return None,
    })
}

fn parse_custom_uniform(
    p: &mut ShadersProperties,
    e: &PropEntry,
    rest: &str,
    value: &str,
    is_variable: bool,
    ctx: &mut Ctx<'_>,
) {
    let kind = if is_variable { "variable" } else { "uniform" };
    let parts: Vec<&str> = rest.split('.').collect();
    let [ty, name] = parts.as_slice() else {
        ctx.bad(e, format!("expected {kind}.<type>.<name>"));
        return;
    };
    let Some(ty) = custom_uniform_type(ty) else {
        ctx.bad(
            e,
            format!(
                "unsupported {kind} type `{ty}` (expected bool, float, int, vec2, vec3 or vec4)"
            ),
        );
        return;
    };
    if name.is_empty()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || name.starts_with(|c: char| c.is_ascii_digit())
    {
        ctx.bad(e, format!("invalid {kind} name `{name}`"));
        return;
    }
    if value.is_empty() {
        ctx.bad(e, "empty expression");
        return;
    }
    if p.custom_uniforms.iter().any(|u| u.name == *name) {
        ctx.at(
            e,
            Diagnostic::warning(
                "props.duplicate-uniform",
                format!(
                    "custom uniform/variable `{name}` is already defined; `{}` ignored",
                    e.key
                ),
            ),
        );
        return;
    }
    p.custom_uniforms.push(CustomUniform {
        name: name.to_string(),
        ty,
        expression: value.to_string(),
        is_variable,
        location: Some(sb_core::SourceLocation::new("shaders.properties", e.line)),
    });
}

/// Parse a screen entry token: `<empty>`, `<profile>`, `*`, `[NAME]` or an option name.
pub fn parse_screen_entry(token: &str) -> ScreenEntry {
    match token {
        "<empty>" => ScreenEntry::Empty,
        "<profile>" => ScreenEntry::Profile,
        "*" => ScreenEntry::Rest,
        t if t.len() > 2 && t.starts_with('[') && t.ends_with(']') => {
            ScreenEntry::Screen(t[1..t.len() - 1].to_string())
        }
        t => ScreenEntry::Option(t.to_string()),
    }
}

fn parse_list(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_string).collect()
}

fn parse_raw(p: &mut ShadersProperties, e: &PropEntry, ctx: &mut Ctx<'_>) {
    let key = e.key.as_str();
    let value = e.value.trim();
    match key {
        "iris.features.required" => p.features_required = parse_list(value),
        "iris.features.optional" => p.features_optional = parse_list(value),
        "sliders" => p.sliders = parse_list(value),
        "screen" => {
            p.main_screen = Some(value.split_whitespace().map(parse_screen_entry).collect())
        }
        "screen.columns" => match value.parse::<u32>() {
            Ok(c) => p.main_screen_columns = Some(c),
            Err(_) => ctx.bad(e, format!("expected an integer, got `{value}`")),
        },
        _ => {
            if let Some(name) = key.strip_prefix("profile.") {
                if !name.is_empty() {
                    p.profiles.insert(name.to_string(), parse_list(value));
                }
            } else if let Some(rest) = key.strip_prefix("screen.") {
                if let Some(name) = rest.strip_suffix(".columns").filter(|n| !n.is_empty()) {
                    match value.parse::<u32>() {
                        Ok(c) => {
                            p.screen_columns.insert(name.to_string(), c);
                        }
                        Err(_) => ctx.bad(e, format!("expected an integer, got `{value}`")),
                    }
                } else if !rest.is_empty() {
                    p.screens.insert(
                        rest.to_string(),
                        value.split_whitespace().map(parse_screen_entry).collect(),
                    );
                }
            }
        }
    }
}

impl ShadersProperties {
    /// Effective particle ordering. When unset, Iris renders particles `after`
    /// translucent geometry (its "has deferred programs" check is always true).
    pub fn particles_ordering_resolved(&self) -> ParticleOrdering {
        self.particles_ordering.unwrap_or(ParticleOrdering::After)
    }

    /// The model's [`PackSettings`] with Iris defaults for unset keys. Const-directive
    /// fields (`sun_path_rotation`, half-lives, ...) keep their defaults; they come
    /// from shader sources.
    pub fn settings(&self) -> PackSettings {
        let d = PackSettings::default();
        PackSettings {
            clouds: self
                .clouds
                .map(|c| c.as_str().to_string())
                .unwrap_or(d.clouds),
            dh_clouds: self
                .dh_clouds
                .map(|c| c.as_str().to_string())
                .unwrap_or(d.dh_clouds),
            old_hand_light: self.old_hand_light.unwrap_or(d.old_hand_light),
            dynamic_hand_light: self.dynamic_hand_light.unwrap_or(d.dynamic_hand_light),
            old_lighting: self.old_lighting.unwrap_or(d.old_lighting),
            separate_ao: self.separate_ao.unwrap_or(d.separate_ao),
            underwater_overlay: self.underwater_overlay.unwrap_or(d.underwater_overlay),
            vignette: self.vignette.unwrap_or(d.vignette),
            sun: self.sun.unwrap_or(d.sun),
            moon: self.moon.unwrap_or(d.moon),
            stars: self.stars.unwrap_or(d.stars),
            sky: self.sky.unwrap_or(d.sky),
            weather: self.weather.unwrap_or(d.weather),
            weather_particles: self.weather_particles.unwrap_or(d.weather_particles),
            rain_depth: self.rain_depth.unwrap_or(d.rain_depth),
            beacon_beam_depth: self.beacon_beam_depth.unwrap_or(d.beacon_beam_depth),
            frustum_culling: self.frustum_culling.unwrap_or(d.frustum_culling),
            occlusion_culling: self.occlusion_culling.unwrap_or(d.occlusion_culling),
            separate_entity_draws: self
                .separate_entity_draws
                .unwrap_or(d.separate_entity_draws),
            allow_concurrent_compute: self
                .allow_concurrent_compute
                .unwrap_or(d.allow_concurrent_compute),
            particles_ordering: self.particles_ordering_resolved().as_str().to_string(),
            supports_color_correction: self
                .supports_color_correction
                .unwrap_or(d.supports_color_correction),
            skip_all_rendering: self.skip_all_rendering.unwrap_or(d.skip_all_rendering),
            voxelize_light_blocks: self
                .voxelize_light_blocks
                .unwrap_or(d.voxelize_light_blocks),
            end_flash_shadows: self.end_flash_shadows.unwrap_or(d.end_flash_shadows),
            fallback_tex: self.fallback_tex.unwrap_or(d.fallback_tex),
            back_face: self.back_face.clone(),
            raw: self.functional.clone(),
            ..d
        }
    }

    /// Apply the shadow-pass keys (`shadow.enabled`, `shadowTerrain`, ...,
    /// `shadow.culling`, `dhShadow.enabled`) to `shadow`. `shadow.enabled` is only
    /// applied when set; the pipeline decides the default (a `shadow` program exists).
    pub fn apply_shadow_settings(&self, shadow: &mut ShadowSettings) {
        if let Some(v) = self.shadow_enabled {
            shadow.enabled = v;
        }
        if let Some(v) = self.shadow_terrain {
            shadow.render_terrain = v;
        }
        if let Some(v) = self.shadow_translucent {
            shadow.render_translucent = v;
        }
        if let Some(v) = self.shadow_entities {
            shadow.render_entities = v;
        }
        if let Some(v) = self.shadow_player {
            shadow.render_player = v;
        }
        if let Some(v) = self.shadow_block_entities {
            shadow.render_block_entities = v;
        }
        if let Some(v) = self.shadow_light_block_entities {
            shadow.render_light_block_entities = v;
        }
        if let Some(c) = self.shadow_culling {
            shadow.culling = c.as_str().to_string();
        }
        if let Some(v) = self.dh_shadow_enabled {
            shadow.dh_shadow_enabled = v;
        }
    }

    /// `scale.<prog>`.
    pub fn scale_of(&self, program: &str) -> Option<ViewportScale> {
        self.scale.get(program).copied()
    }

    /// `alphaTest.<prog>`.
    pub fn alpha_test_of(&self, program: &str) -> Option<AlphaTest> {
        self.alpha_test.get(program).copied()
    }

    /// `blend.<prog>`: outer `None` = not overridden, inner `None` = `off`.
    pub fn blend_of(&self, program: &str) -> Option<Option<BlendMode>> {
        self.blend.get(program).copied()
    }

    /// `blend.<prog>.<buf>` overrides of a program.
    pub fn buffer_blend_of(&self, program: &str) -> Option<&IndexMap<u32, Option<BlendMode>>> {
        self.buffer_blend.get(program)
    }

    /// `flip.<prog>.<buf>` overrides of a program.
    pub fn flips_of(&self, program: &str) -> Option<&IndexMap<u32, bool>> {
        self.flips.get(program)
    }

    /// `program.<path>.enabled` expression of `program` in program folder `folder`
    /// (`""` = pack root).
    ///
    /// The key is matched against the program's path relative to the shaders root,
    /// exactly as Iris (and OptiFine) do: `program.composite.enabled` applies to the
    /// root `composite` only, while `world0/composite` needs
    /// `program.world0/composite.enabled`. (Packs with world folders therefore list
    /// every folder separately, e.g. `program.world0/shadow.enabled`,
    /// `program.world-1/shadow.enabled`, ...)
    pub fn program_enabled_expr(&self, folder: &str, program: &str) -> Option<&str> {
        self.program_enabled
            .get(&program_path(folder, program))
            .map(String::as_str)
    }

    /// `indirect.<prog>` as `(buffer, byte offset)`.
    pub fn indirect_of(&self, program: &str) -> Option<(u32, u32)> {
        self.indirect.get(program).map(|i| (i.buffer, i.offset))
    }

    /// `size.buffer.<buf>` of colortex `index`.
    pub fn buffer_size_of(&self, index: u32) -> Option<BufferSizeOverride> {
        self.buffer_sizes.get(&index).copied()
    }

    /// Storage buffers in index order.
    pub fn storage_buffers(&self) -> Vec<StorageBuffer> {
        self.buffer_objects.values().cloned().collect()
    }

    /// Custom textures for the model with default filtering (no `.mcmeta` lookup):
    /// the effective `texture.*` entries (see [`ShadersProperties::texture_directives`])
    /// followed by `customTexture.*` entries.
    pub fn custom_textures_model(&self) -> Vec<CustomTexture> {
        self.texture_directives()
            .into_iter()
            .map(|d| {
                let (blur, clamp) = d.default_blur_clamp();
                CustomTexture {
                    sampler: d.sampler.clone(),
                    stage: d.stage_name().to_string(),
                    source: d.source.clone(),
                    blur,
                    clamp,
                }
            })
            .collect()
    }

    /// Like [`ShadersProperties::custom_textures_model`], but reads `<path>.mcmeta`
    /// (`{"texture": {"blur": bool, "clamp": bool}}`) from the pack and reports missing
    /// texture files.
    pub fn resolve_custom_textures(
        &self,
        pack: &crate::ShaderPack,
    ) -> (Vec<CustomTexture>, Diagnostics) {
        let mut diags = Diagnostics::new();
        let mut out = Vec::new();
        for d in self.texture_directives() {
            let (mut blur, mut clamp) = d.default_blur_clamp();
            if let Some(path) = d.pack_path() {
                if !pack.exists(path) {
                    diags.push(
                        Diagnostic::warning(
                            "props.texture-missing",
                            format!("`{}`: texture file `{path}` does not exist", d.key),
                        )
                        .at(SourceLocation::new(FILE, d.line)),
                    );
                }
                let meta_path = format!("{path}.mcmeta");
                if let Some(meta) = pack.read_text(&meta_path) {
                    match serde_json::from_str::<serde_json::Value>(&meta) {
                        Ok(v) => {
                            if let Some(t) = v.get("texture") {
                                if let Some(b) = t.get("blur").and_then(serde_json::Value::as_bool)
                                {
                                    blur = b;
                                }
                                if let Some(c) = t.get("clamp").and_then(serde_json::Value::as_bool)
                                {
                                    clamp = c;
                                }
                            }
                        }
                        Err(err) => diags.push(
                            Diagnostic::warning(
                                "props.bad-mcmeta",
                                format!(
                                    "cannot parse `{meta_path}`: {err}; using default filtering"
                                ),
                            )
                            .at(SourceLocation::new(meta_path.clone(), 1)),
                        ),
                    }
                }
            }
            out.push(CustomTexture {
                sampler: d.sampler.clone(),
                stage: d.stage_name().to_string(),
                source: d.source.clone(),
                blur,
                clamp,
            });
        }
        (out, diags)
    }

    /// Effective texture directives (see [`ShadersProperties::custom_textures_model`]).
    ///
    /// For one stage and sampler name, the last PNG/resource definition wins, and so
    /// does the last raw definition of each texture type (Iris binds PNG textures by
    /// sampler name and patches raw ones by `(sampler, type, stage)`, so a raw
    /// `TEXTURE_3D` and a raw `TEXTURE_2D` variant of one sampler can coexist and the
    /// shader's sampler declaration selects one).
    pub fn texture_directives(&self) -> Vec<&TextureDirective> {
        let mut out: Vec<&TextureDirective> = Vec::new();
        for d in &self.textures {
            if let Some(pos) = out.iter().position(|o| {
                o.raw_type == d.raw_type && o.stage == d.stage && o.sampler == d.sampler
            }) {
                out.remove(pos);
            }
            out.push(d);
        }
        out.extend(self.custom_textures.values());
        out
    }

    /// GUI screens for the options model: `(main screen, main columns, sub-screens)`.
    pub fn screens_model(&self) -> (Vec<ScreenEntry>, Option<u32>, IndexMap<String, Screen>) {
        let mut screens: IndexMap<String, Screen> = self
            .screens
            .iter()
            .map(|(name, entries)| {
                (
                    name.clone(),
                    Screen {
                        entries: entries.clone(),
                        columns: self.screen_columns.get(name).copied(),
                    },
                )
            })
            .collect();
        // Column counts for screens without an entry list still create the screen.
        for (name, cols) in &self.screen_columns {
            screens.entry(name.clone()).or_insert_with(|| Screen {
                entries: Vec::new(),
                columns: Some(*cols),
            });
        }
        (
            self.main_screen.clone().unwrap_or_default(),
            self.main_screen_columns,
            screens,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::properties;
    use pretty_assertions::assert_eq;

    fn parse_text(pre: &str, raw: &str) -> (ShadersProperties, Diagnostics) {
        parse(&properties::parse(pre), &properties::parse(raw))
    }

    fn codes(d: &Diagnostics) -> Vec<String> {
        d.iter().map(|d| d.code.clone()).collect()
    }

    #[test]
    fn booleans_and_enums() {
        let (p, d) = parse_text(
            "oldHandLight=false\ndynamicHandLight=0\noldLighting=on\nseparateAo=1\nshadowTerrain=true\nshadowTranslucent=false\n\
             shadowEntities=false\nshadowPlayer=true\nshadowBlockEntities=false\nshadowLightBlockEntities=true\nshadow.enabled=true\n\
             dhShadow.enabled=false\nunderwaterOverlay=true\nvignette=true\nsun=false\nmoon=false\nstars=false\nsky=false\n\
             rain.depth=true\nbeacon.beam.depth=true\nfrustum.culling=false\nocclusion.culling=false\nallowConcurrentCompute=true\n\
             breaksAnisotropy=true\nvoxelizeLightBlocks=true\nskipAllRendering=false\nendFlashShadows=true\n\
             supportsColorCorrection=true\nprepareBeforeShadow=true\nclouds=off\ndhClouds=on\nshadow.culling=reversed\n\
             backFace.solid=true\nbackFace.cutoutMipped=false\nfallbackTex=4\ntexture.noise=/tex/noise.png\nsun=maybe",
            "",
        );
        assert_eq!(p.old_hand_light, Some(false));
        assert_eq!(p.dynamic_hand_light, Some(false));
        assert_eq!(p.old_lighting, Some(true));
        assert_eq!(p.separate_ao, Some(true));
        assert_eq!(p.shadow_light_block_entities, Some(true));
        assert_eq!(p.dh_shadow_enabled, Some(false));
        assert_eq!(p.clouds, Some(CloudSetting::Off));
        assert_eq!(p.dh_clouds, Some(CloudSetting::Fancy));
        assert_eq!(p.shadow_culling, Some(ShadowCulling::SafeZone));
        assert_eq!(p.back_face["solid"], true);
        assert_eq!(p.back_face["cutoutMipped"], false);
        assert_eq!(p.fallback_tex, Some(4));
        assert_eq!(p.noise_texture.as_deref(), Some("tex/noise.png"));
        // `sun=maybe` is a later duplicate key, so it replaced `sun=false` and was rejected.
        assert_eq!(p.sun, None);
        assert_eq!(codes(&d), vec!["props.bad-value"]);

        let s = p.settings();
        assert!(!s.old_hand_light);
        assert!(s.sun, "unset -> default");
        assert!(!s.moon);
        assert_eq!(s.clouds, "off");
        assert_eq!(s.dh_clouds, "fancy");
        assert_eq!(s.fallback_tex, 4);
        assert!(s.raw.contains_key("clouds"));

        let mut shadow = ShadowSettings::default();
        p.apply_shadow_settings(&mut shadow);
        assert!(shadow.enabled);
        assert!(!shadow.render_translucent);
        assert!(shadow.render_player);
        assert!(!shadow.dh_shadow_enabled);
        assert_eq!(shadow.culling, "safe_zone");
    }

    #[test]
    fn weather_two_values() {
        let (p, _) = parse_text("weather=false true", "");
        assert_eq!((p.weather, p.weather_particles), (Some(false), Some(true)));
        let (p, _) = parse_text("weather=true", "");
        assert_eq!((p.weather, p.weather_particles), (Some(true), None));
    }

    #[test]
    fn particle_ordering_rules() {
        let (p, _) = parse_text("particles.before.deferred=true", "");
        assert_eq!(p.particles_ordering, Some(ParticleOrdering::Before));
        let (p, _) = parse_text(
            "particles.ordering=AFTER\nparticles.before.deferred=true",
            "",
        );
        assert_eq!(p.particles_ordering, Some(ParticleOrdering::After));
        let (p, _) = parse_text("particles.ordering=before\nseparateEntityDraws=true", "");
        assert_eq!(p.particles_ordering, Some(ParticleOrdering::Mixed));
        assert_eq!(p.separate_entity_draws, Some(true));
        let (p, _) = parse_text("", "");
        assert_eq!(p.particles_ordering_resolved(), ParticleOrdering::After);
        assert_eq!(p.settings().particles_ordering, "after");
    }

    #[test]
    fn per_program_keys() {
        let (p, d) = parse_text(
            "scale.composite3=0.5\nscale.composite4=0.25 0.5 0.75\nscale.composite5=0.5 1\n\
             alphaTest.gbuffers_terrain=GREATER 0.5\nalphaTest.shadow=off\nalphaTest.gbuffers_water=GL_ALWAYS 0.0 extra\n\
             blend.gbuffers_water=SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ONE_MINUS_SRC_ALPHA\nblend.gbuffers_hand=off\n\
             blend.gbuffers_clouds.colortex3=ONE ZERO ONE ZERO\nblend.gbuffers_clouds.gaux1=off\nblend.composite=ONE BOGUS ZERO ONE\n\
             flip.composite_pre.colortex4=true\nflip.deferred2.gdepth=false\nflip.deferred2.nope=true\n\
             program.world0/composite2.enabled=SSAO && !LOW\nprogram.composite5.enabled=FOO\nprogram.composite.notenabled=1\n\
             indirect.composite3=2 16",
            "",
        );
        assert_eq!(
            p.scale_of("composite3"),
            Some(ViewportScale {
                scale: 0.5,
                offset_x: 0.0,
                offset_y: 0.0
            })
        );
        assert_eq!(
            p.scale_of("composite4"),
            Some(ViewportScale {
                scale: 0.25,
                offset_x: 0.5,
                offset_y: 0.75
            })
        );
        assert_eq!(p.scale_of("composite5"), None, "two values are invalid");
        assert_eq!(
            p.alpha_test_of("gbuffers_terrain"),
            Some(AlphaTest {
                func: AlphaFunc::Greater,
                reference: 0.5
            })
        );
        assert_eq!(
            p.alpha_test_of("shadow"),
            Some(AlphaTest {
                func: AlphaFunc::Always,
                reference: 0.0
            })
        );
        assert_eq!(
            p.alpha_test_of("gbuffers_water").unwrap().func,
            AlphaFunc::Always
        );
        assert_eq!(
            p.blend_of("gbuffers_water"),
            Some(Some(BlendMode::TRANSLUCENT))
        );
        assert_eq!(p.blend_of("gbuffers_hand"), Some(None));
        assert_eq!(p.blend_of("composite"), None, "unknown factor rejected");
        let bb = p.buffer_blend_of("gbuffers_clouds").unwrap();
        assert_eq!(bb[&3].unwrap().src_color, BlendFactor::One);
        assert_eq!(bb[&4], None);
        assert!(p.flips_of("composite_pre").unwrap()[&4]);
        assert!(!p.flips_of("deferred2").unwrap()[&1]);
        assert_eq!(p.flips_of("deferred2").unwrap().len(), 1);
        assert_eq!(
            p.program_enabled_expr("world0", "composite2"),
            Some("SSAO && !LOW")
        );
        assert_eq!(p.program_enabled_expr("", "composite5"), Some("FOO"));
        assert_eq!(p.program_enabled_expr("", "composite2"), None);
        assert_eq!(p.indirect_of("composite3"), Some((2, 16)));
        assert_eq!(
            codes(&d),
            vec![
                "props.bad-value",
                "props.extra-values",
                "props.bad-value",
                "props.bad-value",
                "props.unknown-key"
            ]
        );
    }

    #[test]
    fn program_enabled_keys_match_the_exact_program_path() {
        // Regression: a bare `program.composite5.enabled` used to also apply to
        // `world0/composite5`. Iris compares the key with the program file's path
        // (`world0/composite5`), so it only affects the root program.
        let (p, _) = parse_text(
            "program.composite5.enabled=TAA\nprogram.world-1/shadow.enabled=false\n\
             program.world0/deferred4_a.enabled=SSGI",
            "",
        );
        assert_eq!(p.program_enabled_expr("", "composite5"), Some("TAA"));
        assert_eq!(p.program_enabled_expr("world0", "composite5"), None);
        assert_eq!(p.program_enabled_expr("world-1", "shadow"), Some("false"));
        assert_eq!(p.program_enabled_expr("world1", "shadow"), None);
        assert_eq!(p.program_enabled_expr("", "shadow"), None);
        assert_eq!(
            p.program_enabled_expr("world0/", "deferred4_a"),
            Some("SSGI")
        );
        assert_eq!(program_path("", "final"), "final");
        assert_eq!(program_path("world1", "final"), "world1/final");
    }

    #[test]
    fn buffer_sizes_and_names() {
        let (p, d) = parse_text(
            "size.buffer.colortex5=256 128\nsize.buffer.gaux3=0.5 0.25\nsize.buffer.colortex7=0.5 64\nsize.buffer.foo=1 1",
            "",
        );
        assert_eq!(
            p.buffer_size_of(5).unwrap().to_target_size(),
            Some(TargetSize::Absolute {
                width: 256,
                height: 128
            })
        );
        assert_eq!(
            p.buffer_size_of(6).unwrap().to_target_size(),
            Some(TargetSize::Relative { x: 0.5, y: 0.25 })
        );
        let mixed = p.buffer_size_of(7).unwrap();
        assert_eq!(
            mixed,
            BufferSizeOverride {
                width: SizeValue::Relative(0.5),
                height: SizeValue::Absolute(64)
            }
        );
        assert_eq!(mixed.to_target_size(), None);
        assert_eq!(p.buffer_sizes.len(), 3);
        assert_eq!(codes(&d), vec!["props.mixed-size", "props.bad-value"]);
        assert_eq!(parse_buffer_name("gcolor"), Some(0));
        assert_eq!(parse_buffer_name("gaux4"), Some(7));
        assert_eq!(parse_buffer_name("colortex15"), Some(15));
        assert_eq!(parse_buffer_name("colortex31"), Some(31));
        assert_eq!(parse_buffer_name("colortex32"), None);
        assert_eq!(parse_buffer_name("colortex"), None);
        assert_eq!(parse_buffer_name("colortex-1"), None);
    }

    #[test]
    fn legacy_buffer_names_take_precedence_for_sizes() {
        // Iris (`PackDirectives.getTextureScaleOverride`) checks the legacy name first.
        for text in [
            "size.buffer.gaux1=0.5 0.5\nsize.buffer.colortex4=64 64",
            "size.buffer.colortex4=64 64\nsize.buffer.gaux1=0.5 0.5",
        ] {
            let (p, d) = parse_text(text, "");
            assert_eq!(
                p.buffer_size_of(4).unwrap().to_target_size(),
                Some(TargetSize::Relative { x: 0.5, y: 0.5 }),
                "{text}"
            );
            assert!(d.iter().all(|d| d.severity == sb_core::Severity::Info));
        }
        let (p, _) = parse_text(
            "size.buffer.colortex4=64 64\nsize.buffer.colortex4=32 32",
            "",
        );
        assert_eq!(
            p.buffer_size_of(4).unwrap().to_target_size(),
            Some(TargetSize::Absolute {
                width: 32,
                height: 32
            }),
            "duplicate keys: the last value wins"
        );
    }

    #[test]
    fn textures() {
        let (p, d) = parse_text(
            "texture.noise=tex/noise.png\n\
             texture.composite.colortex8=tex/lut.png\n\
             texture.gbuffers.gaux1.1=minecraft:textures/atlas/blocks.png\n\
             texture.deferred.lightmap=minecraft:dynamic/light_map_1\n\
             texture.composite.volume.0=tex/vol.dat TEXTURE_3D RGBA8 32 32 32 RGBA UNSIGNED_BYTE\n\
             texture.composite.volume.1=tex/line.dat texture_1d R16F 256 RED half_float\n\
             texture.prepare.rect=tex/r.dat TEXTURE_RECTANGLE RG32F 4 4 RG FLOAT\n\
             texture.bogus.x=a.png\n\
             texture.composite.bad=a.dat TEXTURE_2D RGBA8 4 RGBA UNSIGNED_BYTE\n\
             texture.composite.spaces=my file.png\n\
             customTexture.blueNoise=/tex/blue.png",
            "",
        );
        assert_eq!(p.noise_texture.as_deref(), Some("tex/noise.png"));
        assert_eq!(p.textures.len(), 6);
        let lut = &p.textures[0];
        assert_eq!(
            (lut.stage, lut.sampler.as_str()),
            (Some(TextureStage::Composite), "colortex8")
        );
        assert_eq!(
            lut.source,
            TextureSource::PackImage {
                path: "tex/lut.png".into()
            }
        );
        assert_eq!(p.textures[1].sampler, "gaux1");
        assert_eq!(
            p.textures[1].source,
            TextureSource::Resource {
                location: "minecraft:textures/atlas/blocks.png".into()
            }
        );
        assert_eq!(
            p.textures[2].source,
            TextureSource::Dynamic {
                name: "lightmap".into()
            }
        );
        let vol = &p.textures[3];
        assert_eq!(vol.raw_type, Some(RawTextureType::Texture3D));
        assert_eq!(
            vol.source,
            TextureSource::Raw {
                path: "tex/vol.dat".into(),
                target: "3d".into(),
                dimensions: 3,
                format: TextureFormat::RGBA8,
                size: [32, 32, 32],
                pixel_format: "RGBA".into(),
                pixel_type: "UNSIGNED_BYTE".into(),
            }
        );
        assert_eq!(p.textures[4].raw_type, Some(RawTextureType::Texture1D));
        assert_eq!(
            p.custom_textures["blueNoise"].source,
            TextureSource::PackImage {
                path: "tex/blue.png".into()
            }
        );
        assert_eq!(p.custom_textures["blueNoise"].stage_name(), "custom");
        assert_eq!(
            (p.textures[5].stage, p.textures[5].raw_type),
            (Some(TextureStage::Prepare), Some(RawTextureType::Rectangle))
        );
        assert_eq!(
            codes(&d),
            vec![
                "props.bad-texture",
                "props.bad-texture",
                "props.bad-texture"
            ]
        );
    }

    #[test]
    fn rectangle_textures_are_two_dimensional() {
        let (src, ty) =
            parse_texture_source("tex/r.dat TEXTURE_RECTANGLE RG32F 4 8 RG FLOAT").unwrap();
        assert_eq!(ty, Some(RawTextureType::Rectangle));
        match src {
            TextureSource::Raw {
                dimensions, size, ..
            } => assert_eq!((dimensions, size), (2, [4, 8, 0])),
            other => panic!("unexpected {other:?}"),
        }
        assert!(parse_texture_source("x.dat TEXTURE_9D RGBA8 1 RGBA FLOAT").is_err());
        assert!(parse_texture_source("x.dat TEXTURE_1D NOPE 1 RGBA FLOAT").is_err());
        assert!(parse_texture_source("x.dat TEXTURE_1D RGBA8 1 NOPE FLOAT").is_err());
        assert!(parse_texture_source("x.dat TEXTURE_1D RGBA8 1 RGBA NOPE").is_err());
        assert!(parse_texture_source("../x.png").is_err());
        assert!(parse_texture_source("").is_err());
    }

    #[test]
    fn texture_model_dedup_and_filtering_defaults() {
        let (p, _) = parse_text(
            "texture.composite.a.0=one.png\ntexture.composite.a.1=two.png\ntexture.gbuffers.a=three.png\n\
             texture.composite.v.0=v1.dat TEXTURE_1D R8 4 RED UNSIGNED_BYTE\ntexture.composite.v.1=v3.dat TEXTURE_3D R8 2 2 2 RED UNSIGNED_BYTE\n\
             texture.composite.v.2=v3b.dat TEXTURE_3D R8 4 4 4 RED UNSIGNED_BYTE\n\
             customTexture.c=c.png",
            "",
        );
        let model = p.custom_textures_model();
        let summary: Vec<(String, String, bool)> = model
            .iter()
            .map(|t| (t.stage.clone(), t.sampler.clone(), t.blur))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("composite".to_string(), "a".to_string(), false),
                ("gbuffers".to_string(), "a".to_string(), false),
                ("composite".to_string(), "v".to_string(), true),
                ("composite".to_string(), "v".to_string(), true),
                ("custom".to_string(), "c".to_string(), false),
            ]
        );
        assert_eq!(
            model[0].source,
            TextureSource::PackImage {
                path: "two.png".into()
            },
            "last PNG definition wins"
        );
        // Regression: a later raw definition of the same type replaces the earlier one
        // (Iris patches raw textures per sampler, type and stage).
        let raw_paths: Vec<&str> = p
            .texture_directives()
            .into_iter()
            .filter_map(|d| d.raw_type.and(d.pack_path()))
            .collect();
        assert_eq!(raw_paths, vec!["v1.dat", "v3b.dat"]);
    }

    #[test]
    fn mcmeta_and_missing_texture_files() {
        let pack = crate::ShaderPack::from_files(
            "t",
            [
                ("tex/a.png", &b"png"[..]),
                (
                    "tex/a.png.mcmeta",
                    &br#"{"texture":{"blur":true,"clamp":true}}"#[..],
                ),
                ("tex/b.png.mcmeta", &b"{bad"[..]),
            ],
        );
        let (p, _) = parse_text(
            "texture.composite.a=tex/a.png\ntexture.composite.b=tex/b.png",
            "",
        );
        let (model, d) = p.resolve_custom_textures(&pack);
        assert!(model[0].blur && model[0].clamp);
        assert!(!model[1].blur && !model[1].clamp);
        assert_eq!(codes(&d), vec!["props.texture-missing", "props.bad-mcmeta"]);
    }

    #[test]
    fn images() {
        let (p, d) = parse_text(
            "image.voxels=voxelSampler RED_INTEGER R32UI UNSIGNED_INT true false 128 64 128\n\
             image.half=none RGBA RGBA16F HALF_FLOAT false true 0.5 0.5\n\
             image.line=none RGBA RGBA8 UNSIGNED_BYTE TRUE false 256\n\
             image.bad=none RGBA NOPE UNSIGNED_BYTE true false 4 4\n\
             image.short=none RGBA",
            "",
        );
        assert_eq!(p.images.len(), 3);
        let v = &p.images[0];
        assert_eq!(v.name, "voxels");
        assert_eq!(v.sampler_name.as_deref(), Some("voxelSampler"));
        assert_eq!(v.format, TextureFormat::R32UI);
        assert!(v.clear);
        assert_eq!(
            v.size,
            ImageSize::Absolute3D {
                width: 128,
                height: 64,
                depth: 128
            }
        );
        assert_eq!(p.images[1].sampler_name, None);
        assert_eq!(p.images[1].size, ImageSize::Relative { x: 0.5, y: 0.5 });
        assert_eq!(p.images[2].size, ImageSize::Absolute1D { width: 256 });
        assert!(
            p.images[2].clear,
            "Boolean.parseBoolean is case-insensitive"
        );
        assert_eq!(codes(&d), vec!["props.bad-value", "props.bad-value"]);
    }

    #[test]
    fn image_limit() {
        let text: String = (0..18)
            .map(|i| format!("image.i{i}=none RGBA RGBA8 UNSIGNED_BYTE true false 4 4\n"))
            .collect();
        let (p, d) = parse_text(&text, "");
        assert_eq!(p.images.len(), MAX_CUSTOM_IMAGES);
        assert_eq!(
            codes(&d),
            vec!["props.too-many-images", "props.too-many-images"]
        );
    }

    #[test]
    fn buffer_objects() {
        let (p, d) = parse_text(
            "bufferObject.0=1024\nbufferObject.1=4096 data/init.bin\nbufferObject.2=16 true 0.5 0.5\nbufferObject.3=16 false 1 1\n\
             bufferObject.4=0\nbufferObject.13=64\nbufferObject.x=1",
            "",
        );
        assert_eq!(p.buffer_objects.len(), 4);
        assert_eq!(
            p.buffer_objects[&0],
            StorageBuffer {
                index: 0,
                size: 1024,
                relative: None,
                file: None
            }
        );
        assert_eq!(p.buffer_objects[&1].file.as_deref(), Some("data/init.bin"));
        assert_eq!(p.buffer_objects[&2].relative, Some([0.5, 0.5]));
        assert_eq!(p.buffer_objects[&3].relative, None);
        assert!(
            !p.buffer_objects.contains_key(&4),
            "size < 1 disables the buffer"
        );
        assert_eq!(p.storage_buffers().len(), 4);
        assert_eq!(codes(&d), vec!["props.bad-value", "props.bad-value"]);
        assert!(d.has_errors());
    }

    #[test]
    fn custom_uniforms_keep_order_and_first_definition() {
        let (p, d) = parse_text(
            "variable.float.dayFactor=sin(sunAngle * 2 * pi)\nuniform.vec3.sunDir=vec3(0, 1, 0)\nuniform.bool.isDay=dayFactor > 0\n\
             uniform.int.dayFactor=1\nuniform.mat4.bad=x\nuniform.float.too.many=1\nuniform.float.empty=",
            "",
        );
        let names: Vec<&str> = p.custom_uniforms.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, vec!["dayFactor", "sunDir", "isDay"]);
        assert!(p.custom_uniforms[0].is_variable);
        assert_eq!(p.custom_uniforms[0].ty, GlslType::FLOAT);
        assert_eq!(p.custom_uniforms[1].ty, GlslType::VEC3);
        assert_eq!(p.custom_uniforms[1].expression, "vec3(0, 1, 0)");
        assert_eq!(p.custom_uniforms[2].ty, GlslType::BOOL);
        assert_eq!(
            codes(&d),
            vec![
                "props.duplicate-uniform",
                "props.bad-value",
                "props.bad-value",
                "props.bad-value"
            ]
        );
    }

    #[test]
    fn raw_gui_keys() {
        let raw = "screen=<profile> <empty> [LIGHTING] SHADOWS *\nscreen.columns=2\nscreen.LIGHTING=AMBIENT [DEEPER]\n\
                   screen.LIGHTING.columns=3\nscreen.DEEPER.columns=1\nsliders=AMBIENT shadowDistance\n\
                   profile.LOW=!SHADOWS shadowDistance=64 profile.BASE\nprofile.BASE=AMBIENT:2\n\
                   iris.features.required=SSBO  CUSTOM_IMAGES\niris.features.optional=COMPUTE_SHADERS\nscreen.columns=x";
        let (p, d) = parse_text("screen=IGNORED\nsliders=IGNORED", raw);
        assert_eq!(
            p.main_screen,
            Some(vec![
                ScreenEntry::Profile,
                ScreenEntry::Empty,
                ScreenEntry::Screen("LIGHTING".into()),
                ScreenEntry::Option("SHADOWS".into()),
                ScreenEntry::Rest,
            ])
        );
        assert_eq!(
            p.main_screen_columns, None,
            "the later invalid value replaced 2"
        );
        assert_eq!(
            p.screens["LIGHTING"],
            vec![
                ScreenEntry::Option("AMBIENT".into()),
                ScreenEntry::Screen("DEEPER".into())
            ]
        );
        assert_eq!(p.screen_columns["LIGHTING"], 3);
        assert_eq!(p.sliders, vec!["AMBIENT", "shadowDistance"]);
        assert_eq!(
            p.profiles["LOW"],
            vec!["!SHADOWS", "shadowDistance=64", "profile.BASE"]
        );
        assert_eq!(p.features_required, vec!["SSBO", "CUSTOM_IMAGES"]);
        assert_eq!(p.features_optional, vec!["COMPUTE_SHADERS"]);
        assert!(p.functional.is_empty(), "GUI keys are not functional");
        assert_eq!(codes(&d), vec!["props.bad-value"]);
        let (main, cols, screens) = p.screens_model();
        assert_eq!(main.len(), 5);
        assert_eq!(cols, None);
        assert_eq!(screens["LIGHTING"].columns, Some(3));
        assert_eq!(screens["DEEPER"].entries, Vec::<ScreenEntry>::new());
    }

    #[test]
    fn unknown_keys_are_reported_as_info() {
        let (p, d) = parse_text("version.1.20.1=J1\nsomethingNew=1", "");
        assert_eq!(p.functional.len(), 2);
        assert!(d.iter().all(|d| d.severity == sb_core::Severity::Info));
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn unknown_program_names_are_info() {
        let (_, d) = parse_text(
            "alphaTest.clrwl_gbuffers=GREATER 0.1\nscale.composite100=0.5\nprogram.world0/deferred4_a.enabled=X\nflip.deferred_pre.colortex1=true",
            "",
        );
        assert_eq!(
            codes(&d),
            vec!["props.unknown-program", "props.unknown-program"]
        );
    }
}
