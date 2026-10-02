//! Virtual file systems over a pack's `shaders/` root: a directory ([`DirVfs`]), a zip
//! archive ([`ZipVfs`]) or memory ([`MemVfs`]).
//!
//! All paths are relative to the `shaders/` root and use `/` separators. A leading
//! `/`, `.` segments, `\` separators and `..` segments are normalized; paths that
//! escape the root are rejected.
//!
//! Every implementation performs a **case-insensitive fallback** when an exact path is
//! missing: packs developed on Windows often `#include "/lib/Common.glsl"` for a file
//! named `common.glsl`. The fallback is recorded and reported through
//! [`Vfs::case_fallbacks`], so callers can warn about it.

use crate::error::PackError;
use sb_core::normalize_pack_path;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek};
use std::ops::Bound;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Largest single file a [`Vfs`] will read (guards against zip bombs / huge files).
pub const MAX_FILE_SIZE: u64 = 512 * 1024 * 1024;

/// Largest buffer pre-allocated from a size declared inside a zip archive.
const MAX_PREALLOCATION: u64 = 16 * 1024 * 1024;

/// A use of the case-insensitive fallback: `requested` was missing, `actual` was used.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct CaseFallback {
    pub requested: String,
    pub actual: String,
}

/// Read-only file system rooted at a pack's `shaders/` directory.
pub trait Vfs: Send + Sync {
    /// Read a file. Returns `None` if it does not exist (after the case-insensitive
    /// fallback) or cannot be read.
    fn read(&self, path: &str) -> Option<Vec<u8>>;

    /// Names of the direct children of directory `dir` (`""` = root), sorted.
    /// Directory names end with `/`.
    fn list_dir(&self, dir: &str) -> Vec<String>;

    /// Every file in the file system (paths relative to the root), sorted.
    fn all_files(&self) -> Vec<String>;

    /// Whether `path` is a readable file.
    fn exists(&self, path: &str) -> bool {
        self.read(path).is_some()
    }

    /// Whether `path` is a directory. The default implementation (and [`MemVfs`])
    /// only knows directories that contain at least one file; [`DirVfs`] and [`ZipVfs`]
    /// also report empty directories.
    fn is_dir(&self, path: &str) -> bool {
        !self.list_dir(path).is_empty()
    }

    /// Case-insensitive fallbacks used so far (deduplicated, sorted).
    fn case_fallbacks(&self) -> Vec<CaseFallback> {
        Vec::new()
    }

    /// Non-fatal problems encountered so far (I/O errors, unreadable zip entries, ...).
    fn warnings(&self) -> Vec<String> {
        Vec::new()
    }

    /// Short human-readable description (`dir:/path/to/shaders`, `zip:pack.zip!shaders/`).
    fn describe(&self) -> String;
}

