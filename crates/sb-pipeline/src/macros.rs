//! Standard preprocessor macros (Iris `StandardMacros` + `IrisDefines`, adapted to a
//! Vulkan host) and feature flags.
//!
//! Iris passes these to its preprocessor as definitions (they are never written into
//! the source); ShaderBridge does the same through `sb_preprocess::PreprocessOptions`.

use indexmap::IndexMap;
use sb_core::model::CompileEnvironment;

/// `IRIS_VERSION`: the Iris release whose behaviour ShaderBridge follows (1.11.7).
pub const IRIS_VERSION: u32 = 11107;

/// `MAX_COLOR_BUFFERS`. Iris defines 32; ShaderBridge advertises 16 because a Vulkan pass
/// can write at most 8 colour attachments and every colortex is a main/alt image pair,
/// so packs that size their effects by this macro stay within reasonable memory. Hosts
/// can override it with `CompileEnvironment::extra_macros`.
pub const MAX_COLOR_BUFFERS: u32 = 16;

/// GL extensions whose functionality Vulkan 1.2 GLSL 4.50/4.60 provides; ShaderBridge
/// defines `MC_GL_<name>` for each, so packs take the same code paths as on a modern GL
/// driver.
pub const GL_EXTENSIONS: &[&str] = &[
    "ARB_shader_texture_lod",
    "ARB_gpu_shader5",
    "ARB_shader_image_load_store",
    "ARB_compute_shader",
    "ARB_shader_storage_buffer_object",
    "ARB_texture_gather",
    "EXT_gpu_shader4",
    "ARB_explicit_attrib_location",
    "ARB_shading_language_packing",
    "ARB_shader_bit_encoding",
    "ARB_conservative_depth",
    "ARB_texture_query_levels",
    "ARB_derivative_control",
    "ARB_shader_draw_parameters",
    "ARB_separate_shader_objects",
    "ARB_shading_language_420pack",
    "ARB_enhanced_layouts",
    "ARB_texture_cube_map_array",
    "ARB_shader_group_vote",
    "ARB_gpu_shader_fp64",
    "ARB_shader_atomic_counters",
    "ARB_sample_shading",
];

/// `MC_RENDER_STAGE_*` names in Iris `WorldRenderingPhase` ordinal order.
pub const RENDER_STAGES: [&str; 24] = [
    "NONE",
    "SKY",
    "SUNSET",
    "CUSTOM_SKY",
    "SUN",
    "MOON",
    "STARS",
    "VOID",
    "TERRAIN_SOLID",
    "TERRAIN_CUTOUT_MIPPED",
    "TERRAIN_CUTOUT",
    "ENTITIES",
    "BLOCK_ENTITIES",
    "DESTROY",
    "OUTLINE",
    "DEBUG",
    "HAND_SOLID",
    "TERRAIN_TRANSLUCENT",
    "TRIPWIRE",
    "PARTICLES",
    "CLOUDS",
    "RAIN_SNOW",
    "WORLD_BORDER",
    "HAND_TRANSLUCENT",
];

/// `DH_BLOCK_*` material names (Distant Horizons `EDhApiBlockMaterial`), value = index.
pub const DH_BLOCK_MATERIALS: [&str; 16] = [
    "UNKNOWN",
    "LEAVES",
    "STONE",
    "WOOD",
    "METAL",
    "DIRT",
    "LAVA",
    "DEEPSLATE",
    "SNOW",
    "SAND",
    "TERRACOTTA",
    "NETHER_STONE",
    "WATER",
    "GRASS",
    "AIR",
    "ILLUMINATED",
];

