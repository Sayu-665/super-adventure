//! # sb-pipeline
//!
//! Orchestrates every ShaderBridge crate: a [`ShaderPack`] plus [`CompileSettings`] become
//! a [`CompiledPack`] (the host-agnostic model of `docs/ARCHITECTURE.md` §7) and a
//! [`BlobTable`] of SPIR-V and GLSL.
//!
//! Steps (Iris semantics throughout):
//!
//! 1. standard macros ([`standard_macros`]) and feature flags;
//! 2. `shaders.properties`, preprocessed with options and macros (functional keys) and raw
//!    (GUI keys, feature flags); options, profiles and the options GUI model; dimension
//!    folders (`dimension.properties`); id maps;
//! 3. per folder: program resolution (`program.*.enabled`, profile toggles, synthesized
//!    vertex shaders, lettered computes), preprocessing and analysis (parallel), directive
//!    scanning (`RENDERTARGETS`/`DRAWBUFFERS`, const directives, work groups);
//! 4. the pack-global `sb_Frame`/`sb_Draw` layout and binding table;
//! 5. translation and compilation of every (program, draw profile) variant the geometry
//!    fallback chains, composite passes and Distant Horizons need, in parallel, with
//!    graceful degradation: a failing program is reported and its geometry falls back to
//!    the next program of the chain (Iris would reject the whole pack);
//! 6. the pass list and the static main/alt flip schedule, shared attachment lists, render
//!    targets, settings and the DH pipeline (native or synthesized).
//!
//! [`compile_pack`] never panics: every problem becomes a [`Diagnostic`] (tagged with the
//! program and stage, mapped to the original `file:line`).
//!
//! ```no_run
//! let pack = sb_pack::ShaderPack::open(std::path::Path::new("packs/MyPack.zip")).unwrap();
//! let out = sb_pipeline::compile_pack(&pack, &sb_pipeline::CompileSettings::default());
//! for d in out.pack.diagnostics.errors() {
//!     eprintln!("{d}");
//! }
//! let json = out.pack.to_json();
//! let (_infos, buffer) = out.blobs.concat();
//! # let _ = (json, buffer);
//! ```

mod analysis;
mod bindings;
mod cache;
mod compile;
pub mod dh;
mod diagnostics;
pub mod directives;
pub mod flips;
mod folder;
mod inspect;
mod load;
pub mod macros;
mod resolve;
mod sources;

pub use bindings::{resource_kind_of, sample_type_name};
pub use cache::cache_key;
pub use inspect::{FolderSummary, PackSummary, ProgramSummary, inspect};
pub use macros::standard_macros;
pub use sources::PackRef;

use compile::Caches;
use folder::{FolderInputs, FolderState, Profiles, SpecInputs, VariantKey};
use indexmap::IndexMap;
use sb_core::model::{BlobId, BlobInfo, BlobKind, BlobTable, CompileEnvironment, CompiledPack, PackInfo, Program};
use sb_core::program::{GeometryGroup, GeometryProgram};
use sb_core::{Diagnostic, Diagnostics};
use sb_pack::{OptionValues, ShaderPack};
use sb_transform::DrawProfile;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// Settings of a compile.
#[derive(Debug, Clone)]
pub struct CompileSettings {
    /// Host environment (Minecraft version, Distant Horizons, targets, depth mode, device).
    pub env: CompileEnvironment,
    /// User option values (the Iris `shaderpacks/<pack>.txt` contents).
    pub option_values: OptionValues,
    /// World folders to compile (`""` = pack root); `None` = all.
    pub dimension_filter: Option<Vec<String>>,
    /// Language of the options GUI strings (`lang/<code>.lang`, falling back to `en_us`).
    pub language: String,
    /// Run `spirv-val` on every module (slower; used by the CLI and tests).
    pub validate_spirv: bool,
    /// Directory for compiled-pack caching (`<key>.json` + `<key>.bin`); `None` = no cache.
    pub cache_dir: Option<PathBuf>,
    /// Draw profiles registered by the host (they take precedence over built-in profiles
    /// of the same name; their builtins and resources are added to every folder's layout).
    pub extra_profiles: Vec<DrawProfile>,
    /// Extra (geometry program, profile) variants to compile; they are added to the
    /// model's program list (not to the geometry map) so hosts can select them.
    pub profile_overrides: IndexMap<GeometryProgram, Vec<String>>,
}

impl Default for CompileSettings {
    fn default() -> Self {
        Self {
            env: CompileEnvironment::default(),
            option_values: OptionValues::new(),
            dimension_filter: None,
            language: "en_us".into(),
            validate_spirv: false,
            cache_dir: None,
            extra_profiles: Vec::new(),
            profile_overrides: IndexMap::new(),
        }
    }
}

