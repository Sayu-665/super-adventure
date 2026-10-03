//! Corpus integration test: every enabled program of every program folder (the root and
//! every dimension folder) of every corpus pack is preprocessed (Iris standard macros +
//! `DISTANT_HORIZONS`), analyzed, laid out, transformed with its default draw profile and
//! compiled with glslang, for two option sets (pack defaults, and "max": every boolean
//! option on, every value option at its last listed value) and four translation
//! variants. Every module is validated with `spirv-val` (when installed) and the
//! reflected stage interfaces are checked against each other (locations, types,
//! flatness).
//!
//! Corpora: `$SB_CORPUS_DIRS` (colon-separated roots) or the scratchpad defaults; the
//! test is skipped when none exists.
//!
//! Environment knobs (for iterating on failures):
//! * `SB_CORPUS_QUICK=1`: defaults, Forward variant only, no `spirv-val`;
//! * `SB_CORPUS_FULL=1`: every variant for both option sets (by default the max set runs
//!   the Forward and Reversed variants only);
//! * `SB_CORPUS_SETS=defaults,max` and `SB_CORPUS_VARIANTS=forward,reversed,renderpearl,dhsynth`
//!   select a subset;
//! * `SB_CORPUS_PACKS=<substring>[,<substring>]` restricts the packs;
//! * `SB_FAIL_DUMP=<dir>` writes the GLSL of failing stages.
//!
//! Failures that are bugs of the packs themselves (code that no GLSL compiler accepts
//! with those option values) are listed in [`KNOWN_PACK_BUGS`]; they are reported but do
//! not count against the pass rate.

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use common::{OptionSet, PackEnv};
use rayon::prelude::*;
use sb_compile::{CompileOptions, Reflection, VulkanTarget, compile_glsl, reflect, validate};
use sb_core::model::{AlphaTest, DepthMode, OutputTarget};
use sb_core::{GeometryProgram, ProgramName, ShaderStage};
use sb_transform::{AnalyzedStage, PackBuilder, TransformOptions};
use sb_uniforms::{ProgramClass, ResourceContext};

/// Marker for synthesized vertex shaders in job file names.
const SYNTH: &str = "<synthesized>";