/// Vanilla biomes of Minecraft 26.3 in registration order (`net.minecraft.world.level.biome.Biomes`
/// static initializer, read from the 26.3 client jar). Iris numbers `BIOME_<PATH>` in this
/// order (`MixinBiomes` counts `Biomes.register` calls).
pub const VANILLA_BIOMES: [&str; 67] = [
    "the_void",
    "plains",
    "sunflower_plains",
    "snowy_plains",
    "ice_spikes",
    "desert",
    "swamp",
    "mangrove_swamp",
    "forest",
    "flower_forest",
    "birch_forest",
    "dappled_forest",
    "dark_forest",
    "pale_garden",
    "old_growth_birch_forest",
    "old_growth_pine_taiga",
    "old_growth_spruce_taiga",
    "taiga",
    "snowy_taiga",
    "savanna",
    "savanna_plateau",
    "windswept_hills",
    "windswept_gravelly_hills",
    "windswept_forest",
    "windswept_savanna",
    "jungle",
    "sparse_jungle",
    "bamboo_jungle",
    "badlands",
    "eroded_badlands",
    "wooded_badlands",
    "meadow",
    "cherry_grove",
    "grove",
    "snowy_slopes",
    "frozen_peaks",
    "jagged_peaks",
    "stony_peaks",
    "river",
    "frozen_river",
    "beach",
    "snowy_beach",
    "stony_shore",
    "warm_ocean",
    "lukewarm_ocean",
    "deep_lukewarm_ocean",
    "ocean",
    "deep_ocean",
    "cold_ocean",
    "deep_cold_ocean",
    "frozen_ocean",
    "deep_frozen_ocean",
    "mushroom_fields",
    "dripstone_caves",
    "lush_caves",
    "deep_dark",
    "sulfur_caves",
    "nether_wastes",
    "warped_forest",
    "crimson_forest",
    "soul_sand_valley",
    "basalt_deltas",
    "the_end",
    "end_highlands",
    "end_midlands",
    "small_end_islands",
    "end_barrens",
];

/// Feature flags (`iris.features.required` / `iris.features.optional`) ShaderBridge can
/// support, in Iris `FeatureFlags` order. `TESSELLATION_SHADERS` additionally needs the
/// device's tessellation support (see [`is_feature_supported`]).
pub const FEATURE_FLAGS: [&str; 13] = [
    "SEPARATE_HARDWARE_SAMPLERS",
    "HIGHER_SHADOWCOLOR",
    "CUSTOM_IMAGES",
    "PER_BUFFER_BLENDING",
    "COMPUTE_SHADERS",
    "TESSELLATION_SHADERS",
    "ENTITY_TRANSLUCENT",
    "REVERSED_CULLING",
    "BLOCK_EMISSION_ATTRIBUTE",
    "CAN_DISABLE_WEATHER",
    "SSBO",
    "FADE_VARIABLE",
    "TEXTURE_FILTERING",
];

/// Canonical feature-flag name of a flag as written in `shaders.properties`
/// (case-insensitive; `TESSELATION_SHADERS` is Iris's accepted misspelling), or `None`
/// for unknown flags.
pub fn canonical_feature(flag: &str) -> Option<&'static str> {
    let upper = flag.trim().to_ascii_uppercase();
    let upper = if upper == "TESSELATION_SHADERS" { "TESSELLATION_SHADERS".to_string() } else { upper };
    FEATURE_FLAGS.iter().copied().find(|f| *f == upper)
}

/// Whether ShaderBridge supports `flag` (canonical name) in `env`.
pub fn is_feature_supported(flag: &str, env: &CompileEnvironment) -> bool {
    match canonical_feature(flag) {
        Some("TESSELLATION_SHADERS") => env.device.tessellation_shader,
        Some(_) => true,
        None => false,
    }
}

/// `MC_VERSION` of a Minecraft version string in Iris's "122" format: major, then minor
/// and patch with two digits each (`26.3` → 260300, `26.1.2` → 260102, `1.21.4` → 12104).
/// Non-numeric suffixes (`26.4-snapshot-2`) are ignored; unparsable input yields 0.
pub fn mc_version_number(version: &str) -> u32 {
    let core = version.split(|c: char| !(c.is_ascii_digit() || c == '.')).next().unwrap_or("");
    let mut parts = core.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0).min(99);
    let patch = parts.next().unwrap_or(0).min(99);
    major.saturating_mul(10_000).saturating_add(minor * 100 + patch)
}

/// Uppercase `[A-Z0-9_]` form of an environment string used in a macro name suffix.
fn macro_suffix(s: &str, fallback: &str) -> String {
    let out: String = s
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect();
    if out.is_empty() { fallback.to_string() } else { out }
}

