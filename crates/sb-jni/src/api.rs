//! The pure-Rust API behind every native method of
//! `dev.shaderbridge.natives.ShaderBridgeNative`, one function per method. The JNI exports
//! ([`crate::exports`]) only convert arguments and results; everything else, including the
//! handle registry and running heavy calls on the 64 MiB worker thread, happens here, so
//! it can be tested without a JVM.
//!
//! Handles are opaque positive ids (see [`crate::registry`]); 0 is never a valid handle.
//! Functions never panic on bad input: unknown handles, malformed JSON, missing compiles
//! and the like are [`Error`]s. Panics inside the pipeline are caught and reported as
//! [`Error::Internal`].

use crate::error::{Error, HandleKind, Result};
use crate::evaluator::Evaluator;
use crate::registry::{Registry, lock};
use crate::session::{CompileRequest, Session};
use crate::{profiles, worker};
use sb_core::model::CompileEnvironment;
use sb_core::program::GeometryProgram;
use sb_pack::{OptionValues, ShaderPack};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static SESSIONS: LazyLock<Registry<Session>> = LazyLock::new(|| Registry::new(HandleKind::Session));
static EVALUATORS: LazyLock<Registry<Evaluator>> = LazyLock::new(|| Registry::new(HandleKind::Evaluator));

/// The ShaderBridge version, e.g. `0.1.0`.
pub fn version() -> &'static str {
    sb_core::SHADERBRIDGE_VERSION
}

/// How a listed pack is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackKind {
    /// A directory.
    Dir,
    /// A `.zip` file.
    Zip,
}

/// One entry of [`list_packs`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackListing {
    /// File name (including `.zip`); hosts use it as the pack's identity.
    pub name: String,
    /// Absolute path of the directory or zip file.
    pub path: String,
    /// Directory or zip.
    pub kind: PackKind,
    /// The pack opens and has at least one runnable program.
    pub valid: bool,
    /// Why the pack is not valid (`None` when valid).
    pub error: Option<String>,
}

/// List the shader packs in `dir`: every sub-directory and every `.zip` file (any case),
/// sorted by name (case-insensitively). Hidden entries (names starting with `.`) and other
/// files are skipped. Each pack is opened to check it: it is `valid` when it has a
/// `shaders/` root (or is one) and at least one runnable program in its root or a world
/// folder. Runs on the worker thread.
pub fn list_packs(dir: &Path) -> Result<Vec<PackListing>> {
    let dir = dir.to_path_buf();
    worker::run(move || {
        let entries = std::fs::read_dir(&dir).map_err(|e| Error::io(format!("cannot list {}", dir.display()), e))?;
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            // Follow symlinks: a linked pack directory is a pack.
            let kind = if path.is_dir() {
                PackKind::Dir
            } else if path.is_file() && name.to_ascii_lowercase().ends_with(".zip") {
                PackKind::Zip
            } else {
                continue;
            };
            let error = match ShaderPack::open(&path) {
                Ok(pack) if pack.program_sets().is_empty() => Some("the pack has no shader programs".to_string()),
                Ok(_) => None,
                Err(e) => Some(e.to_string()),
            };
            let absolute = std::path::absolute(&path).unwrap_or(path);
            out.push(PackListing { name, path: absolute.to_string_lossy().into_owned(), kind, valid: error.is_none(), error });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
        Ok(out)
    })
}

/// [`list_packs`] as the JSON array of the JNI contract:
/// `[{"name", "path", "kind": "dir"|"zip", "valid", "error"}]`.
pub fn list_packs_json(dir: &Path) -> Result<String> {
    let packs = list_packs(dir)?;
    serde_json::to_string(&packs).map_err(|source| Error::Serialize { what: "the pack list", source })
}

/// Open a pack (directory or zip) and return its session handle. Runs on the worker
/// thread (opening a zip reads its directory).
pub fn open_pack(path: &Path) -> Result<u64> {
    let path = path.to_path_buf();
    let session = worker::run(move || Session::open(&path))?;
    Ok(SESSIONS.insert(session))
}

