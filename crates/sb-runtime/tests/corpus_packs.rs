//! Real shader packs from the corpus, compiled with `sb_pipeline::compile_pack` and
//! rendered on lavapipe with validation. These need the `pipeline-tests` feature (which
//! pulls in sb-pipeline) and are `#[ignore]`d because they take minutes:
//!
//! ```text
//! VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
//!   cargo test -p sb-runtime --features pipeline-tests --test corpus_packs -- --ignored --nocapture
//! ```
//!
//! Packs are looked up in the colon-separated roots of `SB_CORPUS_DIRS` (default: the
//! scratchpad corpora); a missing pack skips its test. Images go to `target/sb-renders/`.
#![cfg(feature = "pipeline-tests")]

mod common;

use common::{GPU_LOCK, assert_no_validation_messages, luminance_variance, mean_abs_diff, render_dir, runtime};
use sb_core::model::{BlobTable, CompiledPack, DepthMode, OutputTarget};
use sb_runtime::{RenderOutput, RenderRequest, Runtime, SceneParams};
use std::path::{Path, PathBuf};

const DEFAULT_ROOTS: [&str; 2] = [
    "/tmp/claude-0/-home-user-super-adventure/14a9b258-2170-5e12-9e2c-1c1a40e3de07/scratchpad/corpus",
    "/tmp/claude-0/-home-user-super-adventure/14a9b258-2170-5e12-9e2c-1c1a40e3de07/scratchpad/corpus2",
];

fn corpus_roots() -> Vec<PathBuf> {
    match std::env::var("SB_CORPUS_DIRS") {
        Ok(v) if !v.trim().is_empty() => v.split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect(),
        _ => DEFAULT_ROOTS.iter().map(PathBuf::from).collect(),
    }
}

fn normalize(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).flat_map(char::to_lowercase).collect()
}

/// The directory containing `shaders/` (directly or one level nested).
fn pack_root(dir: &Path) -> Option<PathBuf> {
    if dir.join("shaders").is_dir() {
        return Some(dir.to_path_buf());
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.join("shaders").is_dir())
}

/// Every pack under the corpus roots: (name, pack root).
fn all_packs() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for root in corpus_roots() {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for d in dirs {
            if let Some(r) = pack_root(&d) {
                out.push((d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), r));
            }
        }
    }
    out
}

/// The first pack whose directory name matches `name` (case/punctuation-insensitive,
/// either name containing the other).
fn find_pack(name: &str) -> Option<PathBuf> {
    let want = normalize(name);
    all_packs().into_iter().find(|(n, _)| {
        let n = normalize(n);
        n == want || n.contains(&want) || want.contains(&n)
    }).map(|(_, p)| p)
}

fn compile(rt: &Runtime, dir: &Path, depth_mode: DepthMode) -> (CompiledPack, BlobTable, sb_pack::ShaderPack) {
    let pack = sb_pack::ShaderPack::open(dir).unwrap_or_else(|e| panic!("cannot open {}: {e}", dir.display()));
    let mut settings = sb_pipeline::CompileSettings::default();
    settings.env.depth_mode = depth_mode;
    settings.env.targets = vec![OutputTarget::Vulkan];
    settings.env.device = rt.device_info().caps;
    let out = sb_pipeline::compile_pack(&pack, &settings);
    (out.pack, out.blobs, pack)
}

fn dimension_of(pack: &CompiledPack) -> String {
    for want in ["world0", ""] {
        if pack.dimensions.iter().any(|d| d.folder == want) {
            return want.to_string();
        }
    }
    pack.dimensions.first().map(|d| d.folder.clone()).unwrap_or_default()
}

fn render_compiled(rt: &mut Runtime, pack: &CompiledPack, blobs: &BlobTable, files: &sb_pack::ShaderPack, size: (u32, u32), frames: u32) -> RenderOutput {
    let dimension = dimension_of(pack);
    let textures = |p: &str| files.read_bytes(p);
    rt.render(&RenderRequest {
        pack,
        blobs,
        dimension: &dimension,
        width: size.0,
        height: size.1,
        frames,
        scene: SceneParams::default(),
        depth_mode: pack.info.environment.depth_mode,
        textures: &textures,
        capture_targets: false,
    })
    .unwrap_or_else(|e| panic!("render failed: {e}"))
}

/// Non-degenerate: some variance and not (nearly) all black.
fn assert_non_degenerate(name: &str, img: &image::RgbaImage) {
    let var = luminance_variance(img);
    let black = img.pixels().filter(|p| p[0] < 4 && p[1] < 4 && p[2] < 4).count() as f64 / f64::from(img.width() * img.height());
    eprintln!("{name}: luminance variance {var:.5}, black fraction {black:.3}");
    assert!(var > 1e-4, "{name}: image is (nearly) uniform");
    assert!(black < 0.95, "{name}: image is black (NaN-black or nothing rendered)");
}