/// The standard macros for `env` (Iris `StandardMacros.createStandardEnvironmentDefines`
/// plus `IrisDefines`), adapted to Vulkan:
///
/// * versions: `MC_VERSION`, `MC_MIPMAP_LEVEL=4`, `IRIS_VERSION`, `MC_GL_VERSION=460`,
///   `MC_GLSL_VERSION=460`;
/// * platform: `MC_OS_<os>`, `MC_GL_VENDOR_<vendor>`, `MC_GL_RENDERER_<renderer>`;
/// * identity: `IS_IRIS`, `IRIS_HAS_TRANSLUCENCY_SORTING`, `IRIS_TAG_SUPPORT=2`,
///   `MAX_COLOR_BUFFERS` (see [`MAX_COLOR_BUFFERS`]), `SHADERBRIDGE=1`,
///   `SHADERBRIDGE_VULKAN=1`;
/// * `DISTANT_HORIZONS` and `DISTANT_HORIZONS_TEXTURES` when `env.distant_horizons`;
///   `DH_BLOCK_*` always;
/// * `MC_GL_<ext>` for [`GL_EXTENSIONS`];
/// * `MC_NORMAL_MAP`, `MC_SPECULAR_MAP`, `MC_RENDER_QUALITY=1.0`, `MC_SHADOW_QUALITY=1.0`,
///   `MC_HAND_DEPTH=0.125`;
/// * `MC_RENDER_STAGE_*` ([`RENDER_STAGES`] ordinals);
/// * `BIOME_*` ([`VANILLA_BIOMES`] order), `CAT_*` and `PPT_*`;
/// * finally `env.extra_macros` (which override earlier definitions of the same name).
///
/// `None` values define a macro with an empty replacement list (Iris's flags). The
/// pack-dependent `IRIS_FEATURE_*` and `COLOR_SPACE_*` macros are not included; see
/// [`feature_macros`].
pub fn standard_macros(env: &CompileEnvironment) -> IndexMap<String, Option<String>> {
    let mut m: IndexMap<String, Option<String>> = IndexMap::new();
    let mut def = |k: String, v: Option<String>| {
        m.insert(k, v);
    };
    def("MC_VERSION".into(), Some(mc_version_number(&env.minecraft_version).to_string()));
    def("MC_MIPMAP_LEVEL".into(), Some("4".into()));
    def("IRIS_VERSION".into(), Some(IRIS_VERSION.to_string()));
    def("MC_GL_VERSION".into(), Some("460".into()));
    def("MC_GLSL_VERSION".into(), Some("460".into()));
    def(format!("MC_OS_{}", macro_suffix(&env.os, "UNKNOWN")), None);
    def(format!("MC_GL_VENDOR_{}", macro_suffix(&env.vendor, "OTHER")), None);
    def(format!("MC_GL_RENDERER_{}", macro_suffix(&env.renderer, "OTHER")), None);
    def("IS_IRIS".into(), None);
    def("MAX_COLOR_BUFFERS".into(), Some(MAX_COLOR_BUFFERS.to_string()));
    def("IRIS_HAS_TRANSLUCENCY_SORTING".into(), None);
    def("IRIS_TAG_SUPPORT".into(), Some("2".into()));
    if env.distant_horizons {
        def("DISTANT_HORIZONS".into(), None);
        def("DISTANT_HORIZONS_TEXTURES".into(), None);
    }
    for (i, name) in DH_BLOCK_MATERIALS.iter().enumerate() {
        def(format!("DH_BLOCK_{name}"), Some(i.to_string()));
    }
    for ext in GL_EXTENSIONS {
        def(format!("MC_GL_{ext}"), None);
    }
    def("MC_NORMAL_MAP".into(), None);
    def("MC_SPECULAR_MAP".into(), None);
    def("MC_RENDER_QUALITY".into(), Some("1.0".into()));
    def("MC_SHADOW_QUALITY".into(), Some("1.0".into()));
    def("MC_HAND_DEPTH".into(), Some("0.125".into()));
    for (i, name) in RENDER_STAGES.iter().enumerate() {
        def(format!("MC_RENDER_STAGE_{name}"), Some(i.to_string()));
    }
    for (i, name) in VANILLA_BIOMES.iter().enumerate() {
        def(format!("BIOME_{}", name.to_ascii_uppercase()), Some(i.to_string()));
    }
    for (name, value) in sb_expr::standard_constants() {
        if let sb_expr::Value::Int(v) = value {
            def(name, Some(v.to_string()));
        }
    }
    def("SHADERBRIDGE".into(), Some("1".into()));
    def("SHADERBRIDGE_VULKAN".into(), Some("1".into()));
    for (k, v) in &env.extra_macros {
        def(k.clone(), v.clone());
    }
    m
}

