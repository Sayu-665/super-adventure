//! Helpers shared by the integration tests that run against the real shader
//! pack corpus.
//!
//! The corpus location defaults to the ShaderBridge research scratchpad and can
//! be overridden with `SB_CORPUS_DIR` (a directory containing pack folders,
//! each with one or more `shaders/` directories). Tests skip when it is missing.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sb_core::SourceProvider;
use sb_preprocess::PreprocessOptions;

pub const DEFAULT_CORPUS: &str = "/tmp/claude-0/-home-user-super-adventure/14a9b258-2170-5e12-9e2c-1c1a40e3de07/scratchpad/corpus";

pub const PROGRAM_EXTENSIONS: &[&str] = &["vsh", "fsh", "gsh", "csh", "tcs", "tes"];

pub fn corpus_root() -> Option<PathBuf> {
    let p = std::env::var_os("SB_CORPUS_DIR")
        .map_or_else(|| PathBuf::from(DEFAULT_CORPUS), PathBuf::from);
    if p.is_dir() {
        Some(p)
    } else {
        eprintln!("corpus not found at {}; skipping", p.display());
        None
    }
}

/// Reads pack files from a `shaders/` directory (lossy UTF-8, like a lenient loader).
pub struct DirSources {
    pub root: PathBuf,
}

impl SourceProvider for DirSources {
    fn read(&self, path: &str) -> Option<Arc<str>> {
        let bytes = std::fs::read(self.root.join(path)).ok()?;
        Some(Arc::from(String::from_utf8_lossy(&bytes).as_ref()))
    }
}

/// `(pack name, shaders dir)` for every shader pack in the corpus.
pub fn shader_roots(corpus: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(corpus)
        .max_depth(3)
        .sort_by_file_name()
    {
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_dir() && entry.file_name() == "shaders" {
            let rel = entry.path().strip_prefix(corpus).unwrap_or(entry.path());
            let name = rel
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            out.push((name, entry.path().to_path_buf()));
        }
    }
    out
}

pub fn is_program_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PROGRAM_EXTENSIONS.contains(&e))
}

/// Program files: directly in `shaders/` and in `shaders/world*/` (not lib/include dirs).
pub fn program_files(shaders: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut dirs = vec![(shaders.to_path_buf(), String::new())];
    if let Ok(rd) = std::fs::read_dir(shaders) {
        let mut subs: Vec<_> = rd
            .flatten()
            .filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with("world"))
            .map(|e| (e.path(), format!("{}/", e.file_name().to_string_lossy())))
            .collect();
        subs.sort();
        dirs.extend(subs);
    }
    for (dir, prefix) in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<String> = rd
            .flatten()
            .filter(|e| e.path().is_file() && is_program_file(&e.path()))
            .map(|e| format!("{prefix}{}", e.file_name().to_string_lossy()))
            .collect();
        files.sort();
        out.extend(files);
    }
    out
}

/// Environment defines used for corpus runs (`(name, value)`, empty = flag).
pub const CORPUS_DEFINES: &[(&str, &str)] = &[
    ("MC_VERSION", "260300"),
    ("MC_GL_VERSION", "460"),
    ("MC_GLSL_VERSION", "460"),
    ("IS_IRIS", ""),
    ("DISTANT_HORIZONS", ""),
    ("MC_OS_LINUX", ""),
    ("MC_GL_VENDOR_OTHER", ""),
    ("MC_GL_RENDERER_OTHER", ""),
];

pub fn corpus_options() -> PreprocessOptions {
    let mut o = PreprocessOptions::default();
    for (k, v) in CORPUS_DEFINES {
        o = if v.is_empty() {
            o.with_flag(*k)
        } else {
            o.with_define(*k, *v)
        };
    }
    o
}