/// Failures caused by the packs themselves, not by the translation: (pack directory
/// name, option set, substring of the failure message, explanation). Each was checked
/// against the pack sources: the code is invalid GLSL for every compiler with those
/// option values (contradictory option combinations of the "max" set).
const KNOWN_PACK_BUGS: &[(&str, OptionSet, &str, &str)] = &[
    ("arc-shader", OptionSet::Max, "'textureAnisotropic' : no matching", "AF_ENABLED: only gbuffers_terrain includes lib/sampling/anisotropic.glsl"),
    ("arc-shader", OptionSet::Max, "l-value required \"sb_Draw\"", "AF_ENABLED: lib/lighting/basic.glsl writes `spriteBounds`, which gbuffers_textured/weather never declare (it resolves to the read-only OptiFine uniform)"),
    ("arc-shader", OptionSet::Max, "'GetParallaxSlopeNormal'", "the slope normal is defined for PARALLAX_SHAPE_SHARP only but called for every shape"),
    ("arc-shader", OptionSet::Max, "'averageLuminance' : undeclared", "final.fsh reads averageLuminance for the exposure meters but declares it only for automatic exposure"),
    ("arc-shader", OptionSet::Max, "cannot apply to an array: z", "WATER_CAUSTICS reads `lightData.shadowPos.z`, an array with cascaded shadows"),
    ("shrimple", OptionSet::Max, "unexpected identifier `lightInfo`", "composite13 uses struct StaticLightData without including lib/buffers/light_static.glsl"),
    ("shrimple", OptionSet::Max, "interface block `VertexData`", "shadow.gsh declares shadowTilePos only with WORLD_SHADOW_ENABLED, shadow.fsh reads it for every cascaded shadow (Nether/End folders)"),
    ("shrimple", OptionSet::Max, "'hash13' : no matching", "world-1/world1 gbuffers_entities*.vsh call hash13 without including lib/sampling/noise.glsl"),
    ("shrimple", OptionSet::Max, "'GetVoxelBlockPosition' : no matching", "world-1/world1 gbuffers_water.vsh call it without including lib/lighting/voxel/mask.glsl"),
    ("shrimple", OptionSet::Max, "l-value required \"globalLightingData\"", "DYN_LIGHT_DEBUG_COUNTS increments a member of a buffer the program declares readonly"),
    ("i-like-vanilla", OptionSet::Max, "'bloomIntScale' : undeclared", "bloomIntScale is not defined anywhere in the pack"),
    ("bsl-shaders", OptionSet::Max, "'focusPoint' : undeclared", "DOF_FOCUS_POINT reads focusPoint, declared only for DOF_FOCUS_MODE 1"),
    ("bsl-shaders-classic", OptionSet::Max, "'focusPoint' : undeclared", "DOF_FOCUS_POINT reads focusPoint, declared only for DOF_FOCUS_MODE 1"),
    ("ComplementaryReimagined", OptionSet::Max, "'nightMiddleSkyColor' : undeclared", "cloudColors.glsl uses overworld-only sky colors in the Nether/End folders"),
    ("complementary-reimagined", OptionSet::Max, "'nightMiddleSkyColor' : undeclared", "cloudColors.glsl uses overworld-only sky colors in the Nether/End folders"),
    ("complementary-unbound", OptionSet::Max, "'nightMiddleSkyColor' : undeclared", "cloudColors.glsl uses overworld-only sky colors in the Nether/End folders"),
    ("ComplementaryReimagined", OptionSet::Max, "'translucentMult' : undeclared", "WATER_ALPHA_MULT > 100: water.glsl writes translucentMult, which dh_water never declares"),
    ("potato-shaders", OptionSet::Max, "'sRGB_P3D65' : undeclared", "sRGB_P3D65 is not defined anywhere in the pack"),
    ("vanilla-plus-shader", OptionSet::Max, "'sRGB_P3D65' : undeclared", "sRGB_P3D65 is not defined anywhere in the pack"),
    ("redhat-shaders", OptionSet::Max, "'ENTITY_GLOWSTONE' : undeclared", "ENTITY_GLOWSTONE is not defined anywhere in the pack"),
    ("redhat-shaders", OptionSet::Max, "'lenscolor' : redefinition", "two lens-flare options both declare the local `lenscolor` in one scope"),
    ("renderpearl", OptionSet::Max, "'view_size' : no matching", "COMPASS calls view_size(), which the pack never defines"),
    ("spectrum", OptionSet::Max, "'skylightPosY' : undeclared", "skylightPosY is not defined anywhere in the pack"),
    ("spectrum", OptionSet::Max, "'RaytraceIntersection' : no matching", "a 5-argument call; only a 7-argument RaytraceIntersection exists"),
];

/// A translation configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Variant {
    /// Vulkan target, forward [0,1] depth.
    Forward,
    /// Vulkan target, reversed depth with depth-read inversion and comparison-sampler
    /// emulation (the configuration of a renderpearl host without compare samplers).
    Reversed,
    /// Renderpearl target (no set/binding), compiled with glslang auto-mapping.
    Renderpearl,
    /// Synthesized Distant Horizons programs: gbuffers_terrain/water with `dh_terrain`.
    DhSynth,
}

impl Variant {
    const ALL: [Variant; 4] = [Variant::Forward, Variant::Reversed, Variant::Renderpearl, Variant::DhSynth];

    fn key(self) -> &'static str {
        match self {
            Variant::Forward => "forward",
            Variant::Reversed => "reversed",
            Variant::Renderpearl => "renderpearl",
            Variant::DhSynth => "dhsynth",
        }
    }
}

fn set_key(s: OptionSet) -> &'static str {
    match s {
        OptionSet::Defaults => "defaults",
        OptionSet::Max => "max",
    }
}

/// Transform diagnostics of the Forward variant, counted by `severity code`.
static DIAGNOSTICS: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

/// Minimum pass rate per variant and corpus (known pack bugs excluded): every stage
/// with the pack defaults; with the max option set, contradictory option combinations
/// keep surfacing new pack bugs when packs change, so a small margin is allowed.
fn required_pass_rate(set: OptionSet) -> f64 {
    match set {
        OptionSet::Defaults => 100.0,
        OptionSet::Max => 99.0,
    }
}

/// Outcome of one stage.
#[derive(Debug, Clone)]
enum Outcome {
    Ok,
    /// Failed with a categorized message.
    Fail(String),
}

