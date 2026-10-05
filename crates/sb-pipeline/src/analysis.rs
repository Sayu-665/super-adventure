//! Spec steps 7–9: preprocess every stage of a folder (one [`Preprocessor`] per rayon
//! worker), analyze it with `sb_transform::analyze`, and scan the directives.
//!
//! Analysis results are cached by the content of the preprocessed code, so a
//! [`crate::PackSession`] recompiling after an option change only re-parses stages whose
//! code changed.

use crate::directives::{self, ConstDirective, PackDirectiveState};
use crate::resolve::{FolderPlan, StageSource, Unit};
use rayon::prelude::*;
use sb_core::model::{ProgramKind, WorkGroups};
use sb_core::program::{GeometryGroup, GeometryProgram, PassGroup};
use sb_core::{Diagnostic, Diagnostics, ShaderStage, SourceProvider};
use sb_preprocess::{PreprocessOptions, Preprocessed, Preprocessor};
use sb_transform::AnalyzedStage;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Cache key (blake3 hash).
pub type Key = [u8; 32];

/// A two-generation cache shared by the parallel workers of a session: the entries the
/// current compile used and those of the previous compile. [`Generations::rotate`] starts
/// a compile: the previous generation is dropped and the current one becomes the previous
/// one; an entry found in the previous generation moves back into the current one. Memory
/// therefore stays bounded by what two consecutive compiles use, however many recompiles a
/// long-lived session runs, and a recompile reuses everything the compile before it
/// produced. Every session table is one ([`crate::compile::Caches`]).
#[derive(Debug)]
pub struct Generations<V> {
    inner: Mutex<GenerationMaps<V>>,
}

#[derive(Debug)]
struct GenerationMaps<V> {
    current: HashMap<Key, V>,
    previous: HashMap<Key, V>,
}

impl<V> Default for Generations<V> {
    fn default() -> Self {
        Self { inner: Mutex::new(GenerationMaps { current: HashMap::new(), previous: HashMap::new() }) }
    }
}

impl<V: Clone> Generations<V> {
    fn lock(&self) -> std::sync::MutexGuard<'_, GenerationMaps<V>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The entry for `k` (moved into the current generation if it was in the previous one).
    pub fn get(&self, k: &Key) -> Option<V> {
        let mut g = self.lock();
        if let Some(v) = g.current.get(k) {
            return Some(v.clone());
        }
        let v = g.previous.remove(k)?;
        g.current.insert(*k, v.clone());
        Some(v)
    }

    /// Store an entry in the current generation.
    pub fn insert(&self, k: Key, v: V) {
        self.lock().current.insert(k, v);
    }

    /// Start a new generation (see the type docs).
    pub fn rotate(&self) {
        let mut g = self.lock();
        g.previous = std::mem::take(&mut g.current);
    }

    /// Entries in both generations.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        let g = self.lock();
        g.current.len() + g.previous.len()
    }
}

/// Analysis outcome of one stage.
pub type AnalysisResult = Result<Arc<AnalyzedStage>, Diagnostics>;

/// One preprocessed and analyzed stage.
#[derive(Debug, Clone)]
pub struct StageAnalysis {
    pub stage: ShaderStage,
    pub source: StageSource,
    pub pre: Arc<Preprocessed>,
    /// Content key of the analysis (identifies `analyzed`; see [`analysis_key`]).
    pub key: Key,
    /// Const directives of the preprocessed code (fragment and compute stages only; empty
    /// for the others, whose directives Iris ignores).
    pub consts: Arc<Vec<ConstDirective>>,
    pub analyzed: AnalysisResult,
}

/// The stages of every unit of a folder (same order as [`FolderPlan::units`]).
#[derive(Debug, Clone, Default)]
pub struct FolderAnalysis {
    pub units: Vec<Vec<StageAnalysis>>,
}

impl FolderAnalysis {
    /// The analyzed stages of unit `u`, if every stage analyzed successfully.
    pub fn stages_of(&self, u: usize) -> Option<Vec<AnalyzedStage>> {
        let stages = self.units.get(u)?;
        if stages.is_empty() {
            return None;
        }
        stages.iter().map(|s| s.analyzed.as_ref().ok().map(|a| (**a).clone())).collect()
    }

