//! Helpers of the corpus integration test: corpus discovery, standard macros and the
//! per-pack environment (folders, option values, enabled programs).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sb_preprocess::PreprocessOptions;

const SCRATCH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test-data");

/// The small corpus packs (default roots).
pub const SMALL_CORPUS: &[&str] = &[
    "photon",
    "Bliss-Shader",
    "ComplementaryReimagined",
    "Super-Duper-Vanilla",
    "glimmer-shaders",
    "spectrum",
    "Ominous-Shaderpack",
    "MinecraftShaderProgramming",
];

/// Corpus roots: `$SB_CORPUS_DIRS` (colon-separated) or the scratchpad defaults
/// (the small corpus packs, then the extended corpus directory).
pub fn corpus_roots() -> Vec<PathBuf> {
    if let Some(v) = std::env::var_os("SB_CORPUS_DIRS") {
        return std::env::split_paths(&v).filter(|p| p.is_dir()).collect();
    }
    let mut out: Vec<PathBuf> = SMALL_CORPUS.iter().map(|p| Path::new(SCRATCH).join("corpus").join(p)).collect();
    out.push(Path::new(SCRATCH).join("corpus2"));
    out.into_iter().filter(|p| p.is_dir()).collect()
}

/// Pack directories under `root`: every directory containing a `shaders/` directory
/// (searched up to three levels deep). Sorted.
pub fn packs_in(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        if dir.join("shaders").is_dir() {
            out.push(dir.to_path_buf());
            return;
        }
        if depth == 0 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut subs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        subs.sort();
        for s in subs {
            walk(&s, depth - 1, out);
        }
    }
    let mut out = Vec::new();
    walk(root, 3, &mut out);
    out
}

/// Iris standard macros for a Vulkan host with Distant Horizons.
pub fn standard_options() -> PreprocessOptions {
    let mut o = PreprocessOptions::default();
    let defines: &[(&str, &str)] = &[
        ("MC_VERSION", "260300"),
        ("MC_GL_VERSION", "460"),
        ("MC_GLSL_VERSION", "460"),
        ("MC_MIPMAP_LEVEL", "4"),
        ("IRIS_VERSION", "11107"),
        ("MAX_COLOR_BUFFERS", "16"),
        ("IRIS_TAG_SUPPORT", "2"),
        ("MC_RENDER_QUALITY", "1.0"),
        ("MC_SHADOW_QUALITY", "1.0"),
        ("MC_HAND_DEPTH", "0.125"),
    ];
    for (k, v) in defines {
        o = o.with_define(*k, *v);
    }
    for f in [
        "IS_IRIS",
        "DISTANT_HORIZONS",
        "DISTANT_HORIZONS_TEXTURES",
        "MC_OS_LINUX",
        "MC_GL_VENDOR_OTHER",
        "MC_GL_RENDERER_OTHER",
        "IRIS_HAS_TRANSLUCENCY_SORTING",
        "MC_NORMAL_MAP",
        "MC_SPECULAR_MAP",
        "SHADERBRIDGE",
        "MC_GL_ARB_shader_texture_lod",
        "MC_GL_ARB_gpu_shader5",
        "MC_GL_ARB_shader_image_load_store",
        "MC_GL_ARB_compute_shader",
        "MC_GL_ARB_shader_storage_buffer_object",
        "MC_GL_ARB_texture_gather",
        "MC_GL_EXT_gpu_shader4",
        "MC_GL_ARB_explicit_attrib_location",
        "MC_GL_ARB_shading_language_packing",
        "MC_GL_ARB_shader_bit_encoding",
        "MC_GL_ARB_conservative_depth",
        "MC_GL_ARB_texture_query_levels",
        "MC_GL_ARB_derivative_control",
        "MC_GL_ARB_shader_draw_parameters",
        "MC_GL_ARB_separate_shader_objects",
        "MC_GL_ARB_shading_language_420pack",
        "MC_GL_ARB_enhanced_layouts",
        "MC_GL_ARB_texture_cube_map_array",
        "MC_GL_ARB_shader_group_vote",
        "MC_GL_ARB_gpu_shader_fp64",
        "MC_GL_ARB_shader_atomic_counters",
        "MC_GL_ARB_sample_shading",
    ] {
        o = o.with_flag(f);
    }
    let dh = [
        "UNKNOWN", "LEAVES", "STONE", "WOOD", "METAL", "DIRT", "LAVA", "DEEPSLATE", "SNOW", "SAND", "TERRACOTTA",
        "NETHER_STONE", "WATER", "GRASS", "AIR", "ILLUMINATED",
    ];
    for (i, n) in dh.iter().enumerate() {
        o = o.with_define(format!("DH_BLOCK_{n}"), i.to_string());
    }
    let stages = [
        "NONE", "SKY", "SUNSET", "CUSTOM_SKY", "SUN", "MOON", "STARS", "VOID", "TERRAIN_SOLID", "TERRAIN_CUTOUT_MIPPED",
        "TERRAIN_CUTOUT", "ENTITIES", "BLOCK_ENTITIES", "DESTROY", "OUTLINE", "DEBUG", "HAND_SOLID", "TERRAIN_TRANSLUCENT",
        "TRIPWIRE", "PARTICLES", "CLOUDS", "RAIN_SNOW", "WORLD_BORDER", "HAND_TRANSLUCENT",
    ];
    for (i, n) in stages.iter().enumerate() {
        o = o.with_define(format!("MC_RENDER_STAGE_{n}"), i.to_string());
    }
    // BIOME_*, CAT_*, PPT_* (Iris IrisDefines).
    for (name, value) in sb_expr::standard_constants() {
        if let sb_expr::Value::Int(v) = value {
            o = o.with_define(name, v.to_string());
        }
    }
    o
}

