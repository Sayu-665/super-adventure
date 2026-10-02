//! Corpus integration test: every enabled program of every corpus pack is
//! preprocessed (Iris standard macros + `DISTANT_HORIZONS`), analyzed, laid out,
//! transformed with its default draw profile and compiled with glslang. Every
//! module is validated with `spirv-val` (when installed) and the reflected stage
//! interfaces are checked against each other (locations, types, flatness).
//!
//! Corpora: `$SB_CORPUS_DIRS` (colon-separated roots) or the scratchpad defaults; the
//! test is skipped when none exists. Set `SB_FAIL_DUMP=<dir>` to write the GLSL of
//! failing stages, `SB_CORPUS_QUICK=1` to run only the default variant without
//! `spirv-val`.

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use rayon::prelude::*;
use sb_compile::{CompileOptions, Reflection, VulkanTarget, compile_glsl, reflect, validate};
use sb_core::model::{AlphaTest, DepthMode, OutputTarget};
use sb_core::{GeometryProgram, ProgramName, ShaderStage};
use sb_transform::{AnalyzedStage, PackBuilder, TransformOptions};
use sb_uniforms::{ProgramClass, ResourceContext};

/// Marker for synthesized vertex shaders in job file names.
const SYNTH: &str = "<synthesized>";

/// A translation configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    /// Vulkan target, forward [0,1] depth.
    Forward,
    /// Vulkan target, reversed depth with depth-read inversion.
    Reversed,
    /// Renderpearl target (no set/binding), compiled with glslang auto-mapping.
    Renderpearl,
    /// Synthesized Distant Horizons programs: gbuffers_terrain/water with `dh_terrain`.
    DhSynth,
}

/// Transform diagnostics of the default variant, counted by `severity code`.
static DIAGNOSTICS: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

/// Minimum pass rate per variant and corpus.
const REQUIRED_PASS_RATE: f64 = 99.0;

