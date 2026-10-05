//! The CompiledPack model: a complete, host-agnostic description of a translated
//! shader pack (contract — see docs/ARCHITECTURE.md §7).
//!
//! Serialized as JSON. Binary payloads (SPIR-V, GLSL text) are stored in a
//! [`BlobTable`] and referenced by [`BlobId`] so that the JSON stays small; the JNI
//! layer ships the JSON plus one concatenated byte buffer.

use crate::diag::Diagnostics;
use crate::format::TextureFormat;
use crate::glsl_type::GlslType;
use crate::program::{AlphaFunc, BlendMode, GeometryProgram, PassGroup};
use crate::stage::ShaderStage;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------------
// Blobs
// ---------------------------------------------------------------------------------

/// Index into [`BlobTable::blobs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlobId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlobKind {
    /// SPIR-V words, little-endian.
    Spirv,
    /// UTF-8 GLSL source.
    Glsl,
    /// Raw bytes (e.g. custom texture payloads).
    Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobInfo {
    pub kind: BlobKind,
    /// Byte offset into the concatenated blob buffer (filled in by [`BlobTable::concat`]).
    pub offset: u64,
    pub len: u64,
}

/// Binary payloads referenced from the model.
#[derive(Debug, Clone, Default)]
pub struct BlobTable {
    pub blobs: Vec<(BlobKind, Vec<u8>)>,
}

impl BlobTable {
    pub fn push(&mut self, kind: BlobKind, data: Vec<u8>) -> BlobId {
        self.blobs.push((kind, data));
        BlobId((self.blobs.len() - 1) as u32)
    }
    pub fn push_spirv(&mut self, words: &[u32]) -> BlobId {
        let mut bytes = Vec::with_capacity(words.len() * 4);
        for w in words {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        self.push(BlobKind::Spirv, bytes)
    }
    pub fn push_glsl(&mut self, src: impl Into<String>) -> BlobId {
        self.push(BlobKind::Glsl, src.into().into_bytes())
    }
    pub fn get(&self, id: BlobId) -> Option<&[u8]> {
        self.blobs.get(id.0 as usize).map(|(_, b)| b.as_slice())
    }
    pub fn get_spirv(&self, id: BlobId) -> Option<Vec<u32>> {
        let b = self.get(id)?;
        if b.len() % 4 != 0 {
            return None;
        }
        Some(b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    }
    pub fn get_str(&self, id: BlobId) -> Option<&str> {
        std::str::from_utf8(self.get(id)?).ok()
    }
    /// Concatenate all blobs (8-byte aligned) and return the index + buffer.
    pub fn concat(&self) -> (Vec<BlobInfo>, Vec<u8>) {
        let mut infos = Vec::with_capacity(self.blobs.len());
        let mut buf = Vec::new();
        for (kind, data) in &self.blobs {
            while buf.len() % 8 != 0 {
                buf.push(0);
            }
            infos.push(BlobInfo { kind: *kind, offset: buf.len() as u64, len: data.len() as u64 });
            buf.extend_from_slice(data);
        }
        (infos, buf)
    }
    /// Rebuild a table from [`BlobTable::concat`] output.
    pub fn from_concat(infos: &[BlobInfo], buf: &[u8]) -> Option<Self> {
        let mut blobs = Vec::with_capacity(infos.len());
        for i in infos {
            let start = usize::try_from(i.offset).ok()?;
            let end = start.checked_add(usize::try_from(i.len).ok()?)?;
            blobs.push((i.kind, buf.get(start..end)?.to_vec()));
        }
        Some(Self { blobs })
    }
}

// ---------------------------------------------------------------------------------
// Top level
// ---------------------------------------------------------------------------------

/// A fully compiled shader pack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledPack {
    pub format_version: u32,
    pub info: PackInfo,
    pub options: OptionsModel,
    pub id_maps: IdMaps,
    /// One entry per world folder that has programs (`world0`, `world-1`, `world1`,
    /// custom folders from `dimension.properties`), plus `""` for the root program set.
    pub dimensions: Vec<DimensionPipeline>,
    /// Pack-wide diagnostics (per-program diagnostics are also here, tagged with `program`).
    pub diagnostics: Diagnostics,
    /// Index of blobs in the concatenated blob buffer (filled when exporting).
    #[serde(default)]
    pub blobs: Vec<BlobInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackInfo {
    pub name: String,
    /// blake3 hash of all pack files + options + environment (cache key).
    pub source_hash: String,
    pub shaderbridge_version: String,
    /// Feature flags declared by the pack (`iris.features.required` / `.optional`) that are
    /// supported and enabled.
    pub features_enabled: Vec<String>,
    /// Required feature flags that are NOT supported (the pack should not be enabled).
    pub features_unsupported: Vec<String>,
    /// The compile environment the pack was compiled for.
    pub environment: CompileEnvironment,
}

/// Host environment that affects compilation (macros, feature availability).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileEnvironment {
    /// Minecraft version string, e.g. `26.3` (MC_VERSION = 260300).
    pub minecraft_version: String,
    /// `MC_OS_*` suffix: `WINDOWS`, `MAC`, `LINUX`, `UNKNOWN`.
    pub os: String,
    /// `MC_GL_VENDOR_*` suffix (e.g. `NVIDIA`, `AMD`, `INTEL`, `MESA`, `OTHER`).
    pub vendor: String,
    /// `MC_GL_RENDERER_*` suffix (e.g. `GEFORCE`, `RADEON`, `INTEL`, `GALLIUM`, `OTHER`).
    pub renderer: String,
    /// Distant Horizons is loaded and rendering (defines `DISTANT_HORIZONS`).
    pub distant_horizons: bool,
    /// Extra user/host macros (`name` -> optional value).
    #[serde(default)]
    pub extra_macros: IndexMap<String, Option<String>>,
    /// Output target(s) to generate.
    pub targets: Vec<OutputTarget>,
    /// Depth convention of the host (ARCHITECTURE §4).
    pub depth_mode: DepthMode,
    /// Device capabilities relevant to translation.
    pub device: DeviceCaps,
}

impl Default for CompileEnvironment {
    fn default() -> Self {
        Self {
            minecraft_version: "26.3".into(),
            os: "LINUX".into(),
            vendor: "OTHER".into(),
            renderer: "OTHER".into(),
            distant_horizons: true,
            extra_macros: IndexMap::new(),
            targets: vec![OutputTarget::Vulkan, OutputTarget::Renderpearl],
            depth_mode: DepthMode::ForwardZeroToOne,
            device: DeviceCaps::default(),
        }
    }
}

/// Depth convention the translated shaders are generated for (ARCHITECTURE §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepthMode {
    /// NDC z in [0,1], near -> 0. Host: LESS/LEQUAL, clear 1.0.
    ForwardZeroToOne,
    /// NDC z in [0,1], near -> 1 (Minecraft 26.2+, DH 3.3+). Host: GEQUAL, clear 0.0.
    ReversedZeroToOne,
    /// GL default clip control, NDC z in [-1,1]; no remap.
    GlNegOneToOne,
}

