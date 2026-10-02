//! Access to pack source files, shared by `sb-pack` (which implements it, applying
//! option edits) and `sb-preprocess` (which consumes it).

use std::sync::Arc;

/// Read-only access to text files of a shader pack, addressed by normalized paths
/// relative to the `shaders/` root (see [`normalize_pack_path`]).
pub trait SourceProvider: Send + Sync {
    /// Return the file's text, or `None` if it does not exist.
    fn read(&self, path: &str) -> Option<Arc<str>>;
}

impl<T: SourceProvider + ?Sized> SourceProvider for &T {
    fn read(&self, path: &str) -> Option<Arc<str>> {
        (**self).read(path)
    }
}

impl<T: SourceProvider + ?Sized> SourceProvider for Arc<T> {
    fn read(&self, path: &str) -> Option<Arc<str>> {
        (**self).read(path)
    }
}

/// In-memory provider (tests, synthesized sources).
#[derive(Debug, Default, Clone)]
pub struct MemorySources {
    pub files: std::collections::BTreeMap<String, Arc<str>>,
}

impl MemorySources {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with(mut self, path: &str, text: &str) -> Self {
        self.insert(path, text);
        self
    }
    pub fn insert(&mut self, path: &str, text: &str) {
        let p = normalize_pack_path("", path).unwrap_or_else(|| path.to_string());
        self.files.insert(p, Arc::from(text));
    }
}

impl SourceProvider for MemorySources {
    fn read(&self, path: &str) -> Option<Arc<str>> {
        self.files.get(path).cloned()
    }
}

/// Resolve `target` (an `#include` argument or a file name) against the directory of
/// `base_file` and normalize it to a pack path relative to the `shaders/` root:
///
/// * a leading `/` means "relative to the shaders root";
/// * otherwise the path is relative to `base_file`'s directory (`base_file` may be `""`);
/// * `\` is treated as `/`, `.` segments are removed and `..` pops a segment;
/// * returns `None` if `..` escapes the root or the result is empty.
///
/// ```
/// use sb_core::source::normalize_pack_path;
/// assert_eq!(normalize_pack_path("world0/composite.fsh", "/lib/a.glsl").as_deref(), Some("lib/a.glsl"));
/// assert_eq!(normalize_pack_path("lib/x/b.glsl", "../c.glsl").as_deref(), Some("lib/c.glsl"));
/// assert_eq!(normalize_pack_path("a.fsh", "../../x").as_deref(), None);
/// ```
pub fn normalize_pack_path(base_file: &str, target: &str) -> Option<String> {
    let target = target.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    let base_owned;
    if !target.starts_with('/') {
        base_owned = base_file.replace('\\', "/");
        let dir = match base_owned.rfind('/') {
            Some(i) => &base_owned[..i],
            None => "",
        };
        for seg in dir.split('/') {
            if !seg.is_empty() && seg != "." {
                parts.push(seg);
            }
        }
    }
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// Like [`normalize_pack_path`], but `..` segments that would climb above the shaders
/// root are ignored instead of failing. This matches Iris's `AbsolutePackPath`, which
/// packs rely on (e.g. `#include "../../lib/x.glsl"` from a shallow file still resolves).
/// Returns `None` only if the result is empty.
pub fn normalize_pack_path_clamped(base_file: &str, target: &str) -> Option<String> {
    let target = target.replace('\\', "/");
    let mut parts: Vec<String> = Vec::new();
    if !target.starts_with('/') {
        let base = base_file.replace('\\', "/");
        if let Some(i) = base.rfind('/') {
            for seg in base[..i].split('/') {
                if !seg.is_empty() && seg != "." {
                    parts.push(seg.to_string());
                }
            }
        }
    }
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s.to_string()),
        }
    }
    if parts.is_empty() { None } else { Some(parts.join("/")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize() {
        assert_eq!(normalize_pack_path("", "composite.fsh").as_deref(), Some("composite.fsh"));
        assert_eq!(normalize_pack_path("world0/composite.fsh", "lib/a.glsl").as_deref(), Some("world0/lib/a.glsl"));
        assert_eq!(normalize_pack_path("world0/composite.fsh", "/lib/a.glsl").as_deref(), Some("lib/a.glsl"));
        assert_eq!(normalize_pack_path("world0/composite.fsh", "../lib/a.glsl").as_deref(), Some("lib/a.glsl"));
        assert_eq!(normalize_pack_path("a/b/c.glsl", "./d/../e.glsl").as_deref(), Some("a/b/e.glsl"));
        assert_eq!(normalize_pack_path("a\\b.glsl", "c\\d.glsl").as_deref(), Some("a/c/d.glsl"));
        assert_eq!(normalize_pack_path("x.fsh", ".."), None);
    }

    #[test]
    fn normalize_clamped() {
        assert_eq!(normalize_pack_path_clamped("a.fsh", "../../lib/x.glsl").as_deref(), Some("lib/x.glsl"));
        assert_eq!(normalize_pack_path_clamped("world0/a.fsh", "../lib/x.glsl").as_deref(), Some("lib/x.glsl"));
        assert_eq!(normalize_pack_path_clamped("a/b.glsl", "c.glsl").as_deref(), Some("a/c.glsl"));
        assert_eq!(normalize_pack_path_clamped("x.fsh", ".."), None);
    }

    #[test]
    fn memory_sources() {
        let m = MemorySources::new().with("/lib/a.glsl", "x").with("b.fsh", "y");
        assert_eq!(m.read("lib/a.glsl").as_deref(), Some("x"));
        assert_eq!(m.read("b.fsh").as_deref(), Some("y"));
        assert!(m.read("nope").is_none());
    }
}
