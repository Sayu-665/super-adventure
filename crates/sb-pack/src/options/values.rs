//! User option values (the Iris `shaderpacks/<pack>.txt` settings file).

use super::{DiscoveredOptions, Profile, effective, is_boolean_option};
use crate::properties;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// User overrides: option name -> value string (`true`/`false` for booleans).
///
/// Values are stored as given; [`OptionValues::normalized`] drops unknown options,
/// invalid booleans and values equal to the default, as Iris does when loading.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionValues {
    pub values: BTreeMap<String, String>,
}

impl OptionValues {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a settings file (`NAME=value` lines in `.properties` syntax; decode the
    /// file as ISO-8859-1). Values are trimmed.
    pub fn parse_settings_file(text: &str) -> OptionValues {
        let values = properties::parse(text)
            .into_iter()
            .map(|e| (e.key, e.value.trim().to_string()))
            .collect();
        OptionValues { values }
    }

    /// Serialize as a settings file (sorted `NAME=value` lines, `.properties` escaping).
    pub fn to_settings_file(&self) -> String {
        properties::write(self.values.iter().map(|(k, v)| (k.as_str(), v.as_str())))
    }

    /// Build from `(name, value)` pairs.
    pub fn from_pairs<K: Into<String>, V: Into<String>>(
        pairs: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        Self {
            values: pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.values.insert(name.into(), value.into());
    }

    pub fn remove(&mut self, name: &str) -> Option<String> {
        self.values.remove(name)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Only known options with valid, non-default values.
    pub fn normalized(&self, options: &DiscoveredOptions) -> OptionValues {
        let mut out = OptionValues::new();
        for o in &options.options {
            if let Some(v) = self.get(&o.name) {
                let eff = effective(o, Some(v));
                if eff != o.default {
                    out.set(o.name.clone(), eff);
                }
            }
        }
        out
    }

    /// Apply a profile: set every option it lists (values equal to the default are
    /// removed, so the result stays normalized for known options).
    pub fn apply_profile(&mut self, profile: &Profile, options: &DiscoveredOptions) {
        for (name, value) in &profile.options {
            match options.option(name) {
                Some(o) if effective(o, Some(value)) == o.default => {
                    self.values.remove(name);
                }
                _ => self.set(name.clone(), value.clone()),
            }
        }
    }

    /// Number of options whose effective value differs from the default.
    pub fn changed_count(&self, options: &DiscoveredOptions) -> usize {
        options
            .options
            .iter()
            .filter(|o| {
                self.get(&o.name)
                    .is_some_and(|v| effective(o, Some(v)) != o.default)
            })
            .count()
    }

    /// Toggle helper for GUIs: the next allowed value of a value option (wrapping), or
    /// the negation of a boolean option.
    pub fn cycle(&mut self, name: &str, options: &DiscoveredOptions) -> Option<String> {
        let o = options.option(name)?;
        let current = effective(o, self.get(name));
        let next = if is_boolean_option(o) {
            if current == "true" {
                "false".to_string()
            } else {
                "true".to_string()
            }
        } else {
            let pos = o.allowed.iter().position(|v| *v == current);
            match pos {
                Some(i) => o.allowed[(i + 1) % o.allowed.len()].clone(),
                None => o.allowed.first().cloned().unwrap_or(current),
            }
        };
        if next == o.default {
            self.values.remove(name);
        } else {
            self.set(name, next.clone());
        }
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ShaderPack;
    use crate::options::discover;
    use indexmap::IndexMap;

    fn options() -> DiscoveredOptions {
        let p = ShaderPack::from_files(
            "t",
            [(
                "a.fsh",
                "#define B\n#ifdef B\n#endif\n#define V 2 // [1 2 3]",
            )],
        );
        discover(&p, &["a.fsh".to_string()]).0
    }

    #[test]
    fn settings_file_roundtrip() {
        let v = OptionValues::parse_settings_file(
            "# saved by Iris\nSHADOWS=false\nQUALITY = 3 \nNAME=a\\=b\n",
        );
        assert_eq!(v.get("SHADOWS"), Some("false"));
        assert_eq!(v.get("QUALITY"), Some("3"));
        assert_eq!(v.get("NAME"), Some("a=b"));
        let text = v.to_settings_file();
        assert_eq!(text, "NAME=a\\=b\nQUALITY=3\nSHADOWS=false\n");
        assert_eq!(OptionValues::parse_settings_file(&text), v);
    }

    #[test]
    fn normalization() {
        let o = options();
        let v = OptionValues::from_pairs([("B", "true"), ("V", "3"), ("UNKNOWN", "1"), ("X", "y")]);
        assert_eq!(v.normalized(&o), OptionValues::from_pairs([("V", "3")]));
        let v = OptionValues::from_pairs([("B", "nope"), ("V", "2")]);
        assert!(v.normalized(&o).is_empty());
        assert_eq!(
            OptionValues::from_pairs([("B", "false")]).changed_count(&o),
            1
        );
    }

    #[test]
    fn profiles_apply() {
        let o = options();
        let mut opts = IndexMap::new();
        opts.insert("B".to_string(), "false".to_string());
        opts.insert("V".to_string(), "2".to_string());
        opts.insert("OTHER".to_string(), "1".to_string());
        let profile = Profile {
            name: "LOW".into(),
            options: opts,
            disabled_programs: vec![],
        };
        let mut v = OptionValues::from_pairs([("V", "3")]);
        v.apply_profile(&profile, &o);
        assert_eq!(
            v,
            OptionValues::from_pairs([("B", "false"), ("OTHER", "1")])
        );
    }

    #[test]
    fn cycling() {
        let o = options();
        let mut v = OptionValues::new();
        assert_eq!(v.cycle("V", &o).as_deref(), Some("3"));
        assert_eq!(v.cycle("V", &o).as_deref(), Some("1"));
        assert_eq!(v.cycle("V", &o).as_deref(), Some("2"));
        assert!(v.is_empty());
        assert_eq!(v.cycle("B", &o).as_deref(), Some("false"));
        assert_eq!(v.get("B"), Some("false"));
        assert_eq!(v.cycle("NOPE", &o), None);
    }
}
