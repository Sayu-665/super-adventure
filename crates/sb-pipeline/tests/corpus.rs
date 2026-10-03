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

const SCRATCH: &str = "/tmp/claude-0/-home-user-super-adventure/14a9b258-2170-5e12-9e2c-1c1a40e3de07/scratchpad";

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