/// One stage result.
struct StageResult {
    /// `<pack>/<file>`.
    label: String,
    /// The program folder (`""` = root).
    folder: String,
    outcome: Outcome,
}

fn class_of(name: &ProgramName) -> ProgramClass {
    match name {
        ProgramName::Geometry { program } => ProgramClass::from_geometry(*program),
        ProgramName::Composite { .. } => ProgramClass::Fullscreen,
    }
}

/// Shorten a failure message into a class key.
fn classify(msg: &str) -> String {
    let m = msg.trim();
    let key: String = m
        .split('\'')
        .enumerate()
        .map(|(i, part)| if i % 2 == 1 && part.len() > 24 { "…" } else { part })
        .collect::<Vec<_>>()
        .join("'");
    key.chars().take(120).collect()
}

struct Analyzed {
    key: String,
    name: ProgramName,
    file: String,
    stage: Result<AnalyzedStage, String>,
}

fn analyze_folder(env: &PackEnv, folder: &str) -> Vec<Analyzed> {
    let set = env.pack.program_set(folder);
    let mut jobs: Vec<(String, ProgramName, ShaderStage, String)> = Vec::new();
    for (key, p) in &set.programs {
        if !env.enabled(folder, key) {
            continue;
        }
        for (st, f) in &p.stages {
            jobs.push((key.clone(), p.name, *st, f.clone()));
        }
        if p.needs_synthesized_vertex() {
            let f = p.stage(ShaderStage::Fragment).unwrap_or_default().replace(".fsh", ".vsh");
            jobs.push((key.clone(), p.name, ShaderStage::Vertex, format!("{SYNTH}{f}")));
        }
        for (letter, f) in &p.computes {
            let k = match letter {
                Some(c) => format!("{key}_{c}"),
                None => format!("{key}#csh"),
            };
            if letter.is_some() && !env.enabled(folder, &k) {
                continue;
            }
            jobs.push((k, p.name, ShaderStage::Compute, f.clone()));
        }
    }
    jobs.par_iter()
        .map(|(key, name, st, f)| {
            let mut pp = sb_preprocess::Preprocessor::new(&env.sources);
            let pre = match f.strip_prefix(SYNTH) {
                Some(path) => pp.preprocess_source(path, sb_transform::DEFAULT_VERTEX_SHADER, &env.options),
                None => pp.preprocess(f, &env.options),
            };
            let stage = sb_transform::analyze(*st, &pre, f)
                .map_err(|d| d.iter().map(|x| format!("[{}] {}", x.code, x.message)).collect::<Vec<_>>().join("; "));
            Analyzed { key: key.clone(), name: *name, file: f.clone(), stage }
        })
        .collect()
}

fn profile_name(name: &ProgramName, variant: Variant) -> &'static str {
    match name {
        _ if variant == Variant::DhSynth => "dh_terrain",
        ProgramName::Geometry { program } => sb_transform::default_profile_for(*program),
        ProgramName::Composite { .. } => sb_transform::FULLSCREEN_PROFILE,
    }
}

fn program_class(name: &ProgramName, compute: bool, variant: Variant) -> ProgramClass {
    if variant == Variant::DhSynth {
        return ProgramClass::Dh;
    }
    if compute && matches!(name, ProgramName::Composite { .. }) { ProgramClass::Compute } else { class_of(name) }
}

/// Check the reflected interfaces of consecutive stages and the fragment outputs.
fn check_interfaces(refl: &[Reflection], frag_outputs: &[(u32, String)]) -> Result<(), String> {
    for pair in refl.windows(2) {
        let (p, c) = (&pair[0], &pair[1]);
        if p.stage == ShaderStage::Compute {
            continue;
        }
        for input in &c.inputs {
            let Some(out) = p.outputs.iter().find(|o| o.location == input.location && o.component == input.component) else {
                return Err(format!("interface: {} input `{}` (location {}) has no {} output", c.stage, input.name, input.location, p.stage));
            };
            let shape = |v: &sb_compile::InterfaceVar| (v.base_type.clone(), v.vec_size, v.columns, v.array_len, v.location_count);
            if shape(out) != shape(input) {
                return Err(format!(
                    "interface: location {} is {:?} in the {} stage but {:?} in the {} stage",
                    input.location,
                    shape(out),
                    p.stage,
                    shape(input),
                    c.stage
                ));
            }
            if out.flat != input.flat && c.stage == ShaderStage::Fragment {
                return Err(format!("interface: flat mismatch at location {}", input.location));
            }
        }
    }
    if let Some(fs) = refl.iter().find(|r| r.stage == ShaderStage::Fragment) {
        let mut locs: Vec<u32> = fs.outputs.iter().flat_map(|o| o.location..o.location + o.location_count.max(1)).collect();
        locs.sort_unstable();
        locs.dedup();
        let want: Vec<u32> = frag_outputs.iter().map(|(l, _)| *l).collect();
        // Outputs the shader never writes are not in the SPIR-V interface.
        if let Some(l) = locs.iter().find(|l| !want.contains(l)) {
            return Err(format!("fragment output location {l} is not reported (reported {want:?})"));
        }
    }
    Ok(())
}