/// Wall-clock timings of a compile, in milliseconds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timings {
    /// Properties, options, id maps and folders.
    pub load_ms: f64,
    /// Per folder: preprocessing, analysis, translation, compilation.
    pub folders_ms: Vec<(String, f64)>,
    pub total_ms: f64,
    /// The result came from the cache.
    pub cache_hit: bool,
}

/// Program counts of a compile (what `shaderbridge validate` reports).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompileStats {
    /// Compiled (program, profile) variants.
    pub programs_ok: usize,
    /// Variants (or programs that failed analysis) that could not be compiled.
    pub programs_failed: Vec<String>,
    /// Compiled shader stages.
    pub modules: usize,
    /// Stages accepted by `spirv-val`.
    pub modules_validated: usize,
}

/// The result of [`compile_pack`].
#[derive(Debug, Clone)]
pub struct CompileOutput {
    pub pack: CompiledPack,
    pub blobs: BlobTable,
    pub timings: Timings,
    pub stats: CompileStats,
}

/// A [`BlobTable`] that stores identical payloads once.
#[derive(Debug, Default)]
pub(crate) struct BlobStore {
    table: BlobTable,
    index: HashMap<(u8, [u8; 32]), BlobId>,
}

impl BlobStore {
    fn put(&mut self, kind: BlobKind, bytes: Vec<u8>) -> BlobId {
        let tag = match kind {
            BlobKind::Spirv => 0,
            BlobKind::Glsl => 1,
            BlobKind::Bytes => 2,
        };
        let h = *blake3::hash(&bytes).as_bytes();
        if let Some(id) = self.index.get(&(tag, h)) {
            return *id;
        }
        let id = self.table.push(kind, bytes);
        self.index.insert((tag, h), id);
        id
    }

    pub(crate) fn spirv(&mut self, words: &[u32]) -> BlobId {
        self.put(BlobKind::Spirv, words.iter().flat_map(|w| w.to_le_bytes()).collect())
    }

    pub(crate) fn glsl(&mut self, text: &str) -> BlobId {
        self.put(BlobKind::Glsl, text.as_bytes().to_vec())
    }

    fn into_table(self) -> BlobTable {
        self.table
    }
}

/// Blob index exactly as [`BlobTable::concat`] lays the buffer out (8-byte aligned).
fn blob_infos(t: &BlobTable) -> Vec<BlobInfo> {
    let mut offset = 0u64;
    t.blobs
        .iter()
        .map(|(kind, data)| {
            offset = offset.div_ceil(8) * 8;
            let info = BlobInfo { kind: *kind, offset, len: data.len() as u64 };
            offset += data.len() as u64;
            info
        })
        .collect()
}

struct SessionState {
    load: load::PackLoad,
    folders: Vec<FolderState>,
}

/// A compile session over one pack. It keeps the preprocessed and analyzed state and
/// content-addressed caches (analysis, SPIR-V, validation), so recompiling after an
/// option change only re-analyzes and recompiles what changed, and extra
/// (program, profile) variants compile quickly ([`compile_variant`]). Used by the JNI
/// layer; [`compile_pack`] uses a one-shot session.
pub struct PackSession<'p> {
    pack: PackRef<'p>,
    settings: CompileSettings,
    caches: Caches,
    state: Option<SessionState>,
}

impl std::fmt::Debug for PackSession<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackSession").field("pack", &self.pack).field("compiled", &self.state.is_some()).finish()
    }
}

impl<'p> PackSession<'p> {
    /// A session over a borrowed pack.
    pub fn new(pack: &'p ShaderPack, settings: CompileSettings) -> Self {
        Self { pack: PackRef::Borrowed(pack), settings, caches: Caches::default(), state: None }
    }