    /// The analysis keys of the stages of unit `u` (with their stages), which identify
    /// the analyzed stages [`FolderAnalysis::stages_of`] returns.
    pub fn stage_keys(&self, u: usize) -> Vec<(ShaderStage, Key)> {
        self.units.get(u).map(|stages| stages.iter().map(|s| (s.stage, s.key)).collect()).unwrap_or_default()
    }

    /// Whether unit `u` analyzed successfully.
    pub fn ok(&self, u: usize) -> bool {
        self.units.get(u).is_some_and(|s| !s.is_empty() && s.iter().all(|x| x.analyzed.is_ok()))
    }

    /// The preprocessed fragment code of unit `u`.
    pub fn fragment_code(&self, u: usize) -> Option<&str> {
        self.units.get(u)?.iter().find(|s| s.stage == ShaderStage::Fragment).map(|s| s.pre.code.as_str())
    }
}

/// Content key of the analysis of a preprocessed stage.
fn analysis_key(stage: ShaderStage, file: &str, pre: &Preprocessed) -> Key {
    let mut h = blake3::Hasher::new();
    h.update(b"sb-analysis-v1\0");
    h.update(stage.name().as_bytes());
    h.update(b"\0");
    h.update(file.as_bytes());
    h.update(b"\0");
    if let Some(v) = &pre.version {
        h.update(v.to_string().as_bytes());
    }
    for e in &pre.extensions {
        h.update(e.to_string().as_bytes());
        h.update(b"\n");
    }
    h.update(b"\0");
    h.update(pre.code.as_bytes());
    for l in &pre.line_map {
        h.update(l.file.as_bytes());
        h.update(&l.line.to_le_bytes());
    }
    *h.finalize().as_bytes()
}

/// Tag diagnostics with a program and stage (keeping existing tags).
pub fn tag(diags: impl IntoIterator<Item = Diagnostic>, program: &str, stage: Option<ShaderStage>) -> Vec<Diagnostic> {
    diags
        .into_iter()
        .map(|mut d| {
            if d.program.is_none() {
                d.program = Some(program.to_string());
            }
            if d.stage.is_none() {
                d.stage = stage;
            }
            d
        })
        .collect()
}

/// A preprocessed stage with what is derived from its text alone.
#[derive(Debug, Clone)]
pub struct PreprocessedStage {
    pub pre: Arc<Preprocessed>,
    /// [`analysis_key`] of `pre`.
    pub key: Key,
    /// Const directives (fragment and compute stages).
    pub consts: Arc<Vec<ConstDirective>>,
}

/// Session caches [`analyze_folder`] uses.
pub struct AnalysisCaches<'a> {
    /// Preprocessed stages, keyed by [`preprocess_key`] (whose sources fingerprint covers
    /// the pack contents, so edits on disk between compiles are picked up).
    pub preprocessed: &'a Generations<PreprocessedStage>,
    /// Analysis results by [`analysis_key`] (content-addressed).
    pub analysis: &'a Generations<AnalysisResult>,
    /// Fingerprint of the option edits the sources apply ([`sources_fingerprint`]).
    pub sources: Key,
}

/// Fingerprint of the sources a compile preprocesses: the pack's contents
/// ([`sb_pack::ShaderPack::content_hash`], which changes when a directory pack is edited
/// on disk between two compiles of a session), the discovered options and the option
/// values whose lines the sources edit.
pub fn sources_fingerprint(pack_content_hash: &str, options: &sb_pack::DiscoveredOptions, values: &sb_pack::OptionValues) -> Key {
    let mut h = blake3::Hasher::new();
    h.update(b"sb-sources-v1\0");
    h.update(pack_content_hash.as_bytes());
    h.update(b"\0");
    // Derived `Debug` shows every field; the maps are ordered.
    h.update(format!("{options:?}").as_bytes());
    h.update(b"\0");
    h.update(values.to_settings_file().as_bytes());
    *h.finalize().as_bytes()
}