/// `BIOME_*` constants for custom-uniform expressions (`sb_expr::CustomUniforms`), plus the
/// standard `CAT_*` / `PPT_*` constants.
pub fn expression_constants() -> IndexMap<String, sb_expr::Value> {
    let mut c = sb_expr::standard_constants();
    for (i, name) in VANILLA_BIOMES.iter().enumerate() {
        c.insert(format!("BIOME_{}", name.to_ascii_uppercase()), sb_expr::Value::Int(i as i32));
    }
    c
}

/// Which `IRIS_FEATURE_*` macros a pack sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureMacroScope {
    /// `.properties` files: every supported flag (Iris defines all usable flags there).
    Properties,
    /// GLSL: only the supported flags the pack lists in `iris.features.optional`, spelled
    /// as written (Iris `ShaderPack`: `"IRIS_FEATURE_" + flag`).
    Glsl,
}

/// `IRIS_FEATURE_*` macros (empty values) for `scope`.
pub fn feature_macros(
    optional_flags: &[String],
    env: &CompileEnvironment,
    scope: FeatureMacroScope,
) -> IndexMap<String, Option<String>> {
    let mut m = IndexMap::new();
    match scope {
        FeatureMacroScope::Properties => {
            for f in FEATURE_FLAGS {
                if is_feature_supported(f, env) {
                    m.insert(format!("IRIS_FEATURE_{f}"), None);
                }
            }
        }
        FeatureMacroScope::Glsl => {
            for f in optional_flags {
                // Iris checks validity with `valueOf(flag.toUpperCase())`, so only exact
                // enum names (any case) count here; the TESSELATION alias does not.
                let upper = f.trim().to_ascii_uppercase();
                if FEATURE_FLAGS.contains(&upper.as_str()) && is_feature_supported(&upper, env) {
                    m.insert(format!("IRIS_FEATURE_{}", f.trim()), None);
                }
            }
        }
    }
    m
}