/// Translate and compile every program of one pack for every variant; returns the
/// per-stage outcomes by variant.
fn run_pack(env: &PackEnv, variants: &[Variant], validate_spirv: bool) -> BTreeMap<Variant, Vec<StageResult>> {
    let mut results: BTreeMap<Variant, Vec<StageResult>> = BTreeMap::new();
    for folder in &env.folders {
        let analyzed = analyze_folder(env, folder);
        if analyzed.is_empty() {
            continue;
        }
        for &variant in variants {
            let subset: Vec<&Analyzed> = analyzed
                .iter()
                .filter(|a| {
                    variant != Variant::DhSynth
                        || (matches!(a.name, ProgramName::Geometry { program: GeometryProgram::Terrain | GeometryProgram::Water })
                            && a.stage.as_ref().is_ok_and(|s| s.stage != ShaderStage::Compute))
                })
                .collect();
            let out = results.entry(variant).or_default();
            for (label, outcome) in translate_folder(env, &subset, variant, validate_spirv) {
                out.push(StageResult { label, folder: folder.clone(), outcome });
            }
        }
    }
    results
}

/// Build the pack layout and bindings of one folder and translate its programs.
fn translate_folder(env: &PackEnv, analyzed: &[&Analyzed], variant: Variant, validate_spirv: bool) -> Vec<(String, Outcome)> {
    let declared: Vec<String> = analyzed
        .iter()
        .filter_map(|a| a.stage.as_ref().ok())
        .flat_map(|s| s.info.opaque_uniforms.iter().map(|o| o.name.clone()))
        .collect();
    let rc = ResourceContext::new(ProgramClass::Gbuffers).detect_watershadow(declared.iter().map(String::as_str));
    let mut builder = PackBuilder::new(rc.clone());
    let mut profiles_used: BTreeMap<&'static str, ProgramClass> = BTreeMap::new();
    for a in analyzed {
        let Ok(s) = &a.stage else { continue };
        let class = program_class(&a.name, s.stage == ShaderStage::Compute, variant);
        builder.add_stage(s, class, Some(&a.key));
        if s.stage != ShaderStage::Compute {
            profiles_used.insert(profile_name(&a.name, variant), class);
        }
    }
    for (p, class) in &profiles_used {
        builder.add_profile(sb_transform::profile(p).unwrap(), *class);
    }
    let data = builder.finish();
    let mut programs: BTreeMap<&str, Vec<&Analyzed>> = BTreeMap::new();
    for a in analyzed {
        programs.entry(a.key.as_str()).or_default().push(a);
    }
    let per_program: Vec<Vec<(String, Outcome)>> = programs
        .par_iter()
        .map(|(key, stages)| translate_program(env, key, stages, &data, &rc, variant, validate_spirv))
        .collect();
    per_program.into_iter().flatten().collect()
}