/// Close a session. Its outputs are freed once a call still running on it returns;
/// evaluators created from it stay valid. Closing an unknown or closed handle is an error
/// (harmless).
pub fn close_pack(session: u64) -> Result<()> {
    SESSIONS.remove(session)
}

/// Check a Minecraft language code (`en_us`): it names a file in the pack.
fn check_language(language: &str) -> Result<()> {
    let ok = !language.is_empty()
        && language.len() <= 32
        && language.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok { Ok(()) } else { Err(Error::invalid(format!("`{language}` is not a language code"))) }
}

/// The options model (`sb_core::model::OptionsModel` JSON) with the lang strings of
/// `language` (falling back to `en_us`). It is computed with the environment and option
/// values of the session's last `compile` request (the defaults before the first), which
/// only matter for `#if`s in `shaders.properties` and for the `value`/`current_profile`
/// fields. Cached per session. Runs on the worker thread.
pub fn get_options(session: u64, language: &str) -> Result<String> {
    check_language(language)?;
    let s = SESSIONS.get(session)?;
    let language = language.to_string();
    worker::run(move || {
        let mut s = lock(&s);
        let model = s.options(&language)?;
        serde_json::to_string(model).map_err(|source| Error::Serialize { what: "the options model", source })
    })
}

/// The `settingsJson` argument of [`compile`].
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileOptions {
    /// World folders to compile (`""` = pack root); `None` = all.
    #[serde(default)]
    pub dimensions: Option<Vec<String>>,
    /// Run `spirv-val` on every module.
    #[serde(default)]
    pub validate: bool,
    /// Directory of the compile cache; `None` = no caching.
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    /// Extension: compile threads (`None` or 0 = one per CPU).
    #[serde(default)]
    pub threads: Option<u32>,
}

impl CompileOptions {
    /// Parse `settingsJson`. Empty text and `null` mean the defaults; unknown keys are
    /// ignored.
    pub fn parse(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let parsed: Option<Self> = serde_json::from_str(json).map_err(|e| Error::invalid(format!("settingsJson: {e}")))?;
        Ok(parsed.unwrap_or_default())
    }
}

/// Compile the pack.
///
/// * `env_json`: `sb_core::model::CompileEnvironment` JSON;
/// * `option_values`: Iris settings text (`NAME=value` lines), may be empty;
/// * `settings_json`: [`CompileOptions`] JSON.
///
/// Returns the CompiledPack JSON with its `blobs` index filled; the blob buffer is then
/// available through [`blob_size`] / [`with_blob_data`] until the next compile. Problems in
/// the pack are diagnostics inside the model, not errors; errors are bad arguments, an
/// unknown handle and internal failures.
///
/// With a cache directory, a cached result is returned when the pack contents, option
/// values, environment, settings, registered draw profiles and ShaderBridge version are
/// unchanged; otherwise the session's incremental compile runs and its result is cached.
/// Runs on the worker thread, inside a pool of `threads` threads when requested.
pub fn compile(session: u64, env_json: &str, option_values: &str, settings_json: &str) -> Result<String> {
    let env: CompileEnvironment = serde_json::from_str(env_json).map_err(|e| Error::invalid(format!("envJson: {e}")))?;
    // Unknown dimension folders are a warning diagnostic of the compile, not an error.
    let options = CompileOptions::parse(settings_json)?;
    let request = CompileRequest {
        env,
        values: OptionValues::parse_settings_file(option_values),
        dimensions: options.dimensions,
        validate: options.validate,
        cache_dir: options.cache_dir,
        threads: options.threads.map_or(0, |t| t as usize),
    };
    let s = SESSIONS.get(session)?;
    worker::run(move || lock(&s).compile(request))
}

/// Size in bytes of the blob buffer of the session's last successful compile.
pub fn blob_size(session: u64) -> Result<u64> {
    let s = SESSIONS.get(session)?;
    let s = lock(&s);
    Ok(s.blobs()?.len() as u64)
}

/// Call `f` with the blob buffer of the session's last successful compile (the session
/// stays locked meanwhile).
pub fn with_blob_data<R>(session: u64, f: impl FnOnce(&[u8]) -> R) -> Result<R> {
    let s = SESSIONS.get(session)?;
    let s = lock(&s);
    Ok(f(s.blobs()?))
}

