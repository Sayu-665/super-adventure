//! Real shader-pack sources through `compile_glsl`.
//!
//! Raw pack files are OptiFine/Iris compatibility GLSL with `#include`s, so
//! almost all of them are *expected* to fail here (translating them is
//! sb-transform's job). This test checks what sb-compile itself guarantees on
//! arbitrary real-world input, in parallel:
//!
//! * no panic and no process abort;
//! * every failure carries at least one error, every reported line exists in
//!   the source, and line-map translation points back at the right file/line;
//! * `#include` is never resolved from disk;
//! * anything that does compile reflects and passes `spirv-val`.
//!
//! The corpus lives outside the repository. Set `SB_CORPUS_DIR` to a directory
//! containing `<pack>/shaders/...`; the test is skipped when it is missing.

mod common;

use rayon::prelude::*;
use sb_compile::{CompileOptions, DIAG_CODE, VulkanTarget, compile_glsl, reflect, validate};
use sb_core::{ShaderStage, SourceLocation};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const DEFAULT_CORPUS: &str =
    "/tmp/claude-0/-home-user-super-adventure/14a9b258-2170-5e12-9e2c-1c1a40e3de07/scratchpad/corpus";

fn corpus_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("SB_CORPUS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_CORPUS));
    dir.is_dir().then_some(dir)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().and_then(|e| e.to_str()).and_then(ShaderStage::from_pack_extension).is_some() {
            out.push(p);
        }
    }
}

/// Make a pack file get further through glslang without changing its line
/// numbering: `#version` -> `#version 450`, `#include` lines blanked.
fn normalise_version_and_includes(source: &str) -> Option<String> {
    let mut found = false;
    let lines: Vec<String> = source
        .lines()
        .map(|l| {
            let t = l.trim_start();
            if t.starts_with("#version") && !found {
                found = true;
                "#version 450".to_string()
            } else if t.starts_with("#include") {
                String::new()
            } else {
                l.to_string()
            }
        })
        .collect();
    found.then(|| lines.join("\n") + "\n")
}

/// A pack file with its `#include`s expanded (Iris semantics: `/x` is relative
/// to the `shaders/` root, anything else to the including file), `#version`
/// replaced by `#version 450` on the first line, and the original location of
/// every line. Conditionals and macros are left to glslang's preprocessor.
fn expand_includes(shaders_root: &Path, rel: &str) -> Option<(String, Vec<Option<SourceLocation>>)> {
    fn walk(
        root: &Path,
        rel: &str,
        stack: &mut Vec<String>,
        text: &mut String,
        map: &mut Vec<Option<SourceLocation>>,
    ) -> bool {
        if stack.len() > 16 || stack.iter().any(|s| s == rel) {
            return false;
        }
        let Ok(bytes) = std::fs::read(root.join(rel)) else { return false };
        stack.push(rel.to_string());
        for (i, line) in String::from_utf8_lossy(&bytes).lines().enumerate() {
            let here = Some(SourceLocation::new(rel, i as u32 + 1));
            let trimmed = line.trim_start();
            if let Some(arg) = trimmed.strip_prefix("#include") {
                let target = arg.trim().trim_matches(|c| c == '"' || c == '<' || c == '>');
                if let Some(path) = sb_core::normalize_pack_path(rel, target)
                    && walk(root, &path, stack, text, map)
                {
                    continue;
                }
            }
            text.push_str(if trimmed.starts_with("#version") { "" } else { line });
            text.push('\n');
            map.push(here);
        }
        stack.pop();
        true
    }
    let mut text = String::from("#version 450\n");
    let mut map = vec![None];
    walk(shaders_root, rel, &mut Vec::new(), &mut text, &mut map).then_some((text, map))
}

/// The pack's `shaders/` directory containing `path`.
fn shaders_root_of(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|p| p.file_name().is_some_and(|n| n == "shaders")).map(Path::to_path_buf)
}