/// Key of one preprocessed stage: the sources' fingerprint, the preprocessor options
/// (macros) and the stage source (path, or virtual path and text).
fn preprocess_key(sources: &Key, options: &str, stage: ShaderStage, src: &StageSource) -> Key {
    let mut h = blake3::Hasher::new();
    h.update(b"sb-preprocess-v1\0");
    h.update(sources);
    h.update(&(options.len() as u64).to_le_bytes());
    h.update(options.as_bytes());
    h.update(stage.name().as_bytes());
    h.update(b"\0");
    h.update(format!("{src:?}").as_bytes());
    *h.finalize().as_bytes()
}

/// Preprocess and analyze every stage of `plan` in parallel. Preprocessing results come
/// from `caches.preprocessed` when the same stage was preprocessed with the same macros
/// and option values before (a recompile), analyses from `caches.analysis` when the
/// preprocessed code is unchanged.
pub fn analyze_folder(
    plan: &FolderPlan,
    sources: &dyn SourceProvider,
    macros: &PreprocessOptions,
    caches: &AnalysisCaches<'_>,
    diags: &mut Diagnostics,
) -> FolderAnalysis {
    let jobs: Vec<(usize, ShaderStage, &StageSource)> = plan
        .units
        .iter()
        .enumerate()
        .flat_map(|(u, unit)| unit.stages.iter().map(move |(st, src)| (u, *st, src)))
        .collect();
    let macro_text = format!("{macros:?}");
    let results: Vec<(usize, StageAnalysis, Vec<Diagnostic>)> = jobs
        .par_iter()
        .map_init(
            || Preprocessor::new(sources),
            |pp, &(u, stage, src)| {
                let path = src.path();
                let pkey = preprocess_key(&caches.sources, &macro_text, stage, src);
                let PreprocessedStage { pre, key, consts } = match caches.preprocessed.get(&pkey) {
                    Some(hit) => hit,
                    None => {
                        let pre = match src {
                            StageSource::File(path) => pp.preprocess(path, macros),
                            StageSource::SynthesizedVertex { virtual_path } => {
                                pp.preprocess_source(virtual_path, sb_transform::DEFAULT_VERTEX_SHADER, macros)
                            }
                            StageSource::Fallback { virtual_path, source } => pp.preprocess_source(virtual_path, source, macros),
                        };
                        let key = analysis_key(stage, path, &pre);
                        let consts = match stage {
                            ShaderStage::Fragment | ShaderStage::Compute => directives::find_const_directives(&pre.code),
                            _ => Vec::new(),
                        };
                        let entry = PreprocessedStage { pre: Arc::new(pre), key, consts: Arc::new(consts) };
                        caches.preprocessed.insert(pkey, entry.clone());
                        entry
                    }
                };
                let analyzed = match caches.analysis.get(&key) {
                    Some(r) => r,
                    None => {
                        let r = sb_transform::analyze(stage, &pre, path).map(Arc::new);
                        caches.analysis.insert(key, r.clone());
                        r
                    }
                };
                let unit = &plan.units[u];
                let mut d = tag(pre.diagnostics.iter().cloned(), &unit.path, Some(stage));
                match &analyzed {
                    Ok(a) => d.extend(tag(a.diagnostics.iter().cloned(), &unit.path, Some(stage))),
                    Err(e) => d.extend(tag(e.iter().cloned(), &unit.path, Some(stage))),
                }
                (u, StageAnalysis { stage, source: src.clone(), pre, key, consts, analyzed }, d)
            },
        )
        .collect();
    let mut out = FolderAnalysis { units: vec![Vec::new(); plan.units.len()] };
    for (u, s, d) in results {
        diags.extend(d);
        out.units[u].push(s);
    }
    out
}

/// Directives of a folder (spec step 8).
#[derive(Debug, Clone, Default)]
pub struct FolderDirectives {
    /// Draw buffers per unit (empty for computes), defaults applied, invalid entries dropped.
    pub draw_buffers: Vec<Vec<u32>>,
    /// Whether the draw buffers came from a directive (`false` = class default).
    pub explicit_draw_buffers: Vec<bool>,
    /// `<buf>MipmapEnabled` per unit (composite-style programs).
    pub mipmaps: Vec<Vec<u32>>,
    /// Work groups per compute unit.
    pub work_groups: Vec<Option<WorkGroups>>,
    pub pack: PackDirectiveState,
}