/// Which GLSL/SPIR-V flavour to emit (ARCHITECTURE §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputTarget {
    /// Explicit set/binding decorations; SPIR-V produced by ShaderBridge.
    Vulkan,
    /// Mojang renderpearl conventions: no set/binding, names are the interface.
    Renderpearl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCaps {
    pub geometry_shader: bool,
    pub tessellation_shader: bool,
    pub storage_image_read_without_format: bool,
    pub storage_image_write_without_format: bool,
    /// `VK_EXT_depth_clip_control` available (host may skip the depth remap epilogue).
    pub depth_clip_control: bool,
    pub max_push_constants_size: u32,
    pub max_color_attachments: u32,
    /// The host can create depth-comparison samplers. When false (Mojang's renderpearl
    /// GpuSampler on 26.3), `sampler2DShadow` lookups are emulated in the shader.
    #[serde(default = "default_true")]
    pub comparison_samplers: bool,
    /// Maximum descriptors (samplers + buffers) one program may use, if the host has a hard
    /// limit (Mojang 26.3 push descriptors: 32). Programs above it get a diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_descriptors_per_program: Option<u32>,
}

fn default_true() -> bool {
    true
}

impl Default for DeviceCaps {
    fn default() -> Self {
        Self {
            geometry_shader: true,
            tessellation_shader: true,
            storage_image_read_without_format: true,
            storage_image_write_without_format: true,
            depth_clip_control: false,
            max_push_constants_size: 128,
            max_color_attachments: 8,
            comparison_samplers: true,
            max_descriptors_per_program: None,
        }
    }
}