/// Parse a geometry program name: the file name (`gbuffers_terrain`, `shadow_cutout`,
/// `dh_water`) or the model's JSON name (`terrain`, `shadow_cutout`, `dh_water`).
pub fn parse_geometry_program(name: &str) -> Result<GeometryProgram> {
    GeometryProgram::from_file_name(name)
        .or_else(|| serde_json::from_value(serde_json::Value::String(name.to_string())).ok())
        .ok_or_else(|| Error::invalid(format!("`{name}` is not a geometry program")))
}

/// Compile one extra (geometry program, draw profile) variant of a compiled folder: the
/// program the folder resolves for `geometry_program` (following its fallback chain past
/// programs that fail) translated for `profile`. The profile must be built in or have been
/// registered before the last compile. Returns `{"program": Program, "blobs": [BlobInfo]}`;
/// the blob offsets index the buffer of [`variant_blob_size`] / [`with_variant_blob_data`],
/// kept until the next variant or compile. Runs on the worker thread.
pub fn compile_variant(session: u64, folder: &str, geometry_program: &str, profile: &str) -> Result<String> {
    let program = parse_geometry_program(geometry_program)?;
    let s = SESSIONS.get(session)?;
    let (folder, profile) = (folder.to_string(), profile.to_string());
    worker::run(move || lock(&s).compile_variant(&folder, program, &profile))
}

/// Size in bytes of the blob buffer of the session's last successful variant.
pub fn variant_blob_size(session: u64) -> Result<u64> {
    let s = SESSIONS.get(session)?;
    let s = lock(&s);
    Ok(s.variant_blobs()?.len() as u64)
}

/// Call `f` with the blob buffer of the session's last successful variant.
pub fn with_variant_blob_data<R>(session: u64, f: impl FnOnce(&[u8]) -> R) -> Result<R> {
    let s = SESSIONS.get(session)?;
    let s = lock(&s);
    Ok(f(s.variant_blobs()?))
}

/// Register an extra draw profile (TOML, see `sb-transform/profiles/README.md`) for all
/// later compiles of all sessions, replacing an earlier one of the same name. Returns the
/// profile name.
pub fn register_profile(toml: &str) -> Result<String> {
    let toml = toml.to_string();
    worker::run(move || profiles::register(&toml))
}

/// Create a custom-uniform evaluator for a folder of the session's last compile. Fails
/// ([`Error::Unavailable`]) when the folder was not compiled or has no custom uniforms.
/// The evaluator keeps its own copy of the definitions and layout.
pub fn create_uniform_evaluator(session: u64, folder: &str) -> Result<u64> {
    let s = SESSIONS.get(session)?;
    let folder = folder.to_string();
    let evaluator = worker::run(move || {
        let s = lock(&s);
        let f = s.folder_uniforms(&folder)?;
        Evaluator::new(&folder, &f.custom, f.frame.clone())
    })?;
    Ok(EVALUATORS.insert(evaluator))
}

/// Evaluate custom uniforms in place on `frame_block`, the whole `sb_Frame` block (at
/// least `frame.size` bytes): builtin-sourced members are read, custom-sourced members are
/// written. Runs on the calling thread (it is a per-frame call; expressions are bounded in
/// depth).
pub fn evaluate_uniforms(evaluator: u64, frame_block: &mut [u8], frame_delta_seconds: f32) -> Result<()> {
    let e = EVALUATORS.get(evaluator)?;
    let mut e = lock(&e);
    e.evaluate(frame_block, frame_delta_seconds)
}

/// Destroy an evaluator. Destroying an unknown or destroyed handle is an error (harmless).
pub fn destroy_uniform_evaluator(evaluator: u64) -> Result<()> {
    EVALUATORS.remove(evaluator)
}

/// Normalize Iris settings text against the pack's options: unknown options, invalid
/// booleans and values equal to the default are dropped. Returns settings text (sorted
/// `NAME=value` lines). Runs on the worker thread.
pub fn normalize_option_values(session: u64, option_values: &str) -> Result<String> {
    let s = SESSIONS.get(session)?;
    let text = option_values.to_string();
    worker::run(move || lock(&s).normalize_option_values(&text))
}
