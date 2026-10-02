//! [`EditedSources`]: pack sources with option values applied by editing lines, as Iris
//! does before include expansion and preprocessing.

use super::annotate::{LineAnnotation, annotate_line_with, replace_span, set_boolean_define};
use super::{DiscoveredOptions, OptionValues, effective};
use crate::ShaderPack;
use crate::text::line_spans;
use crate::vfs::clean_path;
use sb_core::SourceProvider;
use sb_core::model::OptionKind;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

/// Desired state of one option line.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LineEdit {
    /// Boolean `#define`: enabled or not.
    Toggle(bool),
    /// Value define / const: new value text.
    Value(String),
}

/// A [`SourceProvider`] over a pack that returns every file with the user's option
/// values applied:
///
/// * boolean `#define`s are toggled by adding or removing a leading `//`;
/// * value `#define`s and `const` options get their value token replaced in place
///   (the rest of the line, including comments, is kept).
///
/// Line structure is preserved exactly, so line numbers stay valid. Files without
/// changes are returned unmodified. Results are cached.
pub struct EditedSources {
    pack: Arc<ShaderPack>,
    options: DiscoveredOptions,
    values: OptionValues,
    /// file -> 1-based line -> edit (only lines whose value differs from the default).
    edits: HashMap<String, BTreeMap<u32, LineEdit>>,
    cache: Mutex<HashMap<String, Option<Arc<str>>>>,
}

impl std::fmt::Debug for EditedSources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditedSources")
            .field("pack", &self.pack.name())
            .field("options", &self.options.options.len())
            .field("values", &self.values)
            .field("edited_files", &self.edits.len())
            .finish()
    }
}

impl EditedSources {
    /// Wrap `pack`, applying `values` to the option lines found by discovery.
    pub fn new(pack: Arc<ShaderPack>, options: DiscoveredOptions, values: OptionValues) -> Self {
        let by_name: HashMap<&str, &sb_core::model::PackOption> = options
            .options
            .iter()
            .map(|o| (o.name.as_str(), o))
            .collect();
        let mut edits: HashMap<String, BTreeMap<u32, LineEdit>> = HashMap::new();
        for (file, occ) in &options.occurrences {
            for (line, o) in occ {
                let Some(opt) = by_name.get(o.name.as_str()) else {
                    continue;
                };
                let value = effective(opt, values.get(&o.name));
                if value == o.default {
                    continue;
                }
                let edit = match o.kind {
                    OptionKind::BooleanDefine => LineEdit::Toggle(value == "true"),
                    OptionKind::ValueDefine | OptionKind::Const => LineEdit::Value(value),
                };
                edits.entry(file.clone()).or_default().insert(*line, edit);
            }
        }
        Self {
            pack,
            options,
            values,
            edits,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The underlying pack.
    pub fn pack(&self) -> &Arc<ShaderPack> {
        &self.pack
    }

    /// The options used for editing.
    pub fn options(&self) -> &DiscoveredOptions {
        &self.options
    }

    /// The values applied.
    pub fn values(&self) -> &OptionValues {
        &self.values
    }

    /// Files that contain at least one edited line, sorted.
    pub fn edited_files(&self) -> Vec<String> {
        let mut v: Vec<String> = self.edits.keys().cloned().collect();
        v.sort();
        v
    }

    /// Apply the edits for `path` to `text` (the file's original contents).
    pub fn edit_text(&self, path: &str, text: &str) -> String {
        match self.edits.get(path) {
            Some(edits) => apply_edits(text, edits, self.options.config.optifine_numbers),
            None => text.to_string(),
        }
    }

    fn load(&self, path: &str) -> Option<Arc<str>> {
        let text = self.pack.read_text(path)?;
        Some(Arc::from(self.edit_text(path, &text)))
    }
}

impl SourceProvider for EditedSources {
    fn read(&self, path: &str) -> Option<Arc<str>> {
        let path = clean_path(path).filter(|p| !p.is_empty())?;
        if let Some(hit) = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&path)
        {
            return hit.clone();
        }
        let loaded = self.load(&path);
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path, loaded.clone());
        loaded
    }
}

/// Re-annotate an option line and produce its edited form (None = unchanged).
fn edit_line(line: &str, edit: &LineEdit, optifine_numbers: bool) -> Option<String> {
    match (annotate_line_with(line, optifine_numbers)?, edit) {
        (LineAnnotation::BoolDefine { .. }, LineEdit::Toggle(on)) => set_boolean_define(line, *on),
        (LineAnnotation::ConstBool { value_span, .. }, LineEdit::Value(v))
        | (LineAnnotation::ValueDefine { value_span, .. }, LineEdit::Value(v))
        | (LineAnnotation::ConstValue { value_span, .. }, LineEdit::Value(v)) => {
            (line.get(value_span.clone()) != Some(v.as_str()))
                .then(|| replace_span(line, value_span, v))
                .flatten()
        }
        _ => None,
    }
}