/// Whether a unit renders to shadowcolor targets (shadow programs, `dh_shadow`,
/// shadowcomp).
pub fn writes_shadow_targets(unit: &Unit) -> bool {
    match unit.kind {
        ProgramKind::Geometry { program } => {
            program.group() == GeometryGroup::Shadow || program == GeometryProgram::DhShadow
        }
        ProgramKind::Composite { group, .. } => group == PassGroup::ShadowComp,
        _ => false,
    }
}

/// Draw buffers of a pack's own `dh_shadow`: always shadowcolor0 and shadowcolor1, so that
/// fragment output `i` lands in shadowcolor `i`. Iris builds the DH shadow framebuffer with
/// draw buffers `{0, 1}` and never consults the program's `RENDERTARGETS`/`DRAWBUFFERS`
/// (`IrisRenderingPipeline.createDHFramebufferShadow`); outputs past 1 have no attachment
/// and are discarded. A directive that would route the outputs differently (anything but
/// `0` or `0,1`) gets an info diagnostic, since it is ignored.
fn dh_shadow_draw_buffers(directive: Option<&[u32]>, diags: &mut Diagnostics) -> Vec<u32> {
    const IRIS: [u32; 2] = [0, 1];
    if let Some(dir) = directive
        && !IRIS.starts_with(dir)
    {
        diags.push(Diagnostic::info(
            "dir.dh-shadow-draw-buffers",
            format!(
                "dh_shadow always draws into shadowcolor0 and shadowcolor1 (output i to shadowcolor i, as in Iris); \
                 its draw buffer directive {dir:?} is ignored"
            ),
        ));
    }
    IRIS.to_vec()
}

/// Iris `ProgramId` order of the geometry programs (the order `ProgramSet` scans their
/// const directives), with `final` last.
const IRIS_PROGRAM_ID_ORDER: [&str; 39] = [
    "shadow",
    "shadow_solid",
    "shadow_cutout",
    "shadow_water",
    "shadow_entities",
    "shadow_lightning",
    "shadow_block",
    "gbuffers_basic",
    "gbuffers_line",
    "gbuffers_textured",
    "gbuffers_textured_lit",
    "gbuffers_skybasic",
    "gbuffers_skytextured",
    "gbuffers_clouds",
    "gbuffers_terrain",
    "gbuffers_terrain_solid",
    "gbuffers_terrain_cutout",
    "gbuffers_damagedblock",
    "gbuffers_block",
    "gbuffers_block_translucent",
    "gbuffers_beaconbeam",
    "gbuffers_item",
    "gbuffers_entities",
    "gbuffers_entities_translucent",
    "gbuffers_lightning",
    "gbuffers_particles",
    "gbuffers_particles_translucent",
    "gbuffers_entities_glowing",
    "gbuffers_armor_glint",
    "gbuffers_spidereyes",
    "gbuffers_hand",
    "gbuffers_weather",
    "gbuffers_water",
    "gbuffers_hand_water",
    "dh_terrain",
    "dh_water",
    "dh_generic",
    "dh_shadow",
    "final",
];