/// Normalize a VFS path. Returns `Some("")` for the root and `None` for paths that
/// escape the root.
pub fn clean_path(path: &str) -> Option<String> {
    let trimmed = path.trim_matches(|c| c == '/' || c == '\\');
    if trimmed.is_empty() || trimmed == "." {
        return Some(String::new());
    }
    if trimmed.contains('\0') {
        return None;
    }
    normalize_pack_path("", &format!("/{trimmed}")).or_else(|| {
        // `normalize_pack_path` returns None both for escapes and for "everything
        // popped to the root" (e.g. `a/..`); distinguish the two.
        let mut depth: i64 = 0;
        for seg in trimmed.replace('\\', "/").split('/') {
            match seg {
                "" | "." => {}
                ".." => {
                    depth -= 1;
                    if depth < 0 {
                        return None;
                    }
                }
                _ => depth += 1,
            }
        }
        Some(String::new())
    })
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Lower-case path -> actual path index used for the case-insensitive fallback, plus
/// the exact spellings of every file and directory.
#[derive(Debug, Default)]
struct CaseIndex {
    files: HashMap<String, String>,
    dirs: HashMap<String, String>,
    exact_files: HashSet<String>,
    exact_dirs: HashSet<String>,
}

impl CaseIndex {
    fn build<'a>(files: impl IntoIterator<Item = &'a str>) -> Self {
        let mut index = CaseIndex::default();
        for f in files {
            index
                .files
                .entry(f.to_lowercase())
                .or_insert_with(|| f.to_string());
            index.exact_files.insert(f.to_string());
            let mut dir = f;
            while let Some((parent, _)) = dir.rsplit_once('/') {
                if !index.exact_dirs.insert(parent.to_string()) {
                    // Every ancestor of `parent` is already indexed too.
                    break;
                }
                index
                    .dirs
                    .entry(parent.to_lowercase())
                    .or_insert_with(|| parent.to_string());
                dir = parent;
            }
        }
        index
    }

    fn add_dir(&mut self, dir: &str) {
        self.exact_dirs.insert(dir.to_string());
        self.dirs
            .entry(dir.to_lowercase())
            .or_insert_with(|| dir.to_string());
    }

    /// The differently-cased spelling of `rel` that exists, if `rel` itself does not.
    fn mismatch<'a>(&'a self, rel: &str, dir: bool) -> Option<&'a str> {
        let (exact, lower) = if dir {
            (&self.exact_dirs, &self.dirs)
        } else {
            (&self.exact_files, &self.files)
        };
        if exact.contains(rel) {
            return None;
        }
        lower
            .get(&rel.to_lowercase())
            .map(String::as_str)
            .filter(|actual| *actual != rel)
    }
}

/// Join a cleaned relative path (see [`clean_path`]) onto `root`, refusing segments
/// that are not plain file names on this platform. On Windows a segment such as `C:`
/// or `\\?\` would otherwise make [`Path::join`] replace the root entirely.
fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    for seg in rel.split('/').filter(|s| !s.is_empty()) {
        let mut components = Path::new(seg).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) => out.push(name),
            _ => return None,
        }
    }
    Some(out)
}

/// Shared bookkeeping for fallbacks and warnings.
#[derive(Debug, Default)]
struct Journal {
    fallbacks: Mutex<BTreeSet<CaseFallback>>,
    warnings: Mutex<Vec<String>>,
}

impl Journal {
    fn fallback(&self, requested: &str, actual: &str) {
        lock(&self.fallbacks).insert(CaseFallback {
            requested: requested.to_string(),
            actual: actual.to_string(),
        });
    }
    fn warn(&self, message: String) {
        let mut w = lock(&self.warnings);
        if !w.contains(&message) {
            w.push(message);
        }
    }
    fn fallbacks(&self) -> Vec<CaseFallback> {
        lock(&self.fallbacks).iter().cloned().collect()
    }
    fn warnings(&self) -> Vec<String> {
        lock(&self.warnings).clone()
    }
}

/// Direct children of `dir` among a sorted set of file paths (dirs get a `/` suffix).
fn children_of<'a>(
    files: impl IntoIterator<Item = &'a str>,
    extra_dirs: impl IntoIterator<Item = &'a str>,
    dir: &str,
) -> Vec<String> {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let mut out = BTreeSet::new();
    let mut take = |p: &str, is_dir: bool| {
        if let Some(rest) = p.strip_prefix(&prefix) {
            if rest.is_empty() {
                return;
            }
            match rest.split_once('/') {
                Some((first, _)) => {
                    out.insert(format!("{first}/"));
                }
                None if is_dir => {
                    out.insert(format!("{rest}/"));
                }
                None => {
                    out.insert(rest.to_string());
                }
            }
        }
    };
    for f in files {
        take(f, false);
    }
    for d in extra_dirs {
        take(d, true);
    }
    out.into_iter().collect()
}

// ---------------------------------------------------------------------------------
// Directory
// ---------------------------------------------------------------------------------

/// A pack's `shaders/` directory on disk.
#[derive(Debug)]
pub struct DirVfs {
    root: PathBuf,
    index: OnceLock<CaseIndex>,
    journal: Journal,
}