/// Feature flags ShaderBridge supports (`IRIS_FEATURE_<flag>` is defined for those a
/// pack names in `iris.features.*`).
pub const SUPPORTED_FEATURES: &[&str] = &[
    "SEPARATE_HARDWARE_SAMPLERS",
    "HIGHER_SHADOWCOLOR",
    "CUSTOM_IMAGES",
    "PER_BUFFER_BLENDING",
    "COMPUTE_SHADERS",
    "TESSELLATION_SHADERS",
    "TESSELATION_SHADERS",
    "ENTITY_TRANSLUCENT",
    "REVERSED_CULLING",
    "BLOCK_EMISSION_ATTRIBUTE",
    "CAN_DISABLE_WEATHER",
    "SSBO",
    "FADE_VARIABLE",
    "TEXTURE_FILTERING",
];

/// Standard macros plus `IRIS_FEATURE_<flag>` for every supported flag the pack's
/// `shaders.properties` names on an `iris.features.*` line (conditionals ignored).
pub fn pack_options(pack: &sb_pack::ShaderPack) -> PreprocessOptions {
    let mut o = standard_options();
    if let Some(props) = pack.read_latin1("shaders.properties") {
        for line in props.lines().filter(|l| l.trim_start().starts_with("iris.features.")) {
            let value = line.split_once('=').map_or("", |(_, v)| v);
            for flag in value.split_whitespace() {
                if SUPPORTED_FEATURES.contains(&flag) {
                    o = o.with_flag(format!("IRIS_FEATURE_{flag}"));
                }
            }
        }
    }
    o
}

/// Option values a corpus run applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OptionSet {
    /// Every option at the pack's default.
    Defaults,
    /// Every boolean option on, every value option at its last listed value (options
    /// without a value list keep their default).
    Max,
}

impl OptionSet {
    /// Both sets, defaults first.
    pub const ALL: [OptionSet; 2] = [OptionSet::Defaults, OptionSet::Max];