    /// A session that shares ownership of the pack (long-lived hosts).
    pub fn shared(pack: Arc<ShaderPack>, settings: CompileSettings) -> PackSession<'static> {
        PackSession { pack: PackRef::Shared(pack), settings, caches: Caches::default(), state: None }
    }

    /// The pack.
    pub fn pack(&self) -> &ShaderPack {
        &self.pack
    }

    /// The current settings.
    pub fn settings(&self) -> &CompileSettings {
        &self.settings
    }

    /// Change the option values (the next [`PackSession::compile`] reuses unchanged work).
    pub fn set_option_values(&mut self, values: OptionValues) {
        self.settings.option_values = values;
        self.state = None;
    }

    /// Replace all settings. Caches stay valid (they are keyed by content).
    pub fn set_settings(&mut self, settings: CompileSettings) {
        self.settings = settings;
        self.state = None;
    }

    /// Compile the pack with the current settings.
    pub fn compile(&mut self) -> CompileOutput {
        let start = Instant::now();
        let pack: &ShaderPack = &self.pack;
        let settings = &self.settings;
        let load = load::load(pack, &settings.env, &settings.option_values, &settings.language, settings.dimension_filter.as_deref());
        let load_ms = start.elapsed().as_secs_f64() * 1e3;
        let sources = sources::OptionSources::new(self.pack.clone(), load.options.clone(), load.values.clone());
        let mut blobs = BlobStore::default();
        let mut diags = load.diagnostics.clone();
        let mut dimensions = Vec::new();
        let mut states = Vec::new();
        let mut stats = CompileStats::default();
        let mut folders_ms = Vec::new();
        {
            let input = FolderInputs {
                pack,
                sources: &sources,
                load: &load,
                settings,
                caches: &self.caches,
                profiles: Profiles { extra: &settings.extra_profiles },
            };
            for info in &load.folders {
                let t = Instant::now();
                let r = folder::build_folder(&input, info, &mut blobs);
                folders_ms.push((info.folder.clone(), t.elapsed().as_secs_f64() * 1e3));
                diags.extend(r.diagnostics);
                stats.programs_ok += r.stats.programs_ok;
                stats.programs_failed.extend(r.stats.programs_failed);
                stats.modules += r.stats.modules;
                stats.modules_validated += r.stats.modules_validated;
                dimensions.push(r.pipeline);
                states.push(r.state);
            }
        }
        let blobs = blobs.into_table();
        let info = PackInfo {
            name: pack.name().to_string(),
            source_hash: cache::cache_key_with_hash(&pack.content_hash(), settings),
            shaderbridge_version: sb_core::SHADERBRIDGE_VERSION.to_string(),
            features_enabled: load.features_enabled.clone(),
            features_unsupported: load.features_unsupported.clone(),
            environment: settings.env.clone(),
        };
        let compiled = CompiledPack {
            format_version: sb_core::MODEL_FORMAT_VERSION,
            info,
            options: load.options_model.clone(),
            id_maps: load.id_maps.clone(),
            dimensions,
            diagnostics: diagnostics::finalize(diags),
            blobs: blob_infos(&blobs),
        };
        self.state = Some(SessionState { load, folders: states });
        CompileOutput {
            pack: compiled,
            blobs,
            timings: Timings { load_ms, folders_ms, total_ms: start.elapsed().as_secs_f64() * 1e3, cache_hit: false },
            stats,
        }
    }
}

/// Compile a pack. Never panics: errors become diagnostics (an internal panic in a
/// dependency is caught and reported as `pipeline.internal`).
///
/// With [`CompileSettings::cache_dir`], the result is cached under
/// `<dir>/<cache_key>.{json,bin}` and reused when the pack contents, option values,
/// environment and ShaderBridge version are unchanged.
pub fn compile_pack(pack: &ShaderPack, settings: &CompileSettings) -> CompileOutput {
    let start = Instant::now();
    let key = settings.cache_dir.as_ref().map(|_| cache::cache_key(pack, settings));
    if let (Some(dir), Some(key)) = (&settings.cache_dir, &key)
        && let Some((pack_model, blobs)) = cache::load(dir, key)
    {
        return CompileOutput {
            pack: pack_model,
            blobs,
            timings: Timings { total_ms: start.elapsed().as_secs_f64() * 1e3, cache_hit: true, ..Default::default() },
            stats: CompileStats::default(),
        };
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| PackSession::new(pack, settings.clone()).compile()));
    let out = match result {
        Ok(out) => out,
        Err(panic) => {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown panic".into());
            failed_output(pack, settings, Diagnostic::error("pipeline.internal", format!("internal error while compiling the pack: {msg}")))
        }
    };
    if let (Some(dir), Some(key)) = (&settings.cache_dir, &key) {
        let mut out_pack = out.pack.clone();
        out_pack.blobs = blob_infos(&out.blobs);
        if let Err(e) = cache::store(dir, key, &out_pack, &out.blobs) {
            let mut out = out;
            out.pack.diagnostics.push(Diagnostic::warning("pipeline.cache", format!("cannot write the compile cache: {e}")));
            return out;
        }
    }
    out
}

/// An empty model carrying one diagnostic.
fn failed_output(pack: &ShaderPack, settings: &CompileSettings, d: Diagnostic) -> CompileOutput {
    let mut diagnostics = Diagnostics::new();
    diagnostics.push(d);
    CompileOutput {
        pack: CompiledPack {
            format_version: sb_core::MODEL_FORMAT_VERSION,
            info: PackInfo {
                name: pack.name().to_string(),
                source_hash: String::new(),
                shaderbridge_version: sb_core::SHADERBRIDGE_VERSION.to_string(),
                features_enabled: Vec::new(),
                features_unsupported: Vec::new(),
                environment: settings.env.clone(),
            },
            options: Default::default(),
            id_maps: Default::default(),
            dimensions: Vec::new(),
            diagnostics,
            blobs: Vec::new(),
        },
        blobs: BlobTable::default(),
        timings: Timings::default(),
        stats: CompileStats::default(),
    }
}