impl DirVfs {
    /// Use `root` as the `shaders/` root directly.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            index: OnceLock::new(),
            journal: Journal::default(),
        }
    }

    /// Open a pack directory: uses `<dir>/shaders` if it exists, else `dir` itself if
    /// it looks like a shaders root (see [`DirVfs::looks_like_shaders_root`]).
    pub fn open_pack(dir: &Path) -> Result<Self, PackError> {
        match Self::locate_shaders_root(dir) {
            Some(root) => Ok(Self::new(root)),
            None => {
                if !dir.is_dir() {
                    return Err(PackError::Io {
                        path: dir.to_path_buf(),
                        source: std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            "not a directory",
                        ),
                    });
                }
                Err(PackError::NotAShaderPack {
                    path: dir.to_path_buf(),
                })
            }
        }
    }

    /// Find the shaders root of a pack directory.
    pub fn locate_shaders_root(dir: &Path) -> Option<PathBuf> {
        let nested = dir.join("shaders");
        if nested.is_dir() {
            return Some(nested);
        }
        if dir.is_dir() && Self::looks_like_shaders_root(dir) {
            return Some(dir.to_path_buf());
        }
        None
    }

    /// A directory looks like a shaders root if it directly contains a program file
    /// (`.vsh`, `.fsh`, `.gsh`, `.csh`, `.tcs`, `.tes`) or `shaders.properties`.
    pub fn looks_like_shaders_root(dir: &Path) -> bool {
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        entries.flatten().any(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name == "shaders.properties"
                || name.rsplit_once('.').is_some_and(|(_, ext)| {
                    sb_core::ShaderStage::from_pack_extension(ext).is_some()
                })
        })
    }

    /// The `shaders/` root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn index(&self) -> &CaseIndex {
        self.index.get_or_init(|| {
            let files = self.all_files();
            CaseIndex::build(files.iter().map(String::as_str))
        })
    }

    /// Record a case mismatch for a path the OS resolved although its spelling differs
    /// from the file on disk (case-insensitive file systems: Windows, macOS). The pack
    /// would fail on case-sensitive systems, so this is reported exactly like the
    /// explicit fallback on Linux.
    fn note_case(&self, rel: &str, dir: bool) {
        if let Some(actual) = self.index().mismatch(rel, dir) {
            self.journal.fallback(rel, actual);
        }
    }

    fn read_exact(&self, rel: &str) -> Result<Option<Vec<u8>>, ()> {
        let Some(full) = safe_join(&self.root, rel) else {
            return Ok(None);
        };
        match fs::metadata(&full) {
            Ok(m) if m.is_file() => {
                if m.len() > MAX_FILE_SIZE {
                    self.journal.warn(format!(
                        "{rel}: file larger than {MAX_FILE_SIZE} bytes, not read"
                    ));
                    return Err(());
                }
                match fs::read(&full) {
                    Ok(b) => Ok(Some(b)),
                    Err(e) => {
                        self.journal.warn(format!("{rel}: {e}"));
                        Err(())
                    }
                }
            }
            _ => Ok(None),
        }
    }
}