fn translate_program(
    env: &PackEnv,
    key: &str,
    stages: &[&Analyzed],
    data: &sb_transform::PackData,
    rc: &ResourceContext,
    variant: Variant,
    validate_spirv: bool,
) -> Vec<(String, Outcome)> {
    let pack = &env.pack;
    let label = |f: &str| format!("{}/{}", pack.name(), f);
    let mut out = Vec::new();
    let mut ok_stages = Vec::new();
    for a in stages {
        match &a.stage {
            Ok(s) => ok_stages.push(s.clone()),
            Err(e) => out.push((label(&a.file), Outcome::Fail(format!("analyze: {e}")))),
        }
    }
    if let Some((_, Outcome::Fail(first))) = out.first().cloned() {
        for s in ok_stages {
            out.push((label(&s.file), Outcome::Fail(format!("analyze: other stage failed ({first})"))));
        }
        return out;
    }
    let name = stages[0].name;
    let is_compute = ok_stages.iter().any(|s| s.stage == ShaderStage::Compute);
    let prof = sb_transform::profile(profile_name(&name, variant)).unwrap();
    let alpha = match name {
        ProgramName::Geometry { program } if variant != Variant::DhSynth => {
            program.default_alpha_test().map(|(func, reference)| AlphaTest { func, reference })
        }
        _ => None,
    };
    let (depth_mode, invert) = match variant {
        Variant::Reversed => (DepthMode::ReversedZeroToOne, true),
        _ => (DepthMode::ForwardZeroToOne, false),
    };
    let target = if variant == Variant::Renderpearl { OutputTarget::Renderpearl } else { OutputTarget::Vulkan };
    let opts = TransformOptions {
        target,
        depth_mode,
        invert_depth_reads: invert,
        emulate_shadow_samplers: variant == Variant::Reversed,
        program_class: program_class(&name, is_compute, variant),
        alpha_test: alpha,
        is_shadow_pass: matches!(name, ProgramName::Geometry { program } if program.group() == sb_core::program::GeometryGroup::Shadow),
        program_name: Some(format!("{}/{key}", pack.name())),
        ..TransformOptions::default()
    };
    let ctx = data.context(rc);
    let t = match sb_transform::transform_program(&ok_stages, prof, &ctx, &opts) {
        Err(d) => {
            let msg = d.errors().map(|x| format!("[{}] {}", x.code, x.message)).next().unwrap_or_else(|| "unknown".into());
            for s in &ok_stages {
                out.push((label(&s.file), Outcome::Fail(format!("transform: {msg}"))));
            }
            return out;
        }
        Ok(t) => t,
    };
    if variant == Variant::Forward
        && let Ok(mut counts) = DIAGNOSTICS.lock()
    {
        for d in t.diagnostics.iter() {
            *counts.entry(format!("{:?} {}", d.severity, d.code)).or_default() += 1;
        }
    }
    let copts = if target == OutputTarget::Renderpearl { CompileOptions::auto_mapped() } else { CompileOptions::default() };
    let mut reflections = Vec::new();
    let mut failed = false;
    let dump_name = |what: &str| format!("{}__{}__{variant:?}__{}.glsl", pack.name(), env.changed_options, what.replace(['/', '<', '>'], "_"));
    for (s, src) in t.stages.iter().zip(&ok_stages) {
        let dump = |e: &dyn std::fmt::Display| {
            if let Ok(dir) = std::env::var("SB_FAIL_DUMP") {
                let _ = std::fs::write(Path::new(&dir).join(dump_name(&src.file)), format!("// {e}\n{}", s.glsl));
            }
        };
        match compile_glsl(&s.glsl, s.stage, &src.file, &copts, Some(&s.line_map)) {
            Ok(spirv) => {
                if validate_spirv && let sb_compile::ValidationResult::Invalid(m) = validate(&spirv, VulkanTarget::Vulkan1_2) {
                    dump(&m);
                    out.push((label(&src.file), Outcome::Fail(format!("spirv-val: {}", m.lines().next().unwrap_or("")))));
                    failed = true;
                    continue;
                }
                match reflect(&spirv) {
                    Ok(r) => reflections.push(r),
                    Err(e) => {
                        out.push((label(&src.file), Outcome::Fail(format!("reflect: {e}"))));
                        failed = true;
                        continue;
                    }
                }
                out.push((label(&src.file), Outcome::Ok));
            }
            Err(e) => {
                dump(&e);
                let m = e.errors.first().map(|m| m.message.clone()).unwrap_or_default();
                out.push((label(&src.file), Outcome::Fail(format!("glslang: {m}"))));
                failed = true;
            }
        }
    }
    if !failed
        && target == OutputTarget::Vulkan
        && let Err(m) = check_interfaces(&reflections, &t.fragment_outputs)
    {
        if let Ok(dir) = std::env::var("SB_FAIL_DUMP") {
            for s in &t.stages {
                let _ = std::fs::write(Path::new(&dir).join(dump_name(&format!("{key}_{}", s.stage))), format!("// {m}\n{}", s.glsl));
            }
        }
        for o in &mut out {
            o.1 = Outcome::Fail(m.clone());
        }
    }
    out
}