/// Compile one extra variant: the program resolved for `program` in `folder` (following
/// the fallback chain past programs that fail) translated with draw profile `profile`.
///
/// The variant uses the folder's existing `sb_Frame`/`sb_Draw` layout and binding table,
/// so a profile whose builtins or host resources are not already part of them must be
/// registered up front (`CompileSettings::extra_profiles` or `profile_overrides`); the
/// translation otherwise fails with an unbound-resource diagnostic. Compiles the session
/// first if needed. Blob ids of the returned program index the returned table.
pub fn compile_variant(
    session: &mut PackSession<'_>,
    folder: &str,
    program: GeometryProgram,
    profile: &str,
) -> Result<(Program, BlobTable), Diagnostics> {
    if session.state.is_none() {
        session.compile();
    }
    let err = |code: &str, msg: String| Diagnostics(vec![Diagnostic::error(code, msg)]);
    let settings = &session.settings;
    let Some(state) = session.state.as_ref() else {
        return Err(err("pipeline.internal", "the session has no compiled state".into()));
    };
    let Some(fs) = state.folders.iter().find(|f| f.info.folder == folder) else {
        return Err(err("pipeline.unknown-dimension", format!("folder `{folder}` was not compiled")));
    };
    let profiles = Profiles { extra: &settings.extra_profiles };
    let Some(prof) = profiles.get(profile) else {
        return Err(err("pipeline.unknown-profile", format!("draw profile `{profile}` does not exist")));
    };
    let class = if program.group() == GeometryGroup::DistantHorizons {
        sb_uniforms::ProgramClass::Dh
    } else {
        sb_uniforms::ProgramClass::from_geometry(program)
    };
    let shadow_pass = folder::is_shadow_group(program);
    let candidates: Vec<usize> =
        program.chain().filter_map(|p| fs.plan.geometry.get(&p).copied()).filter(|&u| fs.analysis.ok(u)).collect();
    if candidates.is_empty() {
        return Err(err("pipeline.no-program", format!("folder `{folder}` has no program for {}", program.file_name())));
    }
    let props = &state.load.props;
    let spec_inputs = SpecInputs {
        folder,
        plan: &fs.plan,
        directives: &fs.directives,
        gbuffer_attachments: &fs.gbuffer_attachments,
        shadow_attachments: &fs.shadow_attachments,
        dh_constants: &fs.dh_constants,
        props,
    };
    let mut diags = Diagnostics::new();
    for u in candidates {
        let key = VariantKey { unit: u, profile: profile.to_string(), class, shadow_pass };
        let spec = spec_inputs.spec(&key, None, (3, 0, 0));
        let Some(stages) = fs.analysis.stages_of(u) else { continue };
        let ctx = fs.contexts[u].for_class(class);
        let jenv = compile::JobEnv {
            env: &settings.env,
            validate: settings.validate_spirv,
            caches: &session.caches,
            pack: sb_transform::PackContext { layout: &fs.layout, members: &fs.members, bindings: &fs.bindings, resources: &ctx },
            image_formats: &fs.image_formats,
        };
        let job = compile::VariantJob {
            name: spec.name.clone(),
            stages,
            files: fs.plan.units[u].stages.iter().map(|(s, src)| (*s, src.path().to_string())).collect(),
            profile: prof,
            class,
            shadow_pass,
            alpha_test: spec.alpha_test,
            output_locations: spec.output_slots.clone(),
            profile_constants: spec.profile_constants.clone(),
            kind: spec.kind.clone(),
        };
        match compile::compile_job(&job, &jenv) {
            Ok(c) => {
                let mut blobs = BlobStore::default();
                let mut d = Diagnostics::new();
                let mut p = folder::model_program(
                    &spec,
                    &c,
                    &fs.plan.units[u],
                    &fs.directives,
                    props,
                    &fs.bindings,
                    &props.settings(),
                    &settings.env,
                    &mut d,
                    &mut blobs,
                );
                let state = fs.schedule.group_state.get(&folder::phase_of_slot(program)).cloned().unwrap_or_default();
                for b in &mut p.bindings_used {
                    b.use_alt = match fs.bindings.get(&b.name).map(|e| &e.resource) {
                        Some(sb_core::model::ResourceRef::ColorTex(i) | sb_core::model::ResourceRef::ColorImage(i)) => state.contains(i),
                        _ => false,
                    };
                }
                return Ok((p, blobs.into_table()));
            }
            Err(d) => diags.extend(d),
        }
    }
    Err(diags)
}

#[cfg(test)]
mod tests;