impl Vfs for DirVfs {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let rel = clean_path(path)?;
        if rel.is_empty() {
            return None;
        }
        match self.read_exact(&rel) {
            Ok(Some(b)) => {
                self.note_case(&rel, false);
                return Some(b);
            }
            Err(()) => return None,
            Ok(None) => {}
        }
        let actual = self.index().files.get(&rel.to_lowercase())?.clone();
        if actual == rel {
            return None;
        }
        let bytes = self.read_exact(&actual).ok()??;
        self.journal.fallback(&rel, &actual);
        Some(bytes)
    }

    fn list_dir(&self, dir: &str) -> Vec<String> {
        let Some(mut rel) = clean_path(dir) else {
            return Vec::new();
        };
        if safe_join(&self.root, &rel).is_some_and(|p| p.is_dir()) {
            if !rel.is_empty() {
                self.note_case(&rel, true);
            }
        } else {
            match self.index().dirs.get(&rel.to_lowercase()) {
                Some(actual) => {
                    self.journal.fallback(&rel, actual);
                    rel = actual.clone();
                }
                None => return Vec::new(),
            }
        }
        let Some(full) = safe_join(&self.root, &rel) else {
            return Vec::new();
        };
        let Ok(entries) = fs::read_dir(&full) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for e in entries.flatten() {
            let Ok(name) = e.file_name().into_string() else {
                self.journal
                    .warn(format!("{rel}: skipping entry with a non-UTF-8 name"));
                continue;
            };
            // Follow symlinks when classifying.
            let is_dir = fs::metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false);
            out.push(if is_dir { format!("{name}/") } else { name });
        }
        out.sort();
        out
    }

    fn all_files(&self) -> Vec<String> {
        let mut out = Vec::new();
        let walker = walkdir::WalkDir::new(&self.root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| {
                // Skip hidden directories such as `.git` (never part of a pack's sources).
                e.depth() == 0
                    || !(e.file_type().is_dir() && e.file_name().to_string_lossy().starts_with('.'))
            });
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    self.journal.warn(format!("directory walk: {e}"));
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&self.root) else {
                continue;
            };
            let mut parts = Vec::new();
            let mut ok = true;
            for c in rel.components() {
                match c.as_os_str().to_str() {
                    Some(s) => parts.push(s),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && !parts.is_empty() {
                out.push(parts.join("/"));
            } else if !ok {
                self.journal.warn(format!(
                    "skipping file with a non-UTF-8 path: {}",
                    rel.display()
                ));
            }
        }
        out.sort();
        out
    }

    fn exists(&self, path: &str) -> bool {
        let Some(rel) = clean_path(path) else {
            return false;
        };
        if rel.is_empty() {
            return false;
        }
        if safe_join(&self.root, &rel).is_some_and(|p| p.is_file()) {
            return true;
        }
        self.index().files.contains_key(&rel.to_lowercase())
    }

    fn is_dir(&self, path: &str) -> bool {
        let Some(rel) = clean_path(path) else {
            return false;
        };
        safe_join(&self.root, &rel).is_some_and(|p| p.is_dir())
            || self.index().dirs.contains_key(&rel.to_lowercase())
    }

    fn case_fallbacks(&self) -> Vec<CaseFallback> {
        self.journal.fallbacks()
    }

    fn warnings(&self) -> Vec<String> {
        self.journal.warnings()
    }

    fn describe(&self) -> String {
        format!("dir:{}", self.root.display())
    }
}

// ---------------------------------------------------------------------------------
// Zip
// ---------------------------------------------------------------------------------

/// Object-safe `Read + Seek + Send`.
pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// A zipped shader pack. The `shaders/` root is either at the archive root or exactly
/// one level nested (`Name/shaders/`).
pub struct ZipVfs {
    label: String,
    archive: Mutex<zip::ZipArchive<Box<dyn ReadSeek>>>,
    /// Archive path prefix of the shaders root, e.g. `shaders/` or `Pack-1.0/shaders/`.
    prefix: String,
    /// Relative file path -> archive entry index.
    files: BTreeMap<String, usize>,
    /// Explicit directory entries (relative, no trailing slash).
    dirs: BTreeSet<String>,
    index: CaseIndex,
    journal: Journal,
}

impl std::fmt::Debug for ZipVfs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZipVfs")
            .field("label", &self.label)
            .field("prefix", &self.prefix)
            .field("files", &self.files.len())
            .finish()
    }
}

impl ZipVfs {
    /// Open a zip file from disk.
    pub fn open(path: &Path) -> Result<Self, PackError> {
        let file = fs::File::open(path).map_err(|source| PackError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_reader(Box::new(std::io::BufReader::new(file)), path)
    }

    /// Open a zip archive held in memory.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, PackError> {
        Self::from_reader(Box::new(std::io::Cursor::new(bytes)), Path::new("<memory>"))
    }