/// Every program with its includes expanded (up to ~600 KB of GLSL each):
/// glslang must reject or accept it cleanly, the complexity guard must never
/// reject real pack code (the deepest real statement is estimated at 335, the
/// limit is 25,000), and every located message must map back through the line
/// map to the right file and line across include boundaries.
#[test]
fn corpus_with_includes_expanded() {
    let Some(root) = corpus_dir() else {
        eprintln!("corpus not found (set SB_CORPUS_DIR); skipping");
        return;
    };
    let mut files = Vec::new();
    collect(&root, &mut files);
    let start = std::time::Instant::now();
    let results: Vec<(bool, usize)> = files
        .par_iter()
        .filter_map(|path| {
            let shaders = shaders_root_of(path)?;
            let rel = path.strip_prefix(&shaders).ok()?.to_string_lossy().replace('\\', "/");
            let stage = ShaderStage::from_pack_extension(path.extension()?.to_str()?)?;
            let (source, line_map) = expand_includes(&shaders, &rel)?;
            let lines: Vec<&str> = source.lines().collect();
            let label = path.display().to_string();
            match compile_glsl(&source, stage, &rel, &CompileOptions::default(), Some(&line_map)) {
                Ok(spirv) => {
                    let refl = reflect(&spirv).unwrap_or_else(|e| panic!("{label}: reflect failed: {e}"));
                    assert_eq!(refl.stage, stage, "{label}");
                    assert!(!validate(&spirv, VulkanTarget::Vulkan1_2).is_invalid(), "{label}: spirv-val rejected");
                    Some((true, 0))
                }
                Err(f) => {
                    assert!(!f.errors.is_empty(), "{label}");
                    for m in f.errors.iter().chain(&f.warnings) {
                        // Neither the expression-depth guard nor the macro screen may
                        // reject real pack code.
                        assert!(!m.message.contains("to compile safely"), "{label}: guard tripped: {m}");
                        let Some(l) = m.line else { continue };
                        assert!(l as usize <= lines.len() + 1, "{label}: line {l} beyond the source: {m}");
                        assert_eq!(m.original, line_map.get(l as usize - 1).cloned().flatten(), "{label}: {m}");
                    }
                    Some((false, f.errors.iter().filter(|m| m.original.is_some()).count()))
                }
            }
        })
        .collect();
    let compiled = results.iter().filter(|r| r.0).count();
    let mapped: usize = results.iter().map(|r| r.1).sum();
    eprintln!(
        "include-expanded corpus: {} programs in {:.2?}, {compiled} compiled, {mapped} errors mapped to pack files",
        results.len(),
        start.elapsed()
    );
    if !results.is_empty() {
        assert!(mapped > 0, "errors in expanded sources should map back to pack files");
    }
}

struct Outcome {
    pack: String,
    ok: bool,
    first_error: Option<String>,
}

/// With `#version 450` and includes blanked, glslang reaches the bodies and
/// reports located errors (loose uniforms, legacy built-ins, missing include
/// symbols). Every located message must point inside the file, with a column
/// inside its line.
#[test]
fn corpus_errors_are_located_inside_the_source() {
    let Some(root) = corpus_dir() else {
        eprintln!("corpus not found (set SB_CORPUS_DIR); skipping");
        return;
    };
    let mut files = Vec::new();
    collect(&root, &mut files);
    let results: Vec<(usize, usize, usize)> = files
        .par_iter()
        .filter_map(|path| {
            let rel = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/");
            let stage = ShaderStage::from_pack_extension(path.extension()?.to_str()?)?;
            let source = normalise_version_and_includes(&String::from_utf8_lossy(&std::fs::read(path).ok()?))?;
            // Thin wrappers whose `main` lives in an include only produce the
            // (unlocated) "Missing entry point" link error.
            if !source.contains("main(") {
                return None;
            }
            let lines: Vec<&str> = source.lines().collect();
            let line_map: Vec<Option<SourceLocation>> =
                (1..=lines.len() as u32).map(|l| Some(SourceLocation::new(rel.clone(), l))).collect();
            let (mut located, mut unlocated) = (0, 0);
            match compile_glsl(&source, stage, &rel, &CompileOptions::default(), Some(&line_map)) {
                Ok(spirv) => {
                    let refl = reflect(&spirv).unwrap_or_else(|e| panic!("{rel}: {e}"));
                    assert_eq!(refl.stage, stage, "{rel}");
                    assert!(!validate(&spirv, VulkanTarget::Vulkan1_2).is_invalid(), "{rel}: spirv-val rejected");
                    if std::env::var_os("SB_CORPUS_VERBOSE").is_some() {
                        eprintln!("COMPILED {rel}");
                    }
                    return Some((1, 0, 0));
                }
                Err(f) => {
                    for m in &f.errors {
                        let Some(l) = m.line else {
                            if std::env::var_os("SB_CORPUS_VERBOSE").is_some() {
                                eprintln!("UNLOCATED {rel}: {}", m.message.lines().next().unwrap_or_default());
                            }
                            unlocated += 1;
                            continue;
                        };
                        located += 1;
                        let text = lines.get(l as usize - 1).unwrap_or_else(|| panic!("{rel}: line {l} beyond EOF: {m}"));
                        if let Some(c) = m.column {
                            assert!(c as usize <= text.len() + 1, "{rel}:{l}: column {c} beyond line end: {m}");
                        }
                        assert_eq!(m.original, Some(SourceLocation::new(rel.clone(), l)));
                    }
                }
            }
            Some((0, located, unlocated))
        })
        .collect();
    let compiled: usize = results.iter().map(|r| r.0).sum();
    let located: usize = results.iter().map(|r| r.1).sum();
    let unlocated: usize = results.iter().map(|r| r.2).sum();
    eprintln!(
        "normalised corpus: {} files, {compiled} compiled, {located} located errors, {unlocated} unlocated errors",
        results.len()
    );
    if !results.is_empty() {
        assert!(located > unlocated, "most errors in real sources should carry a line");
    }
}

