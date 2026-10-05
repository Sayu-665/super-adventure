//! Corpus integration test: `compile_pack` on every pack of the small corpus with default
//! settings (and one pack in both depth modes). Asserts that nothing panics, every program
//! is accounted for (compiled, or diagnosed as failed), the DH strategy matches the pack,
//! and the model round-trips through JSON; prints the pass rate.
//!
//! Corpus roots: `$SB_CORPUS_DIRS` (colon-separated) or the scratchpad small corpus; set
//! `SB_PIPELINE_FULL_CORPUS=1` to add the extended corpus and `SB_PIPELINE_VALIDATE=1` to
//! run `spirv-val`. Skipped when no corpus exists.

use sb_core::model::{CompiledPack, DepthMode, DhStrategy};
use sb_pipeline::{CompileSettings, compile_pack, inspect};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const SCRATCH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test-data");

/// Packs without Distant Horizons programs (DH is synthesized for them).
const SYNTHESIZED_DH: &[&str] = &["spectrum", "Ominous-Shaderpack", "MinecraftShaderProgramming"];

fn roots() -> Vec<PathBuf> {
    if let Some(v) = std::env::var_os("SB_CORPUS_DIRS") {
        return std::env::split_paths(&v).filter(|p| p.is_dir()).collect();
    }
    let mut out = vec![Path::new(SCRATCH).join("corpus")];
    if std::env::var_os("SB_PIPELINE_FULL_CORPUS").is_some() {
        out.push(Path::new(SCRATCH).join("corpus2"));
    }
    out.into_iter().filter(|p| p.is_dir()).collect()
}