    /// Open a zip archive from any seekable reader; `label` is used in messages.
    pub fn from_reader(reader: Box<dyn ReadSeek>, label: &Path) -> Result<Self, PackError> {
        let archive = zip::ZipArchive::new(reader).map_err(|e| PackError::Zip {
            path: label.to_path_buf(),
            message: e.to_string(),
        })?;
        let mut names: Vec<(String, usize, bool)> = Vec::with_capacity(archive.len());
        for i in 0..archive.len() {
            let Some(name) = archive.name_for_index(i) else {
                continue;
            };
            let is_dir = name.ends_with('/') || name.ends_with('\\');
            let Some(clean) = clean_path(name) else {
                continue;
            };
            if clean.is_empty() {
                continue;
            }
            names.push((clean, i, is_dir));
        }
        let prefix = Self::locate_root(&names).ok_or_else(|| PackError::NotAShaderPack {
            path: label.to_path_buf(),
        })?;
        let mut files = BTreeMap::new();
        let mut dirs = BTreeSet::new();
        for (name, i, is_dir) in &names {
            let Some(rel) = name.strip_prefix(&prefix) else {
                continue;
            };
            if rel.is_empty() {
                continue;
            }
            if *is_dir {
                dirs.insert(rel.to_string());
            } else {
                files.entry(rel.to_string()).or_insert(*i);
            }
        }
        let mut index = CaseIndex::build(files.keys().map(String::as_str));
        for d in &dirs {
            index.add_dir(d);
        }
        Ok(Self {
            label: label.display().to_string(),
            archive: Mutex::new(archive),
            prefix,
            files,
            dirs,
            index,
            journal: Journal::default(),
        })
    }

    /// Locate the shaders root: `shaders/` at the archive root, else a single
    /// `<Name>/shaders/` (the first in sorted order if several exist).
    fn locate_root(names: &[(String, usize, bool)]) -> Option<String> {
        if names
            .iter()
            .any(|(n, _, _)| n == "shaders" || n.starts_with("shaders/"))
        {
            return Some("shaders/".to_string());
        }
        let mut nested = BTreeSet::new();
        for (n, _, _) in names {
            let mut parts = n.splitn(3, '/');
            if let (Some(first), Some("shaders")) = (parts.next(), parts.next())
                && first != "__MACOSX"
            {
                // `__MACOSX/` holds macOS resource-fork copies, never the real pack.
                nested.insert(format!("{first}/shaders/"));
            }
        }
        nested.into_iter().next()
    }

    /// Archive path prefix of the shaders root (`shaders/` or `<Name>/shaders/`).
    pub fn root_prefix(&self) -> &str {
        &self.prefix
    }

    fn read_entry(&self, rel: &str, index: usize) -> Option<Vec<u8>> {
        let mut archive = lock(&self.archive);
        let mut file = match archive.by_index(index) {
            Ok(f) => f,
            Err(e) => {
                self.journal.warn(format!("{rel}: {e}"));
                return None;
            }
        };
        if file.size() > MAX_FILE_SIZE {
            self.journal.warn(format!(
                "{rel}: entry larger than {MAX_FILE_SIZE} bytes, not read"
            ));
            return None;
        }
        // The declared size comes from the archive and may be forged: only use it as a
        // bounded capacity hint.
        let hint = file.size().min(MAX_PREALLOCATION);
        let mut buf = Vec::with_capacity(usize::try_from(hint).unwrap_or(0));
        match file.by_ref().take(MAX_FILE_SIZE + 1).read_to_end(&mut buf) {
            Ok(_) if buf.len() as u64 <= MAX_FILE_SIZE => Some(buf),
            Ok(_) => {
                self.journal.warn(format!(
                    "{rel}: entry larger than {MAX_FILE_SIZE} bytes, not read"
                ));
                None
            }
            Err(e) => {
                self.journal.warn(format!("{rel}: {e}"));
                None
            }
        }
    }

    fn resolve_dir(&self, rel: &str) -> Option<String> {
        if rel.is_empty() || self.index.exact_dirs.contains(rel) {
            return Some(rel.to_string());
        }
        let actual = self.index.dirs.get(&rel.to_lowercase())?;
        self.journal.fallback(rel, actual);
        Some(actual.clone())
    }
}

