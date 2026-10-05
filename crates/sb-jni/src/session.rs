//! Pack sessions (`openPack`): one opened pack, its long-lived pipeline session (with
//! content-addressed analysis, SPIR-V and validation caches, so a recompile after an
//! option change only redoes what changed) and the outputs of the last compile and the
//! last variant.

use crate::error::{Error, Result, panic_message};
use crate::registry::Recover;
use crate::{profiles, worker};
use sb_core::Diagnostics;
use sb_core::model::{BlockLayout, BlobInfo, CompileEnvironment, CompiledPack, CustomUniform, OptionsModel, Program};
use sb_core::program::GeometryProgram;
use sb_pack::{DiscoveredOptions, OptionValues, ShaderPack};
use sb_pipeline::{CompileOutput, CompileSettings, PackSession};
use serde::Serialize;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Language of the options GUI strings embedded in compiled packs.
const COMPILE_LANGUAGE: &str = "en_us";

/// What `compile` was asked to do (parsed from the JNI arguments).
#[derive(Debug, Clone)]
pub(crate) struct CompileRequest {
    pub env: CompileEnvironment,
    pub values: OptionValues,
    pub dimensions: Option<Vec<String>>,
    pub validate: bool,
    pub cache_dir: Option<PathBuf>,
    pub threads: usize,
}

/// The custom uniforms and `sb_Frame` layout of one compiled dimension folder.
pub(crate) struct FolderUniforms {
    pub folder: String,
    pub custom: Vec<CustomUniform>,
    pub frame: BlockLayout,
}

/// What the last successful compile left for later calls.
struct Compiled {
    blobs: Vec<u8>,
    folders: Vec<FolderUniforms>,
}

/// Options model cached for one (language, environment, option values) triple.
struct CachedOptions {
    language: String,
    env: CompileEnvironment,
    values: OptionValues,
    model: OptionsModel,
}

/// One open pack.
pub(crate) struct Session {
    path: PathBuf,
    pack: Arc<ShaderPack>,
    pipeline: PackSession<'static>,
    /// Environment and option values of the last compile request (defaults before):
    /// `getOptions` and `normalizeOptionValues` use them.
    env: CompileEnvironment,
    values: OptionValues,
    /// Compile threads of the last compile request (0 = automatic).
    threads: usize,
    compiled: Option<Compiled>,
    variant: Option<Vec<u8>>,
    options: Option<CachedOptions>,
}

impl Recover for Session {
    fn recover(&mut self) {
        // A call panicked while holding the session: forget everything derived. The caches
        // inside the pipeline session are keyed by content, so they stay valid.
        let settings = self.pipeline.settings().clone();
        self.pipeline.set_settings(settings);
        self.compiled = None;
        self.variant = None;
        self.options = None;
    }
}

/// The JSON of `compileVariant`.
#[derive(Serialize)]
struct VariantJson<'a> {
    program: &'a Program,
    blobs: &'a [BlobInfo],
    /// What compiling the variant reported (warnings, the reason it needs the raw Vulkan
    /// path, ...).
    diagnostics: &'a [sb_core::Diagnostic],
}