    /// The user values of this set for the discovered options.
    pub fn values(self, opts: &sb_pack::DiscoveredOptions) -> sb_pack::OptionValues {
        let mut values = sb_pack::OptionValues::new();
        if self == OptionSet::Max {
            for o in &opts.options {
                if sb_pack::options::is_boolean_option(o) {
                    values.set(o.name.clone(), "true");
                } else if let Some(last) = o.allowed.last() {
                    values.set(o.name.clone(), last.clone());
                }
            }
        }
        values
    }
}

/// What the pipeline would decide about a pack for one [`OptionSet`]: preprocessing
/// options, the program folders, the edited sources and `program.<path>.enabled`.
pub struct PackEnv {
    /// The pack (with its `dimension.properties` map applied).
    pub pack: Arc<sb_pack::ShaderPack>,
    /// Standard macros + feature flags.
    pub options: PreprocessOptions,
    /// Program folders to translate: the root (`""`) followed by every world folder with
    /// runnable programs (`world0`, `world-1`, `world1` and the folders
    /// `dimension.properties` names).
    pub folders: Vec<String>,
    /// Pack sources with the option values applied (as Iris edits option lines).
    pub sources: sb_pack::EditedSources,
    /// Number of options whose value differs from the default.
    pub changed_options: usize,
    enabled: indexmap::IndexMap<String, String>,
    values: std::collections::HashMap<String, String>,
}

impl PackEnv {
    /// Open the pack in `dir` and evaluate its settings for `set`.
    pub fn open(dir: &Path, set: OptionSet) -> Option<Self> {
        let mut pack = sb_pack::ShaderPack::open(dir).ok()?;
        let options = pack_options(&pack);
        // dimension.properties is preprocessed with the environment macros.
        if let Some(dim) = pack.read_latin1("dimension.properties") {
            let (pre, _) = sb_preprocess::preprocess_properties(&dim, "dimension.properties", &options.defines);
            pack.set_dimension_map(sb_pack::idmap::parse_dimension_properties(&pre));
        }
        let pack = Arc::new(pack);
        let (opts, _) = sb_pack::options::discover(&pack, &pack.option_start_files());
        let user = set.values(&opts);
        let changed_options = user.changed_count(&opts);
        let mut macros = options.defines.clone();
        macros.extend(opts.property_macros(&user));
        let mut enabled = indexmap::IndexMap::new();
        if let Some(raw_text) = pack.read_latin1("shaders.properties") {
            let (pre, _) = sb_preprocess::preprocess_properties(&raw_text, "shaders.properties", &macros);
            let raw = sb_pack::properties::parse(&raw_text);
            let prep = sb_pack::properties::parse_preprocessed(&pre);
            let (props, _) = sb_pack::shaders_properties::parse(&prep, &raw);
            enabled = props.program_enabled.clone();
        }
        let mut folders = vec![String::new()];
        folders.extend(pack.world_folders());
        let values: std::collections::HashMap<String, String> =
            opts.names().filter_map(|n| opts.effective_value(n, &user).map(|v| (n.to_string(), v))).collect();
        let sources = sb_pack::EditedSources::new(pack.clone(), opts, user);
        Some(Self { pack, options, folders, sources, changed_options, enabled, values })
    }

    /// Whether `program` (base name, e.g. `composite3` or `deferred1_a`) of `folder` is
    /// enabled.
    pub fn enabled(&self, folder: &str, program: &str) -> bool {
        let path = sb_pack::shaders_properties::program_path(folder, program);
        let Some(expr) = self.enabled.get(&path) else { return true };
        let lookup = |name: &str| -> Option<sb_expr::Value> {
            let v = self.values.get(name)?;
            Some(match v.as_str() {
                "true" => sb_expr::Value::Bool(true),
                "false" => sb_expr::Value::Bool(false),
                s => match s.parse::<i32>() {
                    Ok(i) => sb_expr::Value::Int(i),
                    Err(_) => sb_expr::Value::Float(s.parse::<f32>().unwrap_or(0.0)),
                },
            })
        };
        sb_expr::eval_bool_with(expr, &lookup).unwrap_or(true)
    }
}