/// The comma-separated values of environment variable `var`, if set.
fn env_list(var: &str) -> Option<Vec<String>> {
    std::env::var(var).ok().map(|v| v.split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).collect())
}

/// Whether a failure is a documented pack bug.
fn known_pack_bug(pack_dir: &str, set: OptionSet, msg: &str) -> bool {
    KNOWN_PACK_BUGS.iter().any(|(p, s, m, _)| *p == pack_dir && *s == set && msg.contains(m))
}

/// Pass counts of one (option set, variant, corpus) cell.
#[derive(Default, Clone, Copy)]
struct Counts {
    total: usize,
    ok: usize,
    /// Known pack bugs (failures excluded from the rate).
    pack_bugs: usize,
    /// Stages of the root and `world0` folders.
    base_total: usize,
    base_ok: usize,
}

#[test]
fn corpus_translate_and_compile() {
    let roots = common::corpus_roots();
    if roots.is_empty() {
        eprintln!("no corpus found (set SB_CORPUS_DIRS); skipping");
        return;
    }
    let quick = std::env::var_os("SB_CORPUS_QUICK").is_some();
    let full = std::env::var_os("SB_CORPUS_FULL").is_some();
    let sets: Vec<OptionSet> = match env_list("SB_CORPUS_SETS") {
        Some(l) => OptionSet::ALL.into_iter().filter(|s| l.iter().any(|x| x == set_key(*s))).collect(),
        None if quick => vec![OptionSet::Defaults],
        None => OptionSet::ALL.to_vec(),
    };
    let chosen_variants: Option<Vec<Variant>> =
        env_list("SB_CORPUS_VARIANTS").map(|l| Variant::ALL.into_iter().filter(|v| l.iter().any(|x| x == v.key())).collect());
    // Default matrix: every variant with the pack defaults; the max option set with the
    // two Vulkan variants (Renderpearl and synthesized DH programs add little coverage
    // there). `SB_CORPUS_FULL=1` runs every cell.
    let variants_for = |set: OptionSet| -> Vec<Variant> {
        match &chosen_variants {
            Some(v) => v.clone(),
            None if quick => vec![Variant::Forward],
            None if full || set == OptionSet::Defaults => Variant::ALL.to_vec(),
            None => vec![Variant::Forward, Variant::Reversed],
        }
    };
    let pack_filter = env_list("SB_CORPUS_PACKS");
    let validate_spirv = !quick && sb_compile::find_tool("spirv-val").is_some();
    let start = std::time::Instant::now();
    let mut counts: BTreeMap<(OptionSet, Variant, &'static str), Counts> = BTreeMap::new();
    let mut classes: BTreeMap<(OptionSet, Variant), BTreeMap<String, (usize, String)>> = BTreeMap::new();
    let mut pack_bug_hits: Vec<String> = Vec::new();
    let mut folder_stats: BTreeMap<OptionSet, (usize, usize)> = BTreeMap::new();
    for root in &roots {
        let corpus: &'static str = if common::SMALL_CORPUS.iter().any(|p| root.ends_with(p)) { "small" } else { "extended" };
        for pack_dir in common::packs_in(root) {
            let dir_name = pack_dir.file_name().unwrap_or_default().to_string_lossy().to_string();
            if let Some(f) = &pack_filter
                && !f.iter().any(|x| dir_name.to_ascii_lowercase().contains(x))
            {
                continue;
            }
            for &set in &sets {
                let Some(env) = PackEnv::open(&pack_dir, set) else {
                    eprintln!("{dir_name}: cannot open");
                    continue;
                };
                let fs = folder_stats.entry(set).or_default();
                fs.0 += 1;
                fs.1 += env.folders.len();
                let variants = variants_for(set);
                let results = run_pack(&env, &variants, validate_spirv);
                for (variant, stages) in results {
                    let c = counts.entry((set, variant, corpus)).or_default();
                    let ok = stages.iter().filter(|r| matches!(r.outcome, Outcome::Ok)).count();
                    if variant == variants[0] {
                        let folders: Vec<&str> = env.folders.iter().map(|f| if f.is_empty() { "<root>" } else { f.as_str() }).collect();
                        eprintln!(
                            "{:9} {:36} {ok:5}/{:5}  ({} options changed; folders {})",
                            set_key(set),
                            dir_name,
                            stages.len(),
                            env.changed_options,
                            folders.join(" ")
                        );
                    }
                    for r in stages {
                        c.total += 1;
                        let base = r.folder.is_empty() || r.folder == "world0";
                        if base {
                            c.base_total += 1;
                        }
                        match r.outcome {
                            Outcome::Ok => {
                                c.ok += 1;
                                if base {
                                    c.base_ok += 1;
                                }
                            }
                            Outcome::Fail(m) if known_pack_bug(&dir_name, set, &m) => {
                                c.pack_bugs += 1;
                                pack_bug_hits.push(format!("{} {variant:?} {}: {m}", set_key(set), r.label));
                            }
                            Outcome::Fail(m) => {
                                let e = classes.entry((set, variant)).or_default().entry(classify(&m)).or_insert((0, r.label.clone()));
                                e.0 += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    eprintln!("---- elapsed {:.1?} (spirv-val: {validate_spirv})", start.elapsed());
    for (set, (packs, folders)) in &folder_stats {
        eprintln!("{}: {packs} packs, {folders} program folders", set_key(*set));
    }
    let mut below: Vec<String> = Vec::new();
    for ((set, variant, corpus), c) in &counts {
        let counted = c.total - c.pack_bugs;
        let pct = if counted == 0 { 100.0 } else { 100.0 * c.ok as f64 / counted as f64 };
        let other_total = c.total - c.base_total;
        let other_ok = c.ok - c.base_ok;
        eprintln!(
            "{} {variant:?} {corpus}: {}/{counted} stages compile ({pct:.2}%); root+world0 {}/{}, other dimension folders {other_ok}/{other_total}; known pack bugs {}",
            set_key(*set),
            c.ok,
            c.base_ok,
            c.base_total,
            c.pack_bugs
        );
        if pct < required_pass_rate(*set) {
            below.push(format!("{} {variant:?} {corpus}: {pct:.2}% (required {}%)", set_key(*set), required_pass_rate(*set)));
        }
    }
    for ((set, variant), cl) in &classes {
        let mut cv: Vec<_> = cl.iter().collect();
        cv.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
        eprintln!("failure classes, {} {variant:?}:", set_key(*set));
        for (k, (n, example)) in cv.iter().take(25) {
            eprintln!("    {n:5}  {k}   [e.g. {example}]");
        }
    }
    if !pack_bug_hits.is_empty() {
        eprintln!("known pack bugs hit ({}):", pack_bug_hits.len());
        for h in &pack_bug_hits {
            eprintln!("    {h}");
        }
    }
    if let Ok(counts) = DIAGNOSTICS.lock() {
        eprintln!("transform diagnostics (Forward variant, per program):");
        for (k, n) in counts.iter() {
            eprintln!("    {n:6}  {k}");
        }
    }
    assert!(below.is_empty(), "pass rate too low: {below:?}");
}

/// Debug helper: `SB_DUMP=<pack dir>:<file> cargo test --test corpus dump -- --ignored`
/// writes the preprocessed code to `$SB_DUMP_OUT` (default `/tmp/sb_dump.glsl`);
/// `SB_DUMP_MAX=1` applies the max option set.
#[test]
#[ignore]
fn dump() {
    let Ok(spec) = std::env::var("SB_DUMP") else { return };
    let (dir, file) = spec.rsplit_once(':').expect("SB_DUMP=<pack dir>:<file>");
    let set = if std::env::var_os("SB_DUMP_MAX").is_some() { OptionSet::Max } else { OptionSet::Defaults };
    let env = PackEnv::open(Path::new(dir), set).expect("open pack");
    let mut pp = sb_preprocess::Preprocessor::new(&env.sources);
    let pre = pp.preprocess(file, &env.options);
    let out = std::env::var("SB_DUMP_OUT").unwrap_or_else(|_| "/tmp/sb_dump.glsl".into());
    std::fs::write(&out, &pre.code).unwrap();
    let ext = Path::new(file).extension().and_then(|e| e.to_str()).unwrap_or("fsh");
    let stage = ShaderStage::from_pack_extension(ext).unwrap_or(ShaderStage::Fragment);
    match sb_transform::analyze(stage, &pre, file) {
        Ok(_) => eprintln!("analyze ok"),
        Err(d) => eprintln!("{d:?}"),
    }
}