/// `COLOR_SPACE_*` macros Iris adds for packs with `supportsColorCorrection=true`.
pub fn color_space_macros() -> IndexMap<String, Option<String>> {
    ["SRGB", "DCI_P3", "DISPLAY_P3", "REC2020", "ADOBE_RGB"]
        .iter()
        .enumerate()
        .map(|(i, n)| (format!("COLOR_SPACE_{n}"), Some(i.to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mc_version_format() {
        assert_eq!(mc_version_number("26.3"), 260300);
        assert_eq!(mc_version_number("26.1.2"), 260102);
        assert_eq!(mc_version_number("1.21.4"), 12104);
        assert_eq!(mc_version_number("26.4-snapshot-2"), 260400);
        assert_eq!(mc_version_number("garbage"), 0);
        assert_eq!(mc_version_number(""), 0);
    }

    #[test]
    fn standard_macro_values() {
        let env = CompileEnvironment::default();
        let m = standard_macros(&env);
        let get = |k: &str| m.get(k).cloned();
        assert_eq!(get("MC_VERSION"), Some(Some("260300".into())));
        assert_eq!(get("IRIS_VERSION"), Some(Some("11107".into())));
        assert_eq!(get("MC_GL_VERSION"), Some(Some("460".into())));
        assert_eq!(get("MC_GLSL_VERSION"), Some(Some("460".into())));
        assert_eq!(get("MC_MIPMAP_LEVEL"), Some(Some("4".into())));
        assert_eq!(get("MAX_COLOR_BUFFERS"), Some(Some("16".into())));
        assert_eq!(get("IRIS_TAG_SUPPORT"), Some(Some("2".into())));
        assert_eq!(get("MC_OS_LINUX"), Some(None));
        assert_eq!(get("MC_GL_VENDOR_OTHER"), Some(None));
        assert_eq!(get("MC_GL_RENDERER_OTHER"), Some(None));
        assert_eq!(get("IS_IRIS"), Some(None));
        assert_eq!(get("DISTANT_HORIZONS"), Some(None));
        assert_eq!(get("DISTANT_HORIZONS_TEXTURES"), Some(None));
        assert_eq!(get("DH_BLOCK_UNKNOWN"), Some(Some("0".into())));
        assert_eq!(get("DH_BLOCK_ILLUMINATED"), Some(Some("15".into())));
        assert_eq!(get("MC_GL_ARB_shader_texture_lod"), Some(None));
        assert_eq!(get("MC_GL_ARB_sample_shading"), Some(None));
        assert_eq!(get("MC_HAND_DEPTH"), Some(Some("0.125".into())));
        assert_eq!(get("MC_RENDER_STAGE_NONE"), Some(Some("0".into())));
        assert_eq!(get("MC_RENDER_STAGE_TERRAIN_CUTOUT_MIPPED"), Some(Some("9".into())));
        assert_eq!(get("MC_RENDER_STAGE_HAND_TRANSLUCENT"), Some(Some("23".into())));
        assert_eq!(get("BIOME_THE_VOID"), Some(Some("0".into())));
        assert_eq!(get("BIOME_PLAINS"), Some(Some("1".into())));
        assert_eq!(get("BIOME_END_BARRENS"), Some(Some("66".into())));
        assert_eq!(get("CAT_NONE"), Some(Some("0".into())));
        assert_eq!(get("CAT_UNDERGROUND"), Some(Some("18".into())));
        assert_eq!(get("PPT_SNOW"), Some(Some("2".into())));
        assert_eq!(get("SHADERBRIDGE"), Some(Some("1".into())));
        assert_eq!(get("SHADERBRIDGE_VULKAN"), Some(Some("1".into())));
        assert!(m.keys().all(|k| !k.starts_with("IRIS_FEATURE_")));
        assert_eq!(m.keys().filter(|k| k.starts_with("MC_RENDER_STAGE_")).count(), 24);
        assert_eq!(m.keys().filter(|k| k.starts_with("BIOME_")).count(), 67);
    }

    #[test]
    fn environment_dependent_macros() {
        let mut env = CompileEnvironment {
            distant_horizons: false,
            os: "windows".into(),
            vendor: "NVIDIA".into(),
            renderer: "geforce rtx".into(),
            minecraft_version: "26.1.2".into(),
            ..Default::default()
        };
        env.extra_macros.insert("MAX_COLOR_BUFFERS".into(), Some("32".into()));
        env.extra_macros.insert("MY_HOST".into(), None);
        let m = standard_macros(&env);
        assert!(!m.contains_key("DISTANT_HORIZONS"));
        assert!(m.contains_key("DH_BLOCK_WATER"));
        assert!(m.contains_key("MC_OS_WINDOWS"));
        assert!(m.contains_key("MC_GL_VENDOR_NVIDIA"));
        assert!(m.contains_key("MC_GL_RENDERER_GEFORCE_RTX"));
        assert_eq!(m["MC_VERSION"], Some("260102".into()));
        assert_eq!(m["MAX_COLOR_BUFFERS"], Some("32".into()));
        assert_eq!(m["MY_HOST"], None);
    }

    #[test]
    fn feature_flag_macros() {
        let env = CompileEnvironment::default();
        let props = feature_macros(&[], &env, FeatureMacroScope::Properties);
        assert_eq!(props.len(), 13);
        assert!(props.contains_key("IRIS_FEATURE_SSBO"));
        let opt = vec!["ssbo".to_string(), "TESSELATION_SHADERS".to_string(), "NOPE".to_string(), "CUSTOM_IMAGES".to_string()];
        let glsl = feature_macros(&opt, &env, FeatureMacroScope::Glsl);
        // As written; the TESSELATION alias and unknown flags are not defined.
        assert_eq!(glsl.keys().collect::<Vec<_>>(), ["IRIS_FEATURE_ssbo", "IRIS_FEATURE_CUSTOM_IMAGES"]);
        assert_eq!(canonical_feature("tesselation_shaders"), Some("TESSELLATION_SHADERS"));
        let mut no_tess = env.clone();
        no_tess.device.tessellation_shader = false;
        assert!(!is_feature_supported("TESSELLATION_SHADERS", &no_tess));
        assert!(is_feature_supported("SSBO", &no_tess));
        assert!(!is_feature_supported("UNKNOWN_THING", &env));
    }

    #[test]
    fn expression_constants_include_biomes() {
        let c = expression_constants();
        assert_eq!(c.get("BIOME_PLAINS"), Some(&sb_expr::Value::Int(1)));
        assert_eq!(c.get("CAT_OCEAN"), Some(&sb_expr::Value::Int(11)));
    }
}