impl Session {
    /// Open the pack at `path` (directory or zip).
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let pack = Arc::new(ShaderPack::open(path)?);
        let pipeline = PackSession::shared(pack.clone(), CompileSettings::default());
        Ok(Self {
            path: path.to_path_buf(),
            pack,
            pipeline,
            env: CompileEnvironment::default(),
            values: OptionValues::new(),
            threads: 0,
            compiled: None,
            variant: None,
            options: None,
        })
    }

    /// The options model for `language`, computed with the environment and option values
    /// of the last compile request (cached).
    pub(crate) fn options(&mut self, language: &str) -> Result<&OptionsModel> {
        let hit = self
            .options
            .as_ref()
            .is_some_and(|c| c.language == language && c.env == self.env && c.values == self.values);
        if !hit {
            // A compile restricted to no dimension folder: only the pack-wide inputs
            // (shaders.properties, options, profiles, lang) are loaded, exactly as a real
            // compile loads them.
            let settings = CompileSettings {
                env: self.env.clone(),
                option_values: self.values.clone(),
                dimension_filter: Some(Vec::new()),
                language: language.to_string(),
                ..CompileSettings::default()
            };
            let pack = &self.pack;
            let out = worker::in_pool(self.threads, || sb_pipeline::compile_pack(pack, &settings));
            check_internal(&out.pack)?;
            self.options = Some(CachedOptions {
                language: language.to_string(),
                env: self.env.clone(),
                values: self.values.clone(),
                model: out.pack.options,
            });
        }
        match &self.options {
            Some(c) => Ok(&c.model),
            None => Err(Error::Internal("the options cache is empty".into())),
        }
    }

    /// `text` (Iris settings file) restricted to known options with valid, non-default
    /// values, as a settings file.
    pub(crate) fn normalize_option_values(&mut self, text: &str) -> Result<String> {
        // The options themselves do not depend on the language; reuse any cached model.
        let language = match &self.options {
            Some(c) if c.env == self.env && c.values == self.values => c.language.clone(),
            _ => COMPILE_LANGUAGE.to_string(),
        };
        let model = self.options(&language)?;
        let discovered = DiscoveredOptions { options: model.options.clone(), ..DiscoveredOptions::default() };
        Ok(OptionValues::parse_settings_file(text).normalized(&discovered).to_settings_file())
    }

    /// Compile the pack. Returns the CompiledPack JSON (its `blobs` index filled); the blob
    /// buffer is kept for [`Session::blobs`].
    pub(crate) fn compile(&mut self, req: CompileRequest) -> Result<String> {
        let settings = CompileSettings {
            env: req.env,
            option_values: req.values,
            dimension_filter: req.dimensions,
            language: COMPILE_LANGUAGE.to_string(),
            validate_spirv: req.validate,
            cache_dir: req.cache_dir,
            extra_profiles: profiles::snapshot(),
            profile_overrides: Default::default(),
        };
        self.env = settings.env.clone();
        self.values = settings.option_values.clone();
        self.threads = req.threads;
        self.compiled = None;
        self.variant = None;
        let threads = self.threads;
        let mut out = worker::in_pool(threads, || self.run_compile(settings))?;
        let (infos, buffer) = out.blobs.concat();
        out.blobs = Default::default();
        out.pack.blobs = infos;
        let json = serde_json::to_string(&out.pack).map_err(|source| Error::Serialize { what: "the compiled pack", source })?;
        let folders = out
            .pack
            .dimensions
            .iter()
            .map(|d| FolderUniforms { folder: d.folder.clone(), custom: d.custom_uniforms.clone(), frame: d.uniforms.frame.clone() })
            .collect();
        self.compiled = Some(Compiled { blobs: buffer, folders });
        Ok(json)
    }

    /// The compile itself: a disk-cache hit through [`sb_pipeline::compile_pack`], else the
    /// session's incremental compile (stored in the cache afterwards).
    fn run_compile(&mut self, settings: CompileSettings) -> Result<CompileOutput> {
        let cache = settings.cache_dir.clone().map(|dir| {
            let key = sb_pipeline::cache_key(&self.pack, &settings);
            (dir, key)
        });
        if let Some((dir, key)) = &cache
            && cache_entry_exists(dir, key)
        {
            let out = sb_pipeline::compile_pack(&self.pack, &settings);
            check_internal(&out.pack)?;
            // The pipeline session has no state for these settings; a variant compiles it
            // on demand.
            self.pipeline.set_settings(settings);
            return Ok(out);
        }
        self.pipeline.set_settings(settings);
        let pipeline = &mut self.pipeline;
        let mut out = match std::panic::catch_unwind(AssertUnwindSafe(|| pipeline.compile())) {
            Ok(out) => out,
            Err(payload) => {
                let settings = self.pipeline.settings().clone();
                self.pipeline.set_settings(settings);
                return Err(Error::Internal(format!("compiling {} panicked: {}", self.path.display(), panic_message(&*payload))));
            }
        };
        if let Some((dir, key)) = &cache
            && let Err(e) = store_cache_entry(dir, key, &out)
        {
            out.pack
                .diagnostics
                .push(sb_core::Diagnostic::warning("jni.cache", format!("cannot write the compile cache in {}: {e}", dir.display())));
        }
        Ok(out)
    }

    /// The blob buffer of the last successful compile.
    pub(crate) fn blobs(&self) -> Result<&[u8]> {
        self.compiled.as_ref().map(|c| c.blobs.as_slice()).ok_or(Error::NotCompiled)
    }

    /// The blob buffer of the last successful variant.
    pub(crate) fn variant_blobs(&self) -> Result<&[u8]> {
        self.variant.as_deref().ok_or_else(|| Error::Unavailable("the session has no compiled variant yet".into()))
    }

    /// Custom uniforms and frame layout of a compiled folder.
    pub(crate) fn folder_uniforms(&self, folder: &str) -> Result<&FolderUniforms> {
        let compiled = self.compiled.as_ref().ok_or(Error::NotCompiled)?;
        compiled.folders.iter().find(|f| f.folder == folder).ok_or_else(|| {
            let known: Vec<String> = compiled.folders.iter().map(|f| format!("`{}`", f.folder)).collect();
            Error::Unavailable(format!("dimension folder `{folder}` was not compiled (compiled: {})", known.join(", ")))
        })
    }

    /// Compile `program` of `folder` with draw `profile` after a compile. Returns the
    /// variant JSON; its blob buffer is kept for [`Session::variant_blobs`].
    pub(crate) fn compile_variant(&mut self, folder: &str, program: GeometryProgram, profile: &str) -> Result<String> {
        self.variant = None;
        if self.compiled.is_none() {
            return Err(Error::NotCompiled);
        }
        let known =
            self.pipeline.settings().extra_profiles.iter().any(|p| p.name == profile) || sb_transform::profile(profile).is_some();
        if !known {
            return Err(Error::Unavailable(if profiles::is_registered(profile) {
                format!("draw profile `{profile}` was registered after the last compile; compile again to use it")
            } else {
                format!("draw profile `{profile}` does not exist")
            }));
        }
        let threads = self.threads;
        let pipeline = &mut self.pipeline;
        let result = worker::in_pool(threads, || {
            std::panic::catch_unwind(AssertUnwindSafe(|| sb_pipeline::compile_variant(pipeline, folder, program, profile)))
        });
        let variant = match result {
            Ok(Ok(v)) => v,
            Ok(Err(diagnostics)) => return Err(Error::Unavailable(describe(&diagnostics))),
            Err(payload) => {
                let settings = self.pipeline.settings().clone();
                self.pipeline.set_settings(settings);
                return Err(Error::Internal(format!("compiling a variant panicked: {}", panic_message(&*payload))));
            }
        };
        let (infos, buffer) = variant.blobs.concat();
        let json = serde_json::to_string(&VariantJson { program: &variant.program, blobs: &infos, diagnostics: &variant.diagnostics.0 })
            .map_err(|source| Error::Serialize { what: "the variant", source })?;
        self.variant = Some(buffer);
        Ok(json)
    }
}