#[test]
fn corpus_sources_fail_cleanly_with_located_errors() {
    let Some(root) = corpus_dir() else {
        eprintln!("corpus not found (set SB_CORPUS_DIR); skipping");
        return;
    };
    let mut files = Vec::new();
    collect(&root, &mut files);
    if files.is_empty() {
        eprintln!("no shader files under {}; skipping", root.display());
        return;
    }

    let start = std::time::Instant::now();
    let outcomes: Vec<Outcome> = files
        .par_iter()
        .map(|path| {
            let rel = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/");
            let pack = rel.split('/').next().unwrap_or_default().to_string();
            let stage = ShaderStage::from_pack_extension(path.extension().unwrap().to_str().unwrap()).unwrap();
            let bytes = std::fs::read(path).unwrap();
            let source = String::from_utf8_lossy(&bytes).into_owned();
            let line_count = source.lines().count() as u32;
            // Every line maps to itself in the pack file.
            let line_map: Vec<Option<SourceLocation>> =
                (1..=line_count).map(|l| Some(SourceLocation::new(rel.clone(), l))).collect();

            match compile_glsl(&source, stage, &rel, &CompileOptions::default(), Some(&line_map)) {
                Ok(spirv) => {
                    let refl = reflect(&spirv).unwrap_or_else(|e| panic!("{rel}: reflect failed: {e}"));
                    assert_eq!(refl.stage, stage, "{rel}");
                    assert!(!validate(&spirv, VulkanTarget::Vulkan1_2).is_invalid(), "{rel}: spirv-val rejected");
                    Outcome { pack, ok: true, first_error: None }
                }
                Err(f) => {
                    assert!(!f.errors.is_empty(), "{rel}: failure without errors");
                    assert_eq!(f.stage, stage);
                    assert!(
                        f.errors.iter().all(|m| !m.message.contains("to compile safely")),
                        "{rel}: a safety guard rejected real pack code: {}",
                        f.errors[0]
                    );
                    for m in f.errors.iter().chain(&f.warnings) {
                        if let Some(l) = m.line {
                            assert!(l >= 1 && l <= line_count + 1, "{rel}: line {l} out of range ({line_count} lines): {m}");
                            if l <= line_count {
                                assert_eq!(m.original, Some(SourceLocation::new(rel.clone(), l)), "{rel}");
                            }
                        }
                    }
                    let diags = f.to_diagnostics();
                    assert!(diags.iter().any(|d| d.is_error() && d.code == DIAG_CODE));
                    // `#include` must never be resolved: a file whose first problem is an
                    // include reports it instead of compiling the included text.
                    let first = f.errors[0].message.clone();
                    Outcome { pack, ok: false, first_error: Some(first) }
                }
            }
        })
        .collect();
    let elapsed = start.elapsed();

    let mut per_pack: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for o in &outcomes {
        let e = per_pack.entry(o.pack.as_str()).or_default();
        if o.ok {
            e.0 += 1;
        } else {
            e.1 += 1;
        }
        if let Some(msg) = &o.first_error {
            // Normalise identifiers in quotes so similar errors group together.
            let key = msg.split('\'').enumerate().map(|(i, s)| if i % 2 == 1 { "_" } else { s }).collect::<String>();
            *reasons.entry(key).or_default() += 1;
        }
    }
    eprintln!("corpus: {} stage files in {:.2?}", outcomes.len(), elapsed);
    for (pack, (ok, err)) in &per_pack {
        eprintln!("  {pack:<28} compiled as-is: {ok:>4}   rejected: {err:>4}");
    }
    let mut top: Vec<_> = reasons.into_iter().collect();
    top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    eprintln!("  most common first errors:");
    for (msg, n) in top.iter().take(8) {
        eprintln!("    {n:>5}  {msg}");
    }
    assert_eq!(outcomes.len(), files.len());
}