/// Outcome of one stage.
#[derive(Debug, Clone)]
enum Outcome {
    Ok,
    /// Failed with a categorized message.
    Fail(String),
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

fn analyze_folder(pack: &Arc<sb_pack::ShaderPack>, env: &common::PackEnv, folder: &str) -> Vec<Analyzed> {
    let set = pack.program_set(folder);
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
            let src = common::PackSources(pack.clone());
            let mut pp = sb_preprocess::Preprocessor::new(&src);
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

/// Translate and compile every program of one pack; returns per-stage outcomes.
fn run_pack(pack_dir: &Path, variant: Variant, validate_spirv: bool) -> Vec<(String, Outcome)> {
    let Ok(pack) = sb_pack::ShaderPack::open(pack_dir) else { return Vec::new() };
    let pack = Arc::new(pack);
    let env = common::PackEnv::new(&pack);
    let mut results: Vec<(String, Outcome)> = Vec::new();
    for folder in &env.folders {
        let mut analyzed = analyze_folder(&pack, &env, folder);
        if analyzed.is_empty() {
            continue;
        }
        if variant == Variant::DhSynth {
            analyzed.retain(|a| {
                matches!(a.name, ProgramName::Geometry { program: GeometryProgram::Terrain | GeometryProgram::Water })
                    && a.stage.as_ref().is_ok_and(|s| s.stage != ShaderStage::Compute)
            });
        }
        // Pack layout and bindings.
        let declared: Vec<String> = analyzed
            .iter()
            .filter_map(|a| a.stage.as_ref().ok())
            .flat_map(|s| s.info.opaque_uniforms.iter().map(|o| o.name.clone()))
            .collect();
        let rc = ResourceContext::new(ProgramClass::Gbuffers).detect_watershadow(declared.iter().map(String::as_str));
        let mut builder = PackBuilder::new(rc.clone());
        let mut profiles_used: BTreeMap<&'static str, ProgramClass> = BTreeMap::new();
        for a in &analyzed {
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
        let mut programs: BTreeMap<String, Vec<Analyzed>> = BTreeMap::new();
        for a in analyzed {
            programs.entry(a.key.clone()).or_default().push(a);
        }
        let folder_results: Vec<Vec<(String, Outcome)>> = programs
            .par_iter()
            .map(|(key, stages)| translate_program(&pack, key, stages, &data, &rc, variant, validate_spirv))
            .collect();
        results.extend(folder_results.into_iter().flatten());
    }
    results
}

fn translate_program(
    pack: &sb_pack::ShaderPack,
    key: &str,
    stages: &[Analyzed],
    data: &sb_transform::PackData,
    rc: &ResourceContext,
    variant: Variant,
    validate_spirv: bool,
) -> Vec<(String, Outcome)> {
    let label = |f: &str| format!("{}/{}", pack.name(), f);
    let mut out = Vec::new();
    let mut ok_stages = Vec::new();
    for a in stages {
        match &a.stage {
            Ok(s) => ok_stages.push(s.clone()),
            Err(e) => out.push((label(&a.file), Outcome::Fail(format!("analyze: {e}")))),
        }
    }
    if !out.is_empty() {
        for s in ok_stages {
            out.push((label(&s.file), Outcome::Fail("analyze: other stage failed".into())));
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
    for (s, src) in t.stages.iter().zip(&ok_stages) {
        let dump = |e: &dyn std::fmt::Display| {
            if let Ok(dir) = std::env::var("SB_FAIL_DUMP") {
                let p = Path::new(&dir).join(format!("{}__{variant:?}__{}.glsl", pack.name(), src.file.replace(['/', '<', '>'], "_")));
                let _ = std::fs::write(p, format!("// {e}\n{}", s.glsl));
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
                let p = Path::new(&dir).join(format!("{}__{variant:?}__{key}_{}.glsl", pack.name(), s.stage));
                let _ = std::fs::write(p, format!("// {m}\n{}", s.glsl));
            }
        }
        for o in &mut out {
            o.1 = Outcome::Fail(m.clone());
        }
    }
    out
}

#[test]
fn corpus_translate_and_compile() {
    let roots = common::corpus_roots();
    if roots.is_empty() {
        eprintln!("no corpus found (set SB_CORPUS_DIRS); skipping");
        return;
    }
    let quick = std::env::var_os("SB_CORPUS_QUICK").is_some();
    let variants: &[Variant] =
        if quick { &[Variant::Forward] } else { &[Variant::Forward, Variant::Reversed, Variant::Renderpearl, Variant::DhSynth] };
    let validate_spirv = !quick && sb_compile::find_tool("spirv-val").is_some();
    let start = std::time::Instant::now();
    let mut summary: Vec<String> = Vec::new();
    let mut below: Vec<String> = Vec::new();
    for &variant in variants {
        let mut by_root: Vec<(String, usize, usize)> = Vec::new();
        let mut classes: BTreeMap<String, (usize, String)> = BTreeMap::new();
        for root in &roots {
            let label = if common::SMALL_CORPUS.iter().any(|p| root.ends_with(p)) { "small".to_string() } else { "extended".to_string() };
            let idx = match by_root.iter().position(|(l, ..)| *l == label) {
                Some(i) => i,
                None => {
                    by_root.push((label.clone(), 0, 0));
                    by_root.len() - 1
                }
            };
            for pack_dir in common::packs_in(root) {
                let results = run_pack(&pack_dir, variant, validate_spirv);
                let ok = results.iter().filter(|r| matches!(r.1, Outcome::Ok)).count();
                if variant == Variant::Forward {
                    eprintln!("{:40} {ok:5}/{:5}", pack_dir.file_name().unwrap_or_default().to_string_lossy(), results.len());
                }
                by_root[idx].1 += results.len();
                by_root[idx].2 += ok;
                for (f, o) in results {
                    if let Outcome::Fail(m) = o {
                        let e = classes.entry(classify(&m)).or_insert((0, f.clone()));
                        e.0 += 1;
                    }
                }
            }
        }
        for (label, stages, ok) in &by_root {
            let pct = if *stages == 0 { 100.0 } else { 100.0 * *ok as f64 / *stages as f64 };
            summary.push(format!("{variant:?} {label}: {ok}/{stages} stages compile ({pct:.2}%)"));
            if pct < REQUIRED_PASS_RATE {
                below.push(format!("{variant:?} {label}: {pct:.2}%"));
            }
        }
        let mut cv: Vec<_> = classes.into_iter().collect();
        cv.sort_by(|a, b| b.1.0.cmp(&a.1.0));
        for (k, (n, example)) in cv.iter().take(25) {
            summary.push(format!("    {n:5}  {k}   [e.g. {example}]"));
        }
    }
    eprintln!("---- elapsed {:.1?} (spirv-val: {validate_spirv})", start.elapsed());
    for s in &summary {
        eprintln!("{s}");
    }
    if let Ok(counts) = DIAGNOSTICS.lock() {
        eprintln!("transform diagnostics (Forward variant, per program):");
        for (k, n) in counts.iter() {
            eprintln!("    {n:6}  {k}");
        }
    }
    assert!(below.is_empty(), "pass rate below {REQUIRED_PASS_RATE}%: {below:?}");
}

/// Debug helper: `SB_DUMP=<pack dir>:<file> cargo test --test corpus dump -- --ignored`
/// writes the preprocessed code to `$SB_DUMP_OUT` (default `/tmp/sb_dump.glsl`).
#[test]
#[ignore]
fn dump() {
    let Ok(spec) = std::env::var("SB_DUMP") else { return };
    let (dir, file) = spec.rsplit_once(':').expect("SB_DUMP=<pack dir>:<file>");
    let pack = Arc::new(sb_pack::ShaderPack::open(Path::new(dir)).expect("open pack"));
    let options = common::pack_options(&pack);
    let src = common::PackSources(pack);
    let mut pp = sb_preprocess::Preprocessor::new(&src);
    let pre = pp.preprocess(file, &options);
    let out = std::env::var("SB_DUMP_OUT").unwrap_or_else(|_| "/tmp/sb_dump.glsl".into());
    std::fs::write(&out, &pre.code).unwrap();
    let ext = Path::new(file).extension().and_then(|e| e.to_str()).unwrap_or("fsh");
    let stage = ShaderStage::from_pack_extension(ext).unwrap_or(ShaderStage::Fragment);
    match sb_transform::analyze(stage, &pre, file) {
        Ok(_) => eprintln!("analyze ok"),
        Err(d) => eprintln!("{d:?}"),
    }
}