/// A panic inside [`sb_pipeline::compile_pack`] comes back as an empty model carrying a
/// `pipeline.internal` error; report it as a failure.
fn check_internal(pack: &CompiledPack) -> Result<()> {
    match pack.diagnostics.iter().find(|d| d.code == "pipeline.internal" && d.is_error()) {
        Some(d) if pack.dimensions.is_empty() => Err(Error::Internal(d.message.clone())),
        _ => Ok(()),
    }
}

/// The error diagnostics (or all, if none is an error) as one message.
fn describe(diagnostics: &Diagnostics) -> String {
    let errors: Vec<String> = diagnostics.iter().filter(|d| d.is_error()).map(|d| d.to_string()).collect();
    let all = if errors.is_empty() { diagnostics.iter().map(|d| d.to_string()).collect() } else { errors };
    if all.is_empty() { "the variant could not be compiled".to_string() } else { all.join("; ") }
}

/// `<dir>/<key>.json` and `<dir>/<key>.bin`: the sb-pipeline compile cache layout.
fn cache_paths(dir: &Path, key: &str) -> (PathBuf, PathBuf) {
    (dir.join(format!("{key}.json")), dir.join(format!("{key}.bin")))
}

fn cache_entry_exists(dir: &Path, key: &str) -> bool {
    let (json, bin) = cache_paths(dir, key);
    json.is_file() && bin.is_file()
}

/// Write a compile into the cache in the layout `sb_pipeline::compile_pack` reads: the
/// CompiledPack JSON with its `blobs` index filled, and the concatenated blob buffer.
/// Temporary files with unique names are renamed into place, buffer first, so concurrent
/// writers never expose a partial entry. (If the pipeline's format ever changes, its
/// loader rejects such an entry and rewrites it.)
fn store_cache_entry(dir: &Path, key: &str, out: &CompileOutput) -> std::io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(dir)?;
    let (infos, buffer) = out.blobs.concat();
    let mut pack = out.pack.clone();
    pack.blobs = infos;
    let json = serde_json::to_string(&pack).map_err(std::io::Error::other)?;
    let (json_path, bin_path) = cache_paths(dir, key);
    let unique = format!("{}.{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
    let tmp_json = dir.join(format!("{key}.json.{unique}.tmp"));
    let tmp_bin = dir.join(format!("{key}.bin.{unique}.tmp"));
    let result = (|| {
        std::fs::write(&tmp_bin, &buffer)?;
        std::fs::write(&tmp_json, json)?;
        std::fs::rename(&tmp_bin, &bin_path)?;
        std::fs::rename(&tmp_json, &json_path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_bin);
        let _ = std::fs::remove_file(&tmp_json);
    }
    result
}