impl Vfs for ZipVfs {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let rel = clean_path(path)?;
        if let Some(&i) = self.files.get(&rel) {
            return self.read_entry(&rel, i);
        }
        let actual = self.index.files.get(&rel.to_lowercase())?;
        let &i = self.files.get(actual)?;
        let bytes = self.read_entry(actual, i)?;
        self.journal.fallback(&rel, actual);
        Some(bytes)
    }

    fn list_dir(&self, dir: &str) -> Vec<String> {
        let Some(rel) = clean_path(dir) else {
            return Vec::new();
        };
        let Some(actual) = self.resolve_dir(&rel) else {
            return Vec::new();
        };
        children_of(
            self.files.keys().map(String::as_str),
            self.dirs.iter().map(String::as_str),
            &actual,
        )
    }

    fn all_files(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }

    fn exists(&self, path: &str) -> bool {
        clean_path(path).is_some_and(|rel| {
            self.files.contains_key(&rel) || self.index.files.contains_key(&rel.to_lowercase())
        })
    }

    fn is_dir(&self, path: &str) -> bool {
        clean_path(path).is_some_and(|rel| {
            rel.is_empty()
                || self.index.exact_dirs.contains(&rel)
                || self.index.dirs.contains_key(&rel.to_lowercase())
        })
    }

    fn case_fallbacks(&self) -> Vec<CaseFallback> {
        self.journal.fallbacks()
    }

    fn warnings(&self) -> Vec<String> {
        self.journal.warnings()
    }

    fn describe(&self) -> String {
        format!("zip:{}!{}", self.label, self.prefix)
    }
}

// ---------------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------------

/// An in-memory file system (tests, synthesized packs).
#[derive(Debug, Default)]
pub struct MemVfs {
    files: BTreeMap<String, Vec<u8>>,
    journal: Journal,
}

impl MemVfs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style [`MemVfs::insert`].
    pub fn with(mut self, path: &str, contents: impl Into<Vec<u8>>) -> Self {
        self.insert(path, contents);
        self
    }

    /// Add or replace a file. Paths that escape the root are ignored.
    pub fn insert(&mut self, path: &str, contents: impl Into<Vec<u8>>) {
        if let Some(rel) = clean_path(path).filter(|p| !p.is_empty()) {
            self.files.insert(rel, contents.into());
        }
    }

    /// Remove a file, returning its contents.
    pub fn remove(&mut self, path: &str) -> Option<Vec<u8>> {
        self.files.remove(&clean_path(path)?)
    }

    fn find_ci(&self, rel: &str) -> Option<&str> {
        let lower = rel.to_lowercase();
        self.files
            .keys()
            .find(|k| k.to_lowercase() == lower)
            .map(String::as_str)
    }

    fn find_dir_ci(&self, rel: &str) -> Option<String> {
        // Compare segment-wise: lower-casing may change byte lengths.
        let wanted: Vec<String> = rel.split('/').map(str::to_lowercase).collect();
        self.files.keys().find_map(|k| {
            let segs: Vec<&str> = k.split('/').collect();
            (segs.len() > wanted.len()
                && segs
                    .iter()
                    .zip(&wanted)
                    .all(|(s, w)| s.to_lowercase() == *w))
            .then(|| segs[..wanted.len()].join("/"))
        })
    }
}