/// Directories containing `shaders/` (up to three levels below `root`).
fn packs_in(root: &Path) -> Vec<PathBuf> {
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

/// Every program the pack resolves (per `inspect`) is compiled or reported as failed.
fn check_accounted(pack_dir: &Path, model: &CompiledPack, failed: &[String]) {
    let pack = sb_pack::ShaderPack::open(pack_dir).unwrap();
    let summary = inspect(&pack);
    let compiled: BTreeSet<&str> = model.dimensions.iter().flat_map(|d| d.programs.iter().map(|p| p.name.as_str())).collect();
    for f in &summary.folders {
        for p in &f.programs {
            let path = if f.folder.is_empty() { p.name.clone() } else { format!("{}/{}", f.folder, p.name) };
            let is_failed = failed.iter().any(|x| x.split(' ').next() == Some(path.as_str()));
            // Geometry programs that no slot needs and DH programs without DH are not
            // compiled; everything else must be either compiled or reported.
            let dh = p.name.starts_with("dh_");
            assert!(
                compiled.contains(path.as_str()) || is_failed || dh,
                "{}: program `{path}` is neither compiled nor reported as failed",
                pack_dir.display()
            );
        }
    }
}

/// Every descriptor of every SPIR-V module has a binding-table entry at its set/binding
/// with the reflected kind: the table describes what the shaders declare (ARCHITECTURE
/// §5.2; rectangle samplers as 2D, emulated comparison samplers as plain samplers).
fn check_binding_kinds(pack_dir: &Path, out: &sb_pipeline::CompileOutput) {
    use sb_core::model::ResourceKind;
    for dim in &out.pack.dimensions {
        for p in &dim.programs {
            for st in &p.stages {
                let Some(words) = st.spirv.and_then(|b| out.blobs.get_spirv(b)) else { continue };
                let Ok(refl) = sb_compile::reflect(&words) else { panic!("{}: {} does not reflect", pack_dir.display(), p.name) };
                for d in &refl.descriptors {
                    let Some(reflected) = sb_pipeline::resource_kind_of(&d.kind) else { continue };
                    if matches!(reflected, ResourceKind::UniformBuffer | ResourceKind::StorageBuffer) {
                        continue;
                    }
                    let Some(e) = dim.bindings.entries.iter().find(|e| e.set == d.set && e.binding == d.binding) else {
                        panic!("{}: {}: no binding-table entry for `{}` at {}/{}", pack_dir.display(), p.name, d.name, d.set, d.binding)
                    };
                    // Storage-image access qualifiers are per declaration; compare the rest.
                    let shape = |k: &ResourceKind| match k {
                        ResourceKind::StorageImage { dim, sample_type, .. } => format!("image {dim} {sample_type}"),
                        k => format!("{k:?}"),
                    };
                    assert_eq!(shape(&e.kind), shape(&reflected), "{}: {}: `{}`", pack_dir.display(), p.name, e.name);
                }
            }
        }
    }
}

#[test]
fn corpus_compiles() {
    let roots = roots();
    if roots.is_empty() {
        eprintln!("no corpus found; skipping");
        return;
    }
    let validate = std::env::var_os("SB_PIPELINE_VALIDATE").is_some();
    let mut total_ok = 0usize;
    let mut total_failed = 0usize;
    let mut rows = Vec::new();
    let mut depth_checked = false;
    for root in &roots {
        for dir in packs_in(root) {
            let Ok(pack) = sb_pack::ShaderPack::open(&dir) else { continue };
            let settings = CompileSettings { validate_spirv: validate, ..Default::default() };
            let out = compile_pack(&pack, &settings);
            total_ok += out.stats.programs_ok;
            total_failed += out.stats.programs_failed.len();
            rows.push(format!(
                "{:40} ok {:4} failed {:3} modules {:4} errors {:4} {:.0} ms",
                pack.name(),
                out.stats.programs_ok,
                out.stats.programs_failed.len(),
                out.stats.modules,
                out.pack.diagnostics.errors().count(),
                out.timings.total_ms
            ));
            assert!(!out.pack.dimensions.is_empty(), "{}: no dimension compiled", dir.display());
            check_accounted(&dir, &out.pack, &out.stats.programs_failed);

            // DH strategy.
            let rel = dir.strip_prefix(root).unwrap_or(&dir).to_string_lossy().to_string();
            let synth = SYNTHESIZED_DH.iter().any(|p| rel.starts_with(p));
            for d in &out.pack.dimensions {
                let has_dh = pack.program_set(&d.folder).programs.keys().any(|k| k == "dh_terrain" || k == "dh_water");
                let expected = if has_dh { DhStrategy::Native } else { DhStrategy::Synthesized };
                assert_eq!(d.distant_horizons.strategy, expected, "{} {}", dir.display(), d.folder);
                if synth {
                    assert_eq!(d.distant_horizons.strategy, DhStrategy::Synthesized, "{}", dir.display());
                    assert!(d.distant_horizons.unified_projection);
                }
            }

            check_binding_kinds(&dir, &out);

            // JSON round trip.
            let json = out.pack.to_json();
            let back = CompiledPack::from_json(&json).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            assert_eq!(back, out.pack, "{}: JSON round trip", dir.display());

            // One pack in reversed-Z as well.
            if !depth_checked && pack.name() == "ComplementaryReimagined" {
                depth_checked = true;
                let mut s = settings.clone();
                s.env.depth_mode = DepthMode::ReversedZeroToOne;
                let rev = compile_pack(&pack, &s);
                assert_eq!(rev.stats.programs_ok, out.stats.programs_ok, "reversed-Z compiles the same programs");
                assert_eq!(rev.pack.info.environment.depth_mode, DepthMode::ReversedZeroToOne);
                // Hosts without comparison samplers: emulated shadow lookups compile too,
                // and the table describes the plain samplers the shaders declare.
                s.env.device.comparison_samplers = false;
                let emulated = compile_pack(&pack, &s);
                assert_eq!(emulated.stats.programs_ok, out.stats.programs_ok, "shadow emulation compiles the same programs");
                check_binding_kinds(&dir, &emulated);
                let flagged = emulated.pack.dimensions.iter().flat_map(|d| &d.programs).flat_map(|p| &p.bindings_used).filter(|b| b.shadow_emulated).count();
                assert!(flagged > 0, "{}: no emulated comparison sampler", dir.display());
            }
        }
    }
    for r in &rows {
        eprintln!("{r}");
    }
    let rate = if total_ok + total_failed == 0 { 100.0 } else { 100.0 * total_ok as f64 / (total_ok + total_failed) as f64 };
    eprintln!("pass rate: {total_ok}/{} programs ({rate:.2}%)", total_ok + total_failed);
    assert!(rate >= 99.0, "pass rate {rate:.2}% below 99%");
}

/// Incremental recompiles through a [`PackSession`]: recompiling with unchanged settings
/// reuses the session's caches and produces exactly the cold compile's output. Prints the
/// cold and warm timings (`SB_TIMING_PACK` selects the pack directory; default: photon
/// of the small corpus). Ignored by default (it is a measurement); run with
/// `cargo test -p sb-pipeline --test corpus -- --ignored --nocapture session_recompile`.
#[test]
#[ignore]
fn session_recompile_timing() {
    let dir = std::env::var_os("SB_TIMING_PACK").map(PathBuf::from).unwrap_or_else(|| Path::new(SCRATCH).join("corpus/photon"));
    let Ok(pack) = sb_pack::ShaderPack::open(&dir) else {
        eprintln!("{} not found; skipping", dir.display());
        return;
    };
    let mut session = sb_pipeline::PackSession::new(&pack, CompileSettings::default());
    let t = std::time::Instant::now();
    let cold = session.compile();
    let cold_ms = t.elapsed().as_secs_f64() * 1e3;
    let mut warm_ms = Vec::new();
    for _ in 0..2 {
        session.set_settings(CompileSettings::default());
        let t = std::time::Instant::now();
        let warm = session.compile();
        warm_ms.push(t.elapsed().as_secs_f64() * 1e3);
        assert_eq!(warm.pack, cold.pack, "warm recompile differs from the cold compile");
        assert_eq!(warm.blobs.blobs, cold.blobs.blobs, "warm recompile blobs differ");
    }
    // An option change: the session reuses what the change does not affect and still
    // matches a one-shot compile with the new values.
    let option = cold.pack.options.options.iter().find(|o| o.kind == sb_core::model::OptionKind::BooleanDefine);
    let mut changed_ms = None;
    if let Some(o) = option {
        let flipped = if o.value == "true" { "false" } else { "true" };
        let settings = CompileSettings { option_values: sb_pack::OptionValues::from_pairs([(o.name.as_str(), flipped)]), ..Default::default() };
        session.set_settings(settings.clone());
        let t = std::time::Instant::now();
        let incremental = session.compile();
        changed_ms = Some((o.name.clone(), t.elapsed().as_secs_f64() * 1e3));
        let fresh = compile_pack(&pack, &settings);
        assert_eq!(incremental.pack, fresh.pack, "incremental compile after changing {} differs from a fresh compile", o.name);
        assert_eq!(incremental.blobs.blobs, fresh.blobs.blobs, "incremental blobs after changing {} differ", o.name);
    }
    eprintln!(
        "{}: cold {cold_ms:.0} ms (load {:.0} ms), warm {:?} ms, one option changed {:?}, {} programs",
        pack.name(),
        cold.timings.load_ms,
        warm_ms.iter().map(|m| m.round() as u64).collect::<Vec<_>>(),
        changed_ms.map(|(n, ms)| format!("{n}: {ms:.0} ms")),
        cold.stats.programs_ok
    );
}