// ---------------------------------------------------------------------------------
// Options (for GUI)
// ---------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OptionsModel {
    pub options: Vec<PackOption>,
    /// `screen` (main screen) entries; `screen.<NAME>` sub-screens in `screens`.
    pub main_screen: Vec<ScreenEntry>,
    pub main_screen_columns: Option<u32>,
    pub screens: IndexMap<String, Screen>,
    /// Options shown as sliders.
    pub sliders: Vec<String>,
    /// `profile.<NAME>` definitions: profile name -> settings (option name -> value string).
    pub profiles: IndexMap<String, IndexMap<String, String>>,
    /// Detected active profile, if current values match one.
    pub current_profile: Option<String>,
    /// Programs each profile disables (`!program.[dim/]name` entries): profile -> program paths.
    #[serde(default)]
    pub profile_disabled_programs: IndexMap<String, Vec<String>>,
    /// Lang strings for the selected language (fallback en_us): key -> text.
    pub lang: IndexMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackOption {
    pub name: String,
    pub kind: OptionKind,
    /// Default value as written in the pack (`true`/`false` for booleans).
    pub default: String,
    /// Current value (after user settings).
    pub value: String,
    /// Allowed values (value options); empty for booleans.
    pub allowed: Vec<String>,
    /// Comment text from the source line, if any.
    pub comment: Option<String>,
    /// File where the option was found (relative to shaders/).
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionKind {
    /// `#define NAME` toggled by `//`.
    BooleanDefine,
    /// `#define NAME VALUE // [a b c]`.
    ValueDefine,
    /// `const <type> NAME = VALUE; // [a b c]` or whitelisted `const bool`.
    Const,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Screen {
    pub entries: Vec<ScreenEntry>,
    pub columns: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum ScreenEntry {
    Option(String),
    /// `[NAME]` link to sub-screen.
    Screen(String),
    /// `<profile>` selector.
    Profile,
    /// `<empty>` spacer.
    Empty,
    /// `*` = all remaining options.
    Rest,
}

// ---------------------------------------------------------------------------------
// ID maps
// ---------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdMaps {
    /// `block.<id>=` entries: id -> raw entries (e.g. `minecraft:oak_leaves`, `stone`,
    /// `minecraft:wheat:age=7`, `%minecraft:logs`).
    pub blocks: IndexMap<i32, Vec<String>>,
    pub items: IndexMap<i32, Vec<String>>,
    pub entities: IndexMap<i32, Vec<String>>,
    /// `layer.<solid|cutout|cutout_mipped|translucent>=` overrides.
    pub layers: IndexMap<String, Vec<String>>,
    /// dimension folder -> dimension ids (`dimension.properties`), `*` = wildcard.
    pub dimensions: IndexMap<String, Vec<String>>,
}

// ---------------------------------------------------------------------------------
// Dimension pipeline
// ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionPipeline {
    /// World folder name (`world0`, `world-1`, ...) or `""` for the pack root.
    pub folder: String,
    /// Dimension ids this pipeline applies to (`minecraft:overworld`, `*` wildcard).
    pub dimension_ids: Vec<String>,
    pub targets: RenderTargets,
    pub settings: PackSettings,
    pub uniforms: UniformLayout,
    pub custom_uniforms: Vec<CustomUniform>,
    pub bindings: BindingTable,
    pub programs: Vec<Program>,
    /// Geometry program -> index into `programs` (after fallback resolution). Programs
    /// that are absent (whole chain missing) are not present; the host renders that
    /// geometry unshaded (vanilla) writing to `settings.fallback_tex`.
    pub geometry: IndexMap<GeometryProgram, GeometrySlot>,
    pub passes: Vec<Pass>,
    /// Color attachments of the shared gbuffers render pass: the sorted union of every
    /// gbuffers/DH program's `draw_buffers` (colortex indices, at most 8). Gbuffers programs
    /// write logical output `i` to attachment slot `output_slots[i]` of this list, so all
    /// world geometry can be drawn in ONE render pass without switching attachments.
    /// Empty if the union exceeds the attachment limit (hosts then use per-program passes
    /// and `output_slots` is the identity).
    pub gbuffer_attachments: Vec<u32>,
    /// Same for the shadow pass (shadowcolor indices).
    pub shadow_attachments: Vec<u32>,
    /// End-of-frame copies alt -> main for colortex buffers flipped an odd number of times
    /// (and not cleared), as in Iris.
    pub end_of_frame_copies: Vec<u32>,
    pub distant_horizons: DhPipeline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeometrySlot {
    /// Index into `programs` of the program drawing this geometry with the slot's default
    /// draw profile.
    pub program: u32,
    /// The program actually used (may be a fallback).
    pub resolved_from: GeometryProgram,
    /// The same program (same name and kind) translated for other draw profiles: draw
    /// profile id -> index into `programs`. Lists every other profile the compile produced
    /// it for (the variants other slots needed and the `profile_overrides` the host
    /// requested), each with the `use_alt` of this slot's pass, so a host drawing this
    /// geometry through another draw path (Sodium, a moving-block renderer, ...) finds its
    /// program here. Variants compiled later on demand (`compile_variant`) are not listed.
    #[serde(default)]
    pub variants: IndexMap<String, u32>,
}

/// Render target configuration (from const directives + shaders.properties).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderTargets {
    /// colortex0..N (only indices that any program reads or writes are `used`).
    pub colortex: Vec<ColorTarget>,
    pub shadowcolor: Vec<ColorTarget>,
    pub shadow: ShadowSettings,
    /// Any program samples depthtex1 / depthtex2.
    pub uses_depthtex1: bool,
    pub uses_depthtex2: bool,
    pub noise_texture_resolution: u32,
    /// `texture.noise=` override (pack image or resource location), if any.
    pub noise_texture: Option<TextureSource>,
    pub custom_textures: Vec<CustomTexture>,
    pub images: Vec<CustomImage>,
    pub buffers: Vec<StorageBuffer>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorTarget {
    pub index: u32,
    pub format: TextureFormat,
    pub clear: bool,
    /// `None` = default clear color (colortex0: fog color/alpha 1, colortex1: 1.0, others 0).
    pub clear_color: Option<[f32; 4]>,
    /// Programs that request mipmaps for this target before running (program indices).
    pub mipmap_programs: Vec<u32>,
    pub size: TargetSize,
    pub used: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum TargetSize {
    /// Relative to the screen size.
    Relative { x: f32, y: f32 },
    /// Absolute pixels.
    Absolute { width: u32, height: u32 },
    /// Mixed per-axis sizes (`size.buffer.colortexN = 0.5 64` is valid in Iris).
    PerAxis { x: AxisSize, y: AxisSize },
}

/// One axis of a [`TargetSize::PerAxis`] size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum AxisSize {
    /// Fraction of the screen extent.
    Relative(f32),
    /// Pixels.
    Absolute(u32),
}

impl TargetSize {
    /// Resolve to pixels for a screen of `width` x `height` (at least 1x1).
    pub fn resolve(&self, width: u32, height: u32) -> (u32, u32) {
        let rel = |f: f32, e: u32| ((e as f32 * f).ceil() as u32).max(1);
        let axis = |a: AxisSize, e: u32| match a {
            AxisSize::Relative(f) => rel(f, e),
            AxisSize::Absolute(p) => p.max(1),
        };
        match *self {
            TargetSize::Relative { x, y } => (rel(x, width), rel(y, height)),
            TargetSize::Absolute { width: w, height: h } => (w.max(1), h.max(1)),
            TargetSize::PerAxis { x, y } => (axis(x, width), axis(y, height)),
        }
    }
}

impl Default for TargetSize {
    fn default() -> Self {
        TargetSize::Relative { x: 1.0, y: 1.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowSettings {
    pub enabled: bool,
    pub resolution: u32,
    /// `None` = orthographic.
    pub fov: Option<f32>,
    pub distance: f32,
    pub near_plane: f32,
    pub far_plane: f32,
    pub distance_render_mul: f32,
    pub entity_distance_mul: f32,
    pub interval_size: f32,
    pub voxel_distance: f32,
    /// [shadowtex0, shadowtex1]
    pub hardware_filtering: [bool; 2],
    pub mipmap: [bool; 2],
    pub nearest: [bool; 2],
    pub color_mipmap: Vec<bool>,
    pub color_nearest: Vec<bool>,
    pub culling: String,
    pub render_terrain: bool,
    pub render_translucent: bool,
    pub render_entities: bool,
    pub render_player: bool,
    pub render_block_entities: bool,
    pub render_light_block_entities: bool,
    pub dh_shadow_enabled: bool,
}

impl Default for ShadowSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            resolution: 1024,
            fov: None,
            distance: 160.0,
            near_plane: 0.05,
            far_plane: 256.0,
            distance_render_mul: -1.0,
            entity_distance_mul: 1.0,
            interval_size: 2.0,
            voxel_distance: 0.0,
            hardware_filtering: [false, false],
            mipmap: [false, false],
            nearest: [false, false],
            color_mipmap: vec![false; 8],
            color_nearest: vec![false; 8],
            culling: "default".into(),
            render_terrain: true,
            render_translucent: true,
            render_entities: true,
            render_player: false,
            render_block_entities: true,
            render_light_block_entities: false,
            dh_shadow_enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomTexture {
    /// Sampler name in GLSL (after `.0-.9` suffix stripping).
    pub sampler: String,
    /// Texture stage (`gbuffers`, `composite`, ... ) or `custom` for `customTexture.<name>`.
    pub stage: String,
    pub source: TextureSource,
    pub blur: bool,
    pub clamp: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum TextureSource {
    /// PNG (or other image) inside the pack, path relative to shaders/.
    PackImage { path: String },
    /// A resource-pack / vanilla texture (`minecraft:textures/...`).
    Resource { location: String },
    /// Dynamic vanilla texture (`minecraft:dynamic/lightmap_1`).
    Dynamic { name: String },
    /// Raw binary texture.
    Raw {
        path: String,
        /// Texture target: `1d`, `2d`, `3d` or `2d_rect` (`TEXTURE_RECTANGLE`).
        target: String,
        dimensions: u8,
        format: TextureFormat,
        size: [u32; 3],
        pixel_format: String,
        pixel_type: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomImage {
    pub name: String,
    pub sampler_name: Option<String>,
    pub format: TextureFormat,
    pub pixel_format: String,
    pub pixel_type: String,
    pub clear: bool,
    pub size: ImageSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ImageSize {
    Relative { x: f32, y: f32 },
    Absolute1D { width: u32 },
    Absolute2D { width: u32, height: u32 },
    Absolute3D { width: u32, height: u32, depth: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageBuffer {
    pub index: u32,
    /// Size in bytes (or bytes per pixel when `relative`).
    pub size: u64,
    pub relative: Option<[f32; 2]>,
    /// Initial content file (relative to shaders/), if any.
    pub file: Option<String>,
}

/// Functional `shaders.properties` keys and global const directives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackSettings {
    pub clouds: String,
    pub dh_clouds: String,
    pub old_hand_light: bool,
    pub dynamic_hand_light: bool,
    pub old_lighting: bool,
    pub separate_ao: bool,
    pub underwater_overlay: bool,
    pub vignette: bool,
    pub sun: bool,
    pub moon: bool,
    pub stars: bool,
    pub sky: bool,
    pub weather: bool,
    pub weather_particles: bool,
    pub rain_depth: bool,
    pub beacon_beam_depth: bool,
    pub frustum_culling: bool,
    pub occlusion_culling: bool,
    pub separate_entity_draws: bool,
    pub allow_concurrent_compute: bool,
    pub particles_ordering: String,
    pub supports_color_correction: bool,
    pub skip_all_rendering: bool,
    pub voxelize_light_blocks: bool,
    pub end_flash_shadows: bool,
    /// colortex index unshaded geometry writes to.
    pub fallback_tex: u32,
    /// `backFace.solid/cutout/cutoutMipped/translucent` overrides.
    pub back_face: IndexMap<String, bool>,
    pub sun_path_rotation: f32,
    pub ambient_occlusion_level: f32,
    pub wetness_half_life: f32,
    pub dryness_half_life: f32,
    pub eye_brightness_half_life: f32,
    pub center_depth_half_life: f32,
    /// Every raw functional key/value (preprocessed text) for hosts that need more.
    pub raw: IndexMap<String, String>,
}

impl Default for PackSettings {
    fn default() -> Self {
        Self {
            clouds: "default".into(),
            dh_clouds: "default".into(),
            old_hand_light: true,
            dynamic_hand_light: true,
            old_lighting: false,
            separate_ao: false,
            underwater_overlay: false,
            vignette: false,
            sun: true,
            moon: true,
            stars: true,
            sky: true,
            weather: true,
            weather_particles: true,
            rain_depth: false,
            beacon_beam_depth: false,
            frustum_culling: true,
            occlusion_culling: true,
            separate_entity_draws: false,
            allow_concurrent_compute: false,
            particles_ordering: "after".into(),
            supports_color_correction: false,
            skip_all_rendering: false,
            voxelize_light_blocks: false,
            end_flash_shadows: false,
            fallback_tex: 0,
            back_face: IndexMap::new(),
            sun_path_rotation: 0.0,
            ambient_occlusion_level: 1.0,
            wetness_half_life: 600.0,
            dryness_half_life: 200.0,
            eye_brightness_half_life: 10.0,
            center_depth_half_life: 1.0,
            raw: IndexMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------------
// Uniforms
// ---------------------------------------------------------------------------------

/// The pack-global uniform blocks (ARCHITECTURE §5.1).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UniformLayout {
    /// `sb_Frame` (set 0, binding 0): per-frame values.
    pub frame: BlockLayout,
    /// `sb_Draw` (set 0, binding 1): per-draw values.
    pub draw: BlockLayout,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockLayout {
    pub name: String,
    pub set: u32,
    pub binding: u32,
    /// Total std140 size in bytes (multiple of 16).
    pub size: u32,
    pub members: Vec<BlockMember>,
}

impl BlockLayout {
    pub fn member(&self, name: &str) -> Option<&BlockMember> {
        self.members.iter().find(|m| m.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockMember {
    /// GLSL member name (equals the pack's uniform name).
    pub name: String,
    pub ty: GlslType,
    pub offset: u32,
    pub source: UniformSource,
    /// Constant default from a `uniform T x = init;` initializer: unpadded component values
    /// in GLSL constructor order (column-major for matrices, element by element for arrays),
    /// `rows * cols * array_len` entries. Hosts pad them into std140 when uploading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Vec<f32>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "name")]
pub enum UniformSource {
    /// A builtin uniform provided by the host (name in the sb-uniforms registry).
    Builtin(String),
    /// A custom uniform defined in shaders.properties (`uniform.<type>.<name>`).
    Custom(String),
    /// Declared by the pack but unknown: zero-filled.
    Unset,
}

/// `uniform.<type>.<name>=<expr>` / `variable.<type>.<name>=<expr>` from shaders.properties.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomUniform {
    pub name: String,
    pub ty: GlslType,
    pub expression: String,
    /// `variable.*` (not uploaded, usable by later expressions) vs `uniform.*`.
    pub is_variable: bool,
    /// Where it was defined (`shaders.properties` line), for diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<crate::diag::SourceLocation>,
}

// ---------------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------------

/// Pack-global resource name -> binding (ARCHITECTURE §5.2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingTable {
    pub entries: Vec<BindingEntry>,
}

impl BindingTable {
    pub fn get(&self, name: &str) -> Option<&BindingEntry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingEntry {
    /// Canonical GLSL name in translated shaders.
    pub name: String,
    pub set: u32,
    pub binding: u32,
    pub kind: ResourceKind,
    pub resource: ResourceRef,
}

/// Descriptor kind of a binding.
///
/// In a compiled pack's [`BindingTable`] it describes what the translated shaders
/// declare, i.e. what the host must bind: rectangle samplers (`sampler2DRect`, ...) are
/// declared as 2D samplers (dim `2d`, Vulkan has no rectangle textures), and comparison
/// samplers the translator emulates (`sampler2DShadow` with
/// [`DeviceCaps::comparison_samplers`] false) are plain samplers (`shadow` false, see
/// [`BindingUse::shadow_emulated`]). The SPIR-V reflection of every module agrees with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ResourceKind {
    /// Combined image sampler.
    Sampler {
        /// `1d`, `1d_array`, `2d`, `2d_array`, `3d`, `cube`, `cube_array`, `buffer`,
        /// `2d_ms`, `2d_ms_array` (and `2d_rect` for declarations before translation).
        dim: String,
        /// Depth-comparison sampler: the shaders declare a `sampler*Shadow` and the host
        /// binds a sampler with comparison enabled (compare op of the depth mode). False
        /// means a plain, non-comparison sampler, also for emulated comparisons.
        shadow: bool,
        /// `float`, `int`, `uint`.
        sample_type: String,
    },
    StorageImage {
        dim: String,
        /// GLSL format qualifier (`rgba16f`), if declared.
        format: Option<String>,
        sample_type: String,
        readonly: bool,
        writeonly: bool,
    },
    StorageBuffer,
    UniformBuffer,
}

/// What the host must bind for a resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum ResourceRef {
    ColorTex(u32),
    DepthTex(u32),
    ShadowTex(u32),
    /// Hardware-filtering variant (`shadowtex0HW`).
    ShadowTexHw(u32),
    ShadowColor(u32),
    Noise,
    Atlas,
    Lightmap,
    Normals,
    Specular,
    Overlay,
    DhDepthTex(u32),
    DhBlockAtlas,
    /// Constant 1x1 white texture (Iris binds it for the albedo/overlay samplers of programs
    /// whose geometry has no texture, e.g. Distant Horizons programs).
    White,
    /// A custom texture. Payload: `"<stage>.<sampler>"` for image textures and
    /// `customTexture.*` (stage `custom`), `"<stage>.<sampler>.<dim>"` for raw textures
    /// (dim `1d`/`2d`/`3d`/`2d_rect`); see sb-uniforms `custom_texture_id`.
    CustomTexture(String),
    Image(String),
    ColorImage(u32),
    ShadowColorImage(u32),
    Ssbo(u32),
    UniformBlock(String),
    /// Unknown sampler: GL unit-0 semantics (atlas in gbuffers/shadow, colortex0 in
    /// fullscreen passes).
    Unknown(String),
}

// ---------------------------------------------------------------------------------
// Programs
// ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    /// `world0/gbuffers_terrain`, `composite3`, `composite3_b` (compute), ...
    pub name: String,
    pub kind: ProgramKind,
    /// Draw profile id the program was translated for (`fullscreen`, `vanilla_terrain`,
    /// `dh_terrain`, ...). `None` for compute programs.
    pub draw_profile: Option<String>,
    /// Needs features Mojang's public pipeline API cannot express (1D/3D textures,
    /// storage images, SSBOs, compute, geometry/tessellation).
    pub requires_raw_vulkan: bool,
    pub stages: Vec<StageModule>,
    /// `RENDERTARGETS` / `DRAWBUFFERS`: logical fragment output i writes target `draw_buffers[i]`.
    pub draw_buffers: Vec<u32>,
    /// Physical fragment output location of logical output i (equal to i unless the program
    /// was remapped onto the shared `gbuffer_attachments` / `shadow_attachments` list, in which
    /// case `output_slots[i]` is the index of `draw_buffers[i]` in that list).
    pub output_slots: Vec<u32>,
    /// Output base type per location (`float`, `int`, `uint`).
    pub output_types: Vec<String>,
    pub blend: Option<BlendMode>,
    /// Per-buffer blend overrides: target index -> blend (None = off).
    pub blend_per_buffer: IndexMap<u32, Option<BlendMode>>,
    pub alpha_test: Option<AlphaTest>,
    /// `scale.<prog>`: viewport scale and offset.
    pub viewport: ViewportScale,
    /// Targets whose mipmaps must be generated before this program runs.
    pub mipmap_targets: Vec<u32>,
    pub bindings_used: Vec<BindingUse>,
    pub vertex_inputs: Vec<VertexInput>,
    pub push_constant_size: u32,
    pub compute: Option<ComputeInfo>,
    /// Cull back faces (None = host default for the geometry type).
    pub cull: Option<bool>,
    /// Program was synthesized (e.g. DH program generated from gbuffers_terrain).
    pub synthesized_from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ProgramKind {
    Geometry { program: GeometryProgram },
    Composite { group: PassGroup, index: u8 },
    /// Compute shader attached to a pass (`composite3_b.csh`), `letter` = None for `composite3.csh`.
    Compute { group: PassGroup, index: u8, letter: Option<char> },
    /// Compute shader of a geometry pass (`shadow.csh`, `shadow_a.csh`).
    GeometryCompute { program: GeometryProgram, letter: Option<char> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageModule {
    pub stage: ShaderStage,
    pub entry_point: String,
    /// SPIR-V for [`OutputTarget::Vulkan`].
    pub spirv: Option<BlobId>,
    /// Translated GLSL for [`OutputTarget::Vulkan`] (debugging, host recompilation).
    pub glsl_vulkan: Option<BlobId>,
    /// Translated GLSL for [`OutputTarget::Renderpearl`].
    pub glsl_renderpearl: Option<BlobId>,
    /// Original pack file (relative to shaders/).
    pub source_file: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AlphaTest {
    pub func: AlphaFunc,
    pub reference: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ViewportScale {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl Default for ViewportScale {
    fn default() -> Self {
        Self { scale: 1.0, offset_x: 0.0, offset_y: 0.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingUse {
    /// Canonical resource name (key into the dimension's [`BindingTable`]).
    pub name: String,
    pub set: u32,
    pub binding: u32,
    /// For `ColorTex`/`ColorImage` in composite-style programs: read the alt buffer
    /// (true) or main (false) according to the static flip schedule.
    pub use_alt: bool,
    pub stages: Vec<ShaderStage>,
    /// The pack declares this sampler as a depth-comparison sampler (`sampler2DShadow`,
    /// `sampler2DRectShadow`) but the program compares in the shader because the host has
    /// no comparison samplers ([`DeviceCaps::comparison_samplers`] false): the translated
    /// shaders declare a plain `sampler2D` (a 2×2 percentage-closer filter over
    /// `textureGather`), so the host binds the depth texture with a plain, non-comparison
    /// sampler. The binding's [`ResourceKind::Sampler`] has `shadow: false` accordingly;
    /// this flag only records why. False in models written before emulation existed.
    #[serde(default)]
    pub shadow_emulated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VertexInput {
    pub location: u32,
    /// Attribute name (equals the host vertex format element name for renderpearl).
    pub name: String,
    /// GLSL type (`vec3`, `uvec3`, ...).
    pub ty: String,
    /// Semantic it feeds (`position`, `color`, `uv0`, ...), if known.
    pub semantic: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputeInfo {
    pub local_size: [u32; 3],
    pub work_groups: WorkGroups,
    /// `indirect.<prog>=<ssbo> <offset>`.
    pub indirect: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum WorkGroups {
    /// `const ivec3 workGroups = ivec3(x,y,z);`
    Absolute { x: u32, y: u32, z: u32 },
    /// `const vec2 workGroupsRender = vec2(sx,sy);` (default 1,1): dispatch covers the screen.
    Relative { x: f32, y: f32 },
}

// ---------------------------------------------------------------------------------
// Passes
// ---------------------------------------------------------------------------------

/// One step of the frame, in execution order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pass {
    pub group: PassGroup,
    /// Index within the group (composite3 -> 3). Geometry passes use 0.
    pub index: u8,
    /// Compute programs to dispatch first (indices into `programs`), in order.
    pub computes: Vec<u32>,
    /// Fullscreen program (index into `programs`), if any.
    pub program: Option<u32>,
    /// colortex buffers whose main/alt roles are swapped after this pass.
    pub flips_after: Vec<u32>,
    /// Flip state of every colortex when this pass starts (true = alt is "main" for reading).
    pub flip_state: Vec<bool>,
}

// ---------------------------------------------------------------------------------
// Distant Horizons
// ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DhPipeline {
    pub strategy: DhStrategy,
    /// Render vanilla terrain and LODs with one projection whose far plane is the DH far
    /// plane, and report `far` accordingly (synthesized strategy).
    pub unified_projection: bool,
    pub shadow_enabled: bool,
}

impl Default for DhPipeline {
    fn default() -> Self {
        Self { strategy: DhStrategy::Disabled, unified_projection: false, shadow_enabled: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DhStrategy {
    /// Pack ships dh_* programs.
    Native,
    /// dh_* programs synthesized from gbuffers_* programs.
    Synthesized,
    /// DH not present in the environment.
    Disabled,
}

impl CompiledPack {
    /// Serialize to JSON (compact).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("CompiledPack is always serializable")
    }
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_roundtrip() {
        let mut t = BlobTable::default();
        let a = t.push_spirv(&[0x0723_0203, 1, 2, 3]);
        let b = t.push_glsl("#version 450\nvoid main(){}\n");
        let (infos, buf) = t.concat();
        assert_eq!(infos[1].offset % 8, 0);
        let t2 = BlobTable::from_concat(&infos, &buf).unwrap();
        assert_eq!(t2.get_spirv(a).unwrap(), vec![0x0723_0203, 1, 2, 3]);
        assert_eq!(t2.get_str(b).unwrap(), "#version 450\nvoid main(){}\n");
    }

    #[test]
    fn resource_ref_json_shape() {
        let j = serde_json::to_string(&ResourceRef::ColorTex(3)).unwrap();
        assert_eq!(j, r#"{"type":"color_tex","value":3}"#);
        let j = serde_json::to_string(&ResourceRef::Noise).unwrap();
        assert_eq!(j, r#"{"type":"noise"}"#);
        let j = serde_json::to_string(&UniformSource::Builtin("frameTimeCounter".into())).unwrap();
        assert_eq!(j, r#"{"type":"builtin","name":"frameTimeCounter"}"#);
    }

    /// Fields added after the first model release default when absent.
    #[test]
    fn added_fields_default_when_absent() {
        let slot: GeometrySlot = serde_json::from_str(r#"{"program":3,"resolved_from":"terrain"}"#).unwrap();
        assert_eq!(slot, GeometrySlot { program: 3, resolved_from: GeometryProgram::Terrain, variants: IndexMap::new() });
        let mut with = slot.clone();
        with.variants.insert("sodium_terrain".into(), 7);
        let j = serde_json::to_string(&with).unwrap();
        assert_eq!(j, r#"{"program":3,"resolved_from":"terrain","variants":{"sodium_terrain":7}}"#);
        assert_eq!(serde_json::from_str::<GeometrySlot>(&j).unwrap(), with);

        let used: BindingUse = serde_json::from_str(r#"{"name":"shadowtex0","set":1,"binding":4,"use_alt":false,"stages":["fragment"]}"#).unwrap();
        assert!(!used.shadow_emulated);
        let emulated = BindingUse { shadow_emulated: true, ..used };
        let j = serde_json::to_string(&emulated).unwrap();
        assert!(j.ends_with(r#""shadow_emulated":true}"#), "{j}");
        assert_eq!(serde_json::from_str::<BindingUse>(&j).unwrap(), emulated);
    }
}