fn report(name: &str, out: &RenderOutput) {
    eprintln!(
        "{name}: frames {} passes {} draws {} dispatches {} | skipped programs {} | geometry without program {} | warnings {} | validation errors {} warnings {}",
        out.stats.frames,
        out.stats.passes_run,
        out.stats.draws,
        out.stats.dispatches,
        out.stats.programs_skipped.len(),
        out.stats.geometry_skipped.len(),
        out.stats.warnings.len(),
        out.stats.validation_errors,
        out.stats.validation_warnings
    );
    for s in &out.stats.programs_skipped {
        eprintln!("  skipped {}: {}", s.name, s.reason);
    }
    for w in out.stats.warnings.iter().take(20) {
        eprintln!("  warning: {w}");
    }
}

fn render_named(name: &str) {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(dir) = find_pack(name) else {
        eprintln!("skipping: pack `{name}` not found under {:?}", corpus_roots());
        return;
    };
    let Some(mut rt) = runtime() else { return };
    let (pack, blobs, files) = compile(&rt, &dir, DepthMode::ForwardZeroToOne);
    let out = render_compiled(&mut rt, &pack, &blobs, &files, (640, 360), 3);
    out.image.save(render_dir().join(format!("{}.png", normalize(name)))).ok();
    report(name, &out);
    assert_no_validation_messages(&out);
    assert_non_degenerate(name, &out.image);
}

#[test]
#[ignore = "needs sb-pipeline and the corpus; slow"]
fn render_complementary() {
    render_named("ComplementaryReimagined");
}

#[test]
#[ignore = "needs sb-pipeline and the corpus; slow"]
fn render_photon() {
    render_named("photon");
}

#[test]
#[ignore = "needs sb-pipeline and the corpus; slow"]
fn render_bliss() {
    render_named("Bliss-Shader");
}

/// Forward and reversed Z of the same pack render the same image: the translator's depth
/// remapping (`gl_FragCoord.z`, depth texture reads, `gl_FragDepth`) and the runtime's
/// depth state are exact inverses (up to depth precision and the mean of a few pixels).
fn depth_mode_parity(name: &str) {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(dir) = find_pack(name) else {
        eprintln!("skipping: pack `{name}` not found");
        return;
    };
    let Some(mut rt) = runtime() else { return };
    let (fwd, fb, files) = compile(&rt, &dir, DepthMode::ForwardZeroToOne);
    let (rev, rb, _) = compile(&rt, &dir, DepthMode::ReversedZeroToOne);
    let a = render_compiled(&mut rt, &fwd, &fb, &files, (320, 180), 3);
    let b = render_compiled(&mut rt, &rev, &rb, &files, (320, 180), 3);
    let tag = normalize(name);
    a.image.save(render_dir().join(format!("parity_{tag}_forward.png"))).ok();
    b.image.save(render_dir().join(format!("parity_{tag}_reversed.png"))).ok();
    assert_no_validation_messages(&a);
    assert_no_validation_messages(&b);
    let d = mean_abs_diff(&a.image, &b.image);
    eprintln!("{name}: forward vs reversed mean abs diff {d:.6} ({:.3}/255)", d * 255.0);
    assert!(d < 2.0 / 255.0, "{name}: forward and reversed renders differ: {d}");
}

#[test]
#[ignore = "needs sb-pipeline and the corpus; slow"]
fn depth_mode_parity_complementary() {
    depth_mode_parity("ComplementaryReimagined");
}

#[test]
#[ignore = "needs sb-pipeline and the corpus; slow"]
fn depth_mode_parity_photon() {
    depth_mode_parity("photon");
}

#[test]
#[ignore = "needs sb-pipeline and the corpus; slow"]
fn depth_mode_parity_bliss() {
    depth_mode_parity("Bliss-Shader");
}

/// Every pack of every corpus root, small and one frame: a survey that only fails on
/// panics or device loss and prints a table.
#[test]
#[ignore = "needs sb-pipeline and the corpus; very slow"]
fn render_all_corpus_packs() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    let packs = all_packs();
    let mut rows = Vec::new();
    for (name, dir) in &packs {
        let (pack, blobs, files) = compile(&rt, dir, DepthMode::ForwardZeroToOne);
        let dimension = dimension_of(&pack);
        let textures = |p: &str| files.read_bytes(p);
        let result = rt.render(&RenderRequest {
            pack: &pack,
            blobs: &blobs,
            dimension: &dimension,
            width: 320,
            height: 180,
            frames: 1,
            scene: SceneParams::default(),
            depth_mode: DepthMode::ForwardZeroToOne,
            textures: &textures,
            capture_targets: false,
        });
        match result {
            Ok(out) => {
                out.image.save(render_dir().join(format!("survey_{}.png", normalize(name)))).ok();
                rows.push(format!(
                    "{name:32} ok   programs skipped {:3}  validation errors {:4}  variance {:.4}",
                    out.stats.programs_skipped.len(),
                    out.stats.validation_errors,
                    luminance_variance(&out.image)
                ));
            }
            Err(e @ sb_runtime::RuntimeError::DeviceLost(_)) => panic!("{name}: {e}"),
            Err(e) => rows.push(format!("{name:32} FAIL {e}")),
        }
    }
    for r in &rows {
        eprintln!("{r}");
    }
}