/// Scan the directives of every unit, and the pack-level const directives in Iris order:
/// shadowcomp, begin, prepare, the geometry programs (and `final`) in `ProgramId` order,
/// deferred, composite.
pub fn scan_directives(
    plan: &FolderPlan,
    analysis: &FolderAnalysis,
    max_color_attachments: u32,
    diags: &mut Diagnostics,
) -> FolderDirectives {
    let n = plan.units.len();
    let mut out = FolderDirectives {
        draw_buffers: vec![Vec::new(); n],
        explicit_draw_buffers: vec![false; n],
        mipmaps: vec![Vec::new(); n],
        work_groups: vec![None; n],
        pack: PackDirectiveState::default(),
    };
    let max_attachments = max_color_attachments.clamp(1, 8) as usize;
    for (u, unit) in plan.units.iter().enumerate() {
        if unit.is_compute() {
            if let Some(s) = analysis.units[u].first() {
                let mut d = Diagnostics::new();
                out.work_groups[u] = Some(directives::compute_work_groups_in(&s.consts, &mut d, s.source.path()));
                diags.extend(tag(d, &unit.path, Some(ShaderStage::Compute)));
            }
            continue;
        }
        let shadow = writes_shadow_targets(unit);
        let code = analysis.fragment_code(u).unwrap_or("");
        let mut d = Diagnostics::new();
        let parsed = directives::parse_draw_buffers(code, &mut d);
        out.explicit_draw_buffers[u] = parsed.is_some();
        let mut buffers = if unit.geometry() == Some(GeometryProgram::DhShadow) {
            dh_shadow_draw_buffers(parsed.as_deref(), &mut d)
        } else {
            parsed.unwrap_or_else(|| if shadow { vec![0, 1] } else { vec![0] })
        };
        let limit = if shadow { directives::MAX_SHADOWCOLOR } else { directives::MAX_COLORTEX };
        let before = buffers.len();
        buffers.retain(|b| *b < limit);
        if buffers.len() != before {
            d.push(Diagnostic::error(
                "dir.draw-buffer-range",
                format!("draw buffer index out of range (max {}); the output is dropped", limit - 1),
            ));
        }
        if buffers.len() > max_attachments {
            d.push(Diagnostic::error(
                "dir.too-many-draw-buffers",
                format!("{} draw buffers exceed the {max_attachments} colour attachments of a pass; the extra outputs are dropped", buffers.len()),
            ));
            buffers.truncate(max_attachments);
        }
        let mut seen = std::collections::BTreeSet::new();
        if buffers.iter().any(|b| !seen.insert(*b)) {
            d.push(Diagnostic::warning("dir.duplicate-draw-buffer", format!("draw buffers {buffers:?} name a buffer twice")));
        }
        out.draw_buffers[u] = buffers;
        if matches!(unit.kind, ProgramKind::Composite { .. })
            && let Some(f) = analysis.units[u].iter().find(|s| s.stage == ShaderStage::Fragment)
        {
            out.mipmaps[u] = directives::mipmapped_buffers_in(&f.consts);
        }
        diags.extend(tag(d, &unit.path, Some(ShaderStage::Fragment)));
    }

    // Pack-level const directives.
    let mut order: Vec<usize> = Vec::new();
    let composite_group = |g: PassGroup| -> Vec<usize> {
        plan.groups.get(&g).map(|slots| slots.iter().filter_map(|s| s.program).collect()).unwrap_or_default()
    };
    order.extend(composite_group(PassGroup::ShadowComp));
    order.extend(composite_group(PassGroup::Begin));
    order.extend(composite_group(PassGroup::Prepare));
    for name in IRIS_PROGRAM_ID_ORDER {
        if name == "final" {
            order.extend(composite_group(PassGroup::Final));
        } else if let Some(g) = GeometryProgram::from_file_name(name)
            && let Some(&u) = plan.geometry.get(&g)
        {
            order.push(u);
        }
    }
    order.extend(composite_group(PassGroup::Deferred));
    order.extend(composite_group(PassGroup::Composite));
    for u in order {
        let Some(s) = analysis.units[u].iter().find(|s| s.stage == ShaderStage::Fragment) else { continue };
        let mut d = Diagnostics::new();
        for c in s.consts.iter() {
            // Locate by the preprocessed line's original file and line.
            let loc = s.pre.location(c.line as usize).cloned();
            let mut dd = Diagnostics::new();
            out.pack.apply(c, s.source.path(), &mut dd);
            for mut x in dd {
                x.location = loc.clone().or(x.location);
                d.push(x);
            }
        }
        diags.extend(tag(d, &plan.units[u].path, Some(ShaderStage::Fragment)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::load;
    use crate::resolve::resolve_folder;
    use sb_core::model::CompileEnvironment;
    use sb_pack::{OptionValues, ShaderPack};

    #[test]
    fn analyze_and_scan() {
        let pack = ShaderPack::from_files(
            "t",
            [
                ("composite.fsh", "#version 120\n/* RENDERTARGETS: 0,3 */\nconst int colortex3Format = RGBA16F;\nconst bool colortex3MipmapEnabled = true;\nuniform sampler2D colortex0;\nvoid main(){ gl_FragData[0] = texture2D(colortex0, vec2(0.5)); }\n"),
                ("composite.vsh", "#version 120\nvoid main(){ gl_Position = ftransform(); }\n"),
                ("deferred.fsh", "#version 120\nconst int colortex3Format = R11F_G11F_B10F;\nvoid main(){ gl_FragColor = vec4(1.0); }\n"),
                ("shadow.fsh", "#version 120\nvoid main(){ gl_FragData[0] = vec4(1.0); }\n"),
                ("composite1.csh", "#version 430\nlayout(local_size_x = 8) in;\nconst ivec3 workGroups = ivec3(2, 2, 1);\nvoid main(){}\n"),
                ("broken.fsh", "this is not glsl"),
                ("gbuffers_basic.fsh", "#version 120\nvoid main( { }\n"),
            ],
        );
        let env = CompileEnvironment::default();
        let l = load(&pack, &env, &OptionValues::new(), "en_us", None);
        let plan = resolve_folder(&pack, "", &l, &env);
        let sources = crate::sources::OptionSources::new(crate::sources::PackRef::Borrowed(&pack), l.options.clone(), l.values.clone());
        let opts = PreprocessOptions { defines: l.glsl_macros.clone(), ..Default::default() };
        let memo = Generations::default();
        let preprocessed = Generations::default();
        let caches = AnalysisCaches {
            preprocessed: &preprocessed,
            analysis: &memo,
            sources: sources_fingerprint(&pack.content_hash(), &l.options, &l.values),
        };
        let mut d = Diagnostics::new();
        let a = analyze_folder(&plan, &sources, &opts, &caches, &mut d);
        let first: Vec<String> = d.iter().map(|x| x.to_string()).collect();
        let idx = |name: &str| plan.units.iter().position(|u| u.name == name).unwrap();
        assert!(a.ok(idx("composite")));
        assert!(!a.ok(idx("gbuffers_basic")));
        assert!(d.iter().any(|x| x.program.as_deref() == Some("gbuffers_basic") && x.is_error()));
        let dirs = scan_directives(&plan, &a, 8, &mut d);
        assert_eq!(dirs.draw_buffers[idx("composite")], vec![0, 3]);
        assert_eq!(dirs.draw_buffers[idx("deferred")], vec![0]);
        assert_eq!(dirs.draw_buffers[idx("shadow")], vec![0, 1]);
        assert_eq!(dirs.mipmaps[idx("composite")], vec![3]);
        assert_eq!(dirs.work_groups[idx("composite1.csh")], Some(WorkGroups::Absolute { x: 2, y: 2, z: 1 }));
        // Composite is scanned after deferred: its format wins.
        assert_eq!(dirs.pack.colortex[&3].format, Some(sb_core::TextureFormat::RGBA16F));
        // A second run hits the preprocessing and analysis caches, with the same
        // diagnostics.
        preprocessed.rotate();
        let mut d2 = Diagnostics::new();
        let b = analyze_folder(&plan, &sources, &opts, &caches, &mut d2);
        let c = idx("composite");
        assert!(Arc::ptr_eq(&b.units[c][0].pre, &a.units[c][0].pre));
        assert!(Arc::ptr_eq(b.units[c][0].analyzed.as_ref().unwrap(), a.units[c][0].analyzed.as_ref().unwrap()));
        assert_eq!(b.stage_keys(c), a.stage_keys(c));
        assert_eq!(d2.iter().map(|x| x.to_string()).collect::<Vec<_>>(), first);
        // Different macros: preprocessed again (new generation entries), the analysis of
        // unchanged code is still shared.
        let mut opts2 = opts.clone();
        opts2.defines.insert("SB_TEST_MACRO".into(), None);
        let e = analyze_folder(&plan, &sources, &opts2, &caches, &mut Diagnostics::new());
        assert!(!Arc::ptr_eq(&e.units[c][0].pre, &a.units[c][0].pre));
        assert!(Arc::ptr_eq(e.units[c][0].analyzed.as_ref().unwrap(), a.units[c][0].analyzed.as_ref().unwrap()));
        // Two rotations without use drop the entries.
        let n = preprocessed.len();
        assert!(n > 0);
        preprocessed.rotate();
        preprocessed.rotate();
        assert_eq!(preprocessed.len(), 0);
    }
}
