//! Pack access for a compile: a borrowed or shared [`ShaderPack`] and a
//! [`SourceProvider`] that applies the user's option values by editing lines (Iris
//! `OptionAnnotatedSource`), as `sb_pack::EditedSources` does, but over a borrowed pack.

use sb_core::SourceProvider;
use sb_pack::{DiscoveredOptions, EditedSources, OptionValues, ShaderPack};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};

/// A pack that a [`crate::PackSession`] reads: borrowed (one-shot compiles) or shared
/// (long-lived sessions, e.g. the JNI layer).
#[derive(Clone)]
pub enum PackRef<'p> {
    Borrowed(&'p ShaderPack),
    Shared(Arc<ShaderPack>),
}

impl Deref for PackRef<'_> {
    type Target = ShaderPack;
    fn deref(&self) -> &ShaderPack {
        match self {
            PackRef::Borrowed(p) => p,
            PackRef::Shared(p) => p,
        }
    }
}

impl std::fmt::Debug for PackRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PackRef").field(&self.name()).finish()
    }
}

/// Pack sources with option edits applied. Results are cached per path.
pub struct OptionSources<'p> {
    pack: PackRef<'p>,
    /// Edits only (its own pack handle is an empty placeholder; texts come from `pack`).
    editor: EditedSources,
    cache: Mutex<HashMap<String, Option<Arc<str>>>>,
}

impl std::fmt::Debug for OptionSources<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OptionSources").field("pack", &self.pack.name()).field("editor", &self.editor).finish()
    }
}

impl<'p> OptionSources<'p> {
    /// Sources of `pack` with `values` applied to the option lines of `options`.
    pub fn new(pack: PackRef<'p>, options: DiscoveredOptions, values: OptionValues) -> Self {
        let placeholder = Arc::new(ShaderPack::from_files("", Vec::<(&str, &str)>::new()));
        Self { pack, editor: EditedSources::new(placeholder, options, values), cache: Mutex::new(HashMap::new()) }
    }
}

impl SourceProvider for OptionSources<'_> {
    fn read(&self, path: &str) -> Option<Arc<str>> {
        let path = sb_pack::vfs::clean_path(path).filter(|p| !p.is_empty())?;
        if let Some(hit) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(&path) {
            return hit.clone();
        }
        let loaded = self.pack.read_text(&path).map(|t| Arc::from(self.editor.edit_text(&path, &t)));
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(path, loaded.clone());
        loaded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_option_edits() {
        let pack = ShaderPack::from_files(
            "t",
            [("composite.fsh", "#define BLOOM\n#ifdef BLOOM\n#endif\n#define QUALITY 2 // [1 2 3]\nvoid main(){}\n")],
        );
        let (opts, _) = sb_pack::options::discover(&pack, &["composite.fsh".to_string()]);
        let values = OptionValues::from_pairs([("BLOOM", "false"), ("QUALITY", "3")]);
        let src = OptionSources::new(PackRef::Borrowed(&pack), opts, values);
        let text = src.read("composite.fsh").unwrap();
        assert!(text.starts_with("//#define BLOOM\n"), "{text}");
        assert!(text.contains("#define QUALITY 3"), "{text}");
        assert_eq!(src.read("./composite.fsh").as_deref(), Some(&*text));
        assert!(src.read("missing.fsh").is_none());
    }
}