fn apply_edits(text: &str, edits: &BTreeMap<u32, LineEdit>, optifine_numbers: bool) -> String {
    let mut out = String::with_capacity(text.len() + 16 * edits.len());
    let mut copied = 0;
    for (i, span) in line_spans(text).into_iter().enumerate() {
        let Some(edit) = edits.get(&((i + 1) as u32)) else {
            continue;
        };
        if let Some(new_line) = edit_line(span.text, edit, optifine_numbers) {
            out.push_str(&text[copied..span.start]);
            out.push_str(&new_line);
            copied = span.end;
        }
    }
    out.push_str(&text[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::discover;
    use pretty_assertions::assert_eq;

    const SETTINGS: &str = "#define SHADOWS // Shadows\r\n\
                            //#define BLOOM\r\n\
                            #define QUALITY 2 // [1 2 3]\r\n\
                            const int shadowMapResolution = 2048; // [1024 2048 4096]\r\n\
                            const bool shadowHardwareFiltering = true;\r\n\
                            #ifdef SHADOWS\r\n#endif\r\n#ifdef BLOOM\r\n#endif\r\n\
                            #ifdef shadowHardwareFiltering\r\n#endif\r\n";

    fn setup(values: &[(&str, &str)]) -> EditedSources {
        let pack = ShaderPack::from_files(
            "t",
            [
                (
                    "composite.fsh",
                    "#include \"/lib/settings.glsl\"\nvoid main() {}\n",
                ),
                ("lib/settings.glsl", SETTINGS),
                ("lib/untouched.glsl", "x"),
            ],
        );
        let (o, _) = discover(&pack, &["composite.fsh".to_string()]);
        EditedSources::new(
            Arc::new(pack),
            o,
            OptionValues::from_pairs(values.iter().copied()),
        )
    }

    #[test]
    fn defaults_leave_files_untouched() {
        let s = setup(&[("QUALITY", "2"), ("SHADOWS", "true")]);
        assert!(s.edited_files().is_empty());
        assert_eq!(s.read("lib/settings.glsl").as_deref(), Some(SETTINGS));
    }

    #[test]
    fn edits_preserve_line_structure() {
        let s = setup(&[
            ("SHADOWS", "false"),
            ("BLOOM", "true"),
            ("QUALITY", "3"),
            ("shadowMapResolution", "4096"),
            ("shadowHardwareFiltering", "false"),
        ]);
        assert_eq!(s.edited_files(), vec!["lib/settings.glsl"]);
        let text = s.read("/lib/settings.glsl").unwrap();
        let expected = "//#define SHADOWS // Shadows\r\n\
                        #define BLOOM\r\n\
                        #define QUALITY 3 // [1 2 3]\r\n\
                        const int shadowMapResolution = 4096; // [1024 2048 4096]\r\n\
                        const bool shadowHardwareFiltering = false;\r\n\
                        #ifdef SHADOWS\r\n#endif\r\n#ifdef BLOOM\r\n#endif\r\n\
                        #ifdef shadowHardwareFiltering\r\n#endif\r\n";
        assert_eq!(&*text, expected);
        assert_eq!(text.lines().count(), SETTINGS.lines().count());
        // Cached: the same Arc comes back.
        assert!(Arc::ptr_eq(&text, &s.read("lib/settings.glsl").unwrap()));
        // Other files are returned as-is; missing ones are None.
        assert_eq!(s.read("lib/untouched.glsl").as_deref(), Some("x"));
        assert_eq!(s.read("lib/nope.glsl"), None);
        assert_eq!(s.read("../escape"), None);
    }

    #[test]
    fn invalid_values_fall_back_to_defaults() {
        let s = setup(&[("SHADOWS", "maybe"), ("QUALITY", "1\n#define EVIL")]);
        assert!(s.edited_files().is_empty());
    }

    #[test]
    fn optifine_number_syntax_round_trips_through_editing() {
        use crate::options::{DiscoverConfig, discover_with};
        let pack = ShaderPack::from_files("t", [("composite.fsh", "#define FOG 5. // [1. 5.]\n")]);
        let config = DiscoverConfig {
            optifine_numbers: true,
            ..Default::default()
        };
        let (o, _) = discover_with(&pack, &["composite.fsh".to_string()], config);
        assert_eq!(o.option("FOG").unwrap().default, "5.");
        let s = EditedSources::new(Arc::new(pack), o, OptionValues::from_pairs([("FOG", "1.")]));
        assert_eq!(
            s.read("composite.fsh").as_deref(),
            Some("#define FOG 1. // [1. 5.]\n")
        );
    }

    #[test]
    fn values_outside_the_allowed_list_are_applied() {
        // Iris does not check values from the settings file against the list.
        let s = setup(&[("QUALITY", "7")]);
        assert!(
            s.read("lib/settings.glsl")
                .unwrap()
                .contains("#define QUALITY 7 // [1 2 3]")
        );
    }
}