impl Vfs for MemVfs {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let rel = clean_path(path)?;
        if let Some(b) = self.files.get(&rel) {
            return Some(b.clone());
        }
        let actual = self.find_ci(&rel)?;
        self.journal.fallback(&rel, actual);
        self.files.get(actual).cloned()
    }

    fn list_dir(&self, dir: &str) -> Vec<String> {
        let Some(rel) = clean_path(dir) else {
            return Vec::new();
        };
        let prefix = format!("{rel}/");
        let has_children = self
            .files
            .range::<str, _>((Bound::Included(prefix.as_str()), Bound::Unbounded))
            .next()
            .is_some_and(|(k, _)| k.starts_with(&prefix));
        let actual = if rel.is_empty() || has_children {
            rel
        } else {
            match self.find_dir_ci(&rel) {
                Some(a) => {
                    self.journal.fallback(&rel, &a);
                    a
                }
                None => return Vec::new(),
            }
        };
        children_of(
            self.files.keys().map(String::as_str),
            std::iter::empty(),
            &actual,
        )
    }

    fn all_files(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }

    fn exists(&self, path: &str) -> bool {
        clean_path(path)
            .is_some_and(|rel| self.files.contains_key(&rel) || self.find_ci(&rel).is_some())
    }

    fn case_fallbacks(&self) -> Vec<CaseFallback> {
        self.journal.fallbacks()
    }

    fn describe(&self) -> String {
        format!("mem:{} files", self.files.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_path_normalizes() {
        assert_eq!(clean_path("/lib/a.glsl").as_deref(), Some("lib/a.glsl"));
        assert_eq!(clean_path("lib\\a.glsl").as_deref(), Some("lib/a.glsl"));
        assert_eq!(
            clean_path("./lib/./x/../a.glsl").as_deref(),
            Some("lib/a.glsl")
        );
        assert_eq!(clean_path("").as_deref(), Some(""));
        assert_eq!(clean_path("/").as_deref(), Some(""));
        assert_eq!(clean_path("a/..").as_deref(), Some(""));
        assert_eq!(clean_path("../x"), None);
        assert_eq!(clean_path("a/../../x"), None);
        assert_eq!(clean_path("world0/"), Some("world0".to_string()));
    }

    #[test]
    fn case_index_reports_only_real_mismatches() {
        let mut index = CaseIndex::build(["lib/common.glsl", "A.glsl", "a.glsl", "x/y/z.glsl"]);
        index.add_dir("empty");
        assert_eq!(
            index.mismatch("lib/Common.glsl", false),
            Some("lib/common.glsl")
        );
        assert_eq!(index.mismatch("lib/common.glsl", false), None);
        // Two files that differ only in case are both exact.
        assert_eq!(index.mismatch("a.glsl", false), None);
        assert_eq!(index.mismatch("A.glsl", false), None);
        assert_eq!(index.mismatch("LIB", true), Some("lib"));
        assert_eq!(index.mismatch("X/Y", true), Some("x/y"));
        assert_eq!(index.mismatch("x/y", true), None);
        assert_eq!(index.mismatch("Empty", true), Some("empty"));
        assert_eq!(index.mismatch("missing.glsl", false), None);
    }

    #[test]
    fn safe_join_keeps_paths_inside_the_root() {
        let root = Path::new("root");
        assert_eq!(
            safe_join(root, "lib/a.glsl"),
            Some(root.join("lib").join("a.glsl"))
        );
        assert_eq!(safe_join(root, ""), Some(root.to_path_buf()));
        assert_eq!(safe_join(root, ".."), None);
        if cfg!(windows) {
            // Drive prefixes would make `Path::join` discard the root.
            assert_eq!(safe_join(root, "C:/Windows/win.ini"), None);
            assert_eq!(safe_join(root, "lib/C:x"), None);
        } else {
            assert_eq!(safe_join(root, "C:"), Some(root.join("C:")));
        }
    }

    #[test]
    fn zip_and_mem_directory_lookups() {
        let vfs = MemVfs::new()
            .with("lib/a.glsl", "x")
            .with("lib2/b.glsl", "y")
            .with("libz.glsl", "z");
        assert_eq!(vfs.list_dir("lib"), vec!["a.glsl"]);
        assert_eq!(vfs.list_dir("lib2"), vec!["b.glsl"]);
        assert!(vfs.list_dir("li").is_empty(), "a prefix is not a directory");
        assert!(vfs.list_dir("libz.glsl").is_empty());
    }

    #[test]
    fn children_listing() {
        let files = [
            "a.fsh",
            "lib/x.glsl",
            "lib/y/z.glsl",
            "world0/composite.fsh",
        ];
        assert_eq!(children_of(files, [], ""), vec!["a.fsh", "lib/", "world0/"]);
        assert_eq!(children_of(files, [], "lib"), vec!["x.glsl", "y/"]);
        assert_eq!(
            children_of(files, ["empty"], ""),
            vec!["a.fsh", "empty/", "lib/", "world0/"]
        );
        assert!(children_of(files, [], "nope").is_empty());
    }

    #[test]
    fn mem_vfs_basics_and_case_fallback() {
        let vfs = MemVfs::new()
            .with("lib/common.glsl", "x")
            .with("/composite.fsh", "y");
        assert_eq!(vfs.read("lib/common.glsl").as_deref(), Some(&b"x"[..]));
        assert!(vfs.case_fallbacks().is_empty());
        assert_eq!(vfs.read("/lib/Common.glsl").as_deref(), Some(&b"x"[..]));
        assert_eq!(
            vfs.case_fallbacks(),
            vec![CaseFallback {
                requested: "lib/Common.glsl".into(),
                actual: "lib/common.glsl".into()
            }]
        );
        assert_eq!(vfs.list_dir(""), vec!["composite.fsh", "lib/"]);
        assert_eq!(vfs.list_dir("LIB"), vec!["common.glsl"]);
        assert!(vfs.is_dir("lib"));
        assert!(!vfs.is_dir("composite.fsh"));
        assert!(vfs.exists("COMPOSITE.FSH"));
        assert!(vfs.read("../etc/passwd").is_none());
        assert_eq!(vfs.all_files(), vec!["composite.fsh", "lib/common.glsl"]);
    }

    #[test]
    fn case_fallback_with_length_changing_lowercase() {
        // 'İ' (2 bytes) lower-cases to "i̇" (3 bytes).
        let vfs = MemVfs::new().with("\u{130}dir/a.glsl", "x");
        assert_eq!(vfs.list_dir("\u{130}DIR"), vec!["a.glsl"]);
        assert!(vfs.list_dir("i\u{307}dir/a.glsl/deeper").is_empty());
        assert!(
            vfs.list_dir("\u{130}dir/a.glsl").is_empty(),
            "a file is not a directory"
        );
    }

    #[test]
    fn dir_vfs_reads_lists_and_falls_back() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("shaders");
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::create_dir_all(root.join("world0")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/HEAD"), "x").unwrap();
        fs::write(root.join("composite.fsh"), "void main(){}").unwrap();
        fs::write(root.join("lib/common.glsl"), "// common").unwrap();
        fs::write(root.join("world0/final.fsh"), "f").unwrap();

        let vfs = DirVfs::open_pack(tmp.path()).unwrap();
        assert_eq!(vfs.root(), root.as_path());
        assert_eq!(vfs.read("composite.fsh").unwrap(), b"void main(){}");
        assert_eq!(
            vfs.list_dir(""),
            vec![".git/", "composite.fsh", "lib/", "world0/"]
        );
        assert_eq!(vfs.list_dir("world0"), vec!["final.fsh"]);
        assert_eq!(
            vfs.all_files(),
            vec!["composite.fsh", "lib/common.glsl", "world0/final.fsh"]
        );
        assert!(vfs.read("lib").is_none(), "directories are not files");
        assert!(vfs.read("../outside").is_none());
        assert_eq!(vfs.read("lib/Common.glsl").unwrap(), b"// common");
        assert_eq!(vfs.case_fallbacks().len(), 1);
        assert_eq!(vfs.list_dir("World0"), vec!["final.fsh"]);
        assert!(vfs.exists("LIB/COMMON.GLSL"));
        assert!(vfs.is_dir("lib"));

        // The shaders directory itself is accepted as a root.
        let direct = DirVfs::open_pack(&root).unwrap();
        assert_eq!(direct.root(), root.as_path());
        // A directory without shaders is rejected.
        let empty = tempfile::tempdir().unwrap();
        assert!(matches!(
            DirVfs::open_pack(empty.path()),
            Err(PackError::NotAShaderPack { .. })
        ));
    }
}
