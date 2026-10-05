//! Computes the translator revision (`SB_TRANSLATOR_REVISION`): a blake3 hash of every
//! source tree whose code determines what a compile produces, i.e. `src/**`,
//! `profiles/**`, `Cargo.toml` and `build.rs` of `sb-core`, `sb-pack`, `sb-preprocess`,
//! `sb-expr`, `sb-uniforms`, `sb-transform`, `sb-compile` and `sb-pipeline`, plus the
//! workspace `Cargo.lock` (the exact parser, glslang and reflection versions), the
//! workspace root `Cargo.toml` (enabled dependency features, `[patch]`/`[replace]`
//! sections and profiles, none of which `Cargo.lock` records) and `.cargo/config.toml`
//! (the C/C++ flags glslang is built with).
//!
//! `sb_pipeline::cache_key` includes it, so any change to the translator (even without a
//! version bump) invalidates compiled-pack caches written by an older build. Files are
//! hashed by path relative to `crates/` (with `/` separators) and content, in sorted
//! order, so the revision depends only on what the files contain. A tree that does not
//! exist (a crate built outside this workspace) contributes a marker instead.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Crates whose sources determine the translator's output.
const CRATES: &[&str] = &["sb-core", "sb-pack", "sb-preprocess", "sb-expr", "sb-uniforms", "sb-transform", "sb-compile", "sb-pipeline"];
/// Directories of each crate that are hashed recursively.
const TREES: &[&str] = &["src", "profiles"];
/// Single files of each crate that are hashed.
const FILES: &[&str] = &["Cargo.toml", "build.rs"];
/// Files of the workspace root (relative to it, `/`-separated) that are hashed.
const WORKSPACE_FILES: &[&str] = &["Cargo.lock", "Cargo.toml", ".cargo/config.toml"];

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| ".".into()));
    let crates_dir = manifest_dir.parent().map(Path::to_path_buf).unwrap_or_else(|| manifest_dir.clone());
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sb-translator-revision-v1\0");
    for name in CRATES {
        let dir = crates_dir.join(name);
        for tree in TREES {
            let root = dir.join(tree);
            if root.is_dir() {
                println!("cargo:rerun-if-changed={}", root.display());
                let mut files = Vec::new();
                collect(&root, &mut files);
                files.sort();
                for f in files {
                    hash_file(&mut hasher, &crates_dir, &f);
                }
            } else {
                hash_marker(&mut hasher, &format!("{name}/{tree}: absent"));
            }
        }
        for file in FILES {
            let path = dir.join(file);
            if path.is_file() {
                println!("cargo:rerun-if-changed={}", path.display());
                hash_file(&mut hasher, &crates_dir, &path);
            } else {
                hash_marker(&mut hasher, &format!("{name}/{file}: absent"));
            }
        }
    }
    // Workspace-level files. The lock file pins glsl-lang, glslang, spirq, ... (whose
    // versions change the output as much as our own code does). The root manifest holds
    // what the lock file does not record: the features enabled on those dependencies
    // (e.g. glsl-lang's lexer), `[patch]`/`[replace]` overrides and build profiles. The
    // cargo config sets the flags the bundled glslang is compiled with. Hashing the whole
    // manifest is conservative: an unrelated edit to it also yields a new revision.
    let workspace = crates_dir.parent();
    for file in WORKSPACE_FILES {
        match workspace.map(|w| w.join(file)).filter(|p| p.is_file()) {
            Some(path) => {
                println!("cargo:rerun-if-changed={}", path.display());
                hash_marker(&mut hasher, file);
                hash_bytes(&mut hasher, &fs::read(&path).unwrap_or_default());
            }
            None => hash_marker(&mut hasher, &format!("{file}: absent")),
        }
    }
    let revision = hasher.finalize().to_hex();
    println!("cargo:rustc-env=SB_TRANSLATOR_REVISION={}", &revision[..32]);
}

/// Every regular file below `dir` (hidden files and editor backups excluded), recursively.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.ends_with('~') {
            continue;
        }
        match entry.file_type() {
            Ok(t) if t.is_dir() => collect(&path, out),
            Ok(t) if t.is_file() => out.push(path),
            // Symlinks: follow them only to regular files.
            Ok(_) if path.is_file() => out.push(path),
            _ => {}
        }
    }
}

/// Hash a file's path (relative to `base`, `/`-separated) and contents.
fn hash_file(hasher: &mut blake3::Hasher, base: &Path, path: &Path) {
    let rel = path.strip_prefix(base).unwrap_or(path);
    let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    hash_marker(hasher, &rel.join("/"));
    hash_bytes(hasher, &fs::read(path).unwrap_or_default());
}

fn hash_marker(hasher: &mut blake3::Hasher, s: &str) {
    hash_bytes(hasher, s.as_bytes());
}

/// Length-prefixed, so that file boundaries are unambiguous.
fn hash_bytes(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}
