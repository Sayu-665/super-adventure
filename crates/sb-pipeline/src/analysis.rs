//! Spec steps 7–9: preprocess every stage of a folder (one [`Preprocessor`] per rayon
//! worker), analyze it with `sb_transform::analyze`, and scan the directives.
//!
//! Analysis results are cached by the content of the preprocessed code, so a
//! [`crate::PackSession`] recompiling after an option change only re-parses stages whose
//! code changed.

use crate::directives::{self, PackDirectiveState};
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

/// A bounded memo table shared by the parallel workers of a session.
#[derive(Debug)]
pub struct Memo<V> {
    map: Mutex<HashMap<Key, V>>,
}

impl<V> Default for Memo<V> {
    fn default() -> Self {
        Self { map: Mutex::new(HashMap::new()) }
    }
}

impl<V: Clone> Memo<V> {
    /// Entries beyond which the table is cleared (a session over many option changes
    /// must not grow without bound).
    const LIMIT: usize = 20_000;

    pub fn get(&self, k: &Key) -> Option<V> {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).get(k).cloned()
    }

    pub fn insert(&self, k: Key, v: V) {
        let mut m = self.map.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() >= Self::LIMIT {
            m.clear();
        }
        m.insert(k, v);
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

    /// Whether unit `u` analyzed successfully.
    pub fn ok(&self, u: usize) -> bool {
        self.units.get(u).is_some_and(|s| !s.is_empty() && s.iter().all(|x| x.analyzed.is_ok()))
    }

    /// The preprocessed fragment code of unit `u`.
    pub fn fragment_code(&self, u: usize) -> Option<&str> {
        self.units.get(u)?.iter().find(|s| s.stage == ShaderStage::Fragment).map(|s| s.pre.code.as_str())
    }
}

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

/// Preprocess and analyze every stage of `plan` in parallel.
pub fn analyze_folder(
    plan: &FolderPlan,
    sources: &dyn SourceProvider,
    macros: &PreprocessOptions,
    memo: &Memo<AnalysisResult>,
    diags: &mut Diagnostics,
) -> FolderAnalysis {
    let jobs: Vec<(usize, ShaderStage, &StageSource)> = plan
        .units
        .iter()
        .enumerate()
        .flat_map(|(u, unit)| unit.stages.iter().map(move |(st, src)| (u, *st, src)))
        .collect();
    let results: Vec<(usize, StageAnalysis, Vec<Diagnostic>)> = jobs
        .par_iter()
        .map_init(
            || Preprocessor::new(sources),
            |pp, &(u, stage, src)| {
                let pre = match src {
                    StageSource::File(path) => pp.preprocess(path, macros),
                    StageSource::SynthesizedVertex { virtual_path } => {
                        pp.preprocess_source(virtual_path, sb_transform::DEFAULT_VERTEX_SHADER, macros)
                    }
                    StageSource::Fallback { virtual_path, source } => pp.preprocess_source(virtual_path, source, macros),
                };
                let pre = Arc::new(pre);
                let path = src.path();
                let key = analysis_key(stage, path, &pre);
                let analyzed = match memo.get(&key) {
                    Some(r) => r,
                    None => {
                        let r = sb_transform::analyze(stage, &pre, path).map(Arc::new);
                        memo.insert(key, r.clone());
                        r
                    }
                };
                let unit = &plan.units[u];
                let mut d = tag(pre.diagnostics.iter().cloned(), &unit.path, Some(stage));
                match &analyzed {
                    Ok(a) => d.extend(tag(a.diagnostics.iter().cloned(), &unit.path, Some(stage))),
                    Err(e) => d.extend(tag(e.iter().cloned(), &unit.path, Some(stage))),
                }
                (u, StageAnalysis { stage, source: src.clone(), pre, analyzed }, d)
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
                out.work_groups[u] = Some(directives::compute_work_groups(&s.pre.code, &mut d, s.source.path()));
                diags.extend(tag(d, &unit.path, Some(ShaderStage::Compute)));
            }
            continue;
        }
        let shadow = writes_shadow_targets(unit);
        let code = analysis.fragment_code(u).unwrap_or("");
        let mut d = Diagnostics::new();
        let parsed = directives::parse_draw_buffers(code, &mut d);
        out.explicit_draw_buffers[u] = parsed.is_some();
        let mut buffers = parsed.unwrap_or_else(|| if shadow { vec![0, 1] } else { vec![0] });
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
        if matches!(unit.kind, ProgramKind::Composite { .. }) {
            out.mipmaps[u] = directives::mipmapped_buffers(code);
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
        for c in directives::find_const_directives(&s.pre.code) {
            // Locate by the preprocessed line's original file and line.
            let loc = s.pre.location(c.line as usize).cloned();
            let mut dd = Diagnostics::new();
            out.pack.apply(&c, s.source.path(), &mut dd);
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
        let memo = Memo::default();
        let mut d = Diagnostics::new();
        let a = analyze_folder(&plan, &sources, &opts, &memo, &mut d);
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
        // A second run hits the analysis cache.
        let mut d2 = Diagnostics::new();
        let b = analyze_folder(&plan, &sources, &opts, &memo, &mut d2);
        assert!(Arc::ptr_eq(
            b.units[idx("composite")][0].analyzed.as_ref().unwrap(),
            a.units[idx("composite")][0].analyzed.as_ref().unwrap()
        ));
    }
}
