//! User options: discovery (Iris `OptionAnnotatedSource` rules), user values, profiles,
//! the GUI model and source editing.
//!
//! # Discovery rules
//!
//! Every file reachable through `#include` from the start files (the program files of
//! the root and the world folders) is scanned line by line:
//!
//! * `[//]#define NAME [// comment]` is a **boolean** option, on unless the line starts
//!   with `//`, but only if `#ifdef NAME` or `#ifndef NAME` (exactly, nothing after
//!   the name) appears in a file of the same weakly connected include component.
//!   `#if defined(NAME)` does not count (OptiFine compatibility).
//! * `#define NAME VALUE // [v1 v2 ...] comment` is a **value** option; the default is
//!   appended to the allowed values if missing. A leading `//` disqualifies it.
//! * `const int|float NAME = VALUE; // [list]` and `const bool NAME = true|false;` are
//!   **const** options, for a fixed whitelist of names
//!   ([`annotate::CONST_OPTION_NAMES`]). Like boolean `#define`s, a `const bool` is
//!   only an option when `#ifdef NAME`/`#ifndef NAME` appears in its component: Iris
//!   (`OptionAnnotatedSource.getOptionSet`) filters every boolean option by those
//!   references. [`DiscoverConfig::optifine_const_bools`] lifts that requirement.
//! * Occurrences of one name are merged when their defaults agree. Names declared with
//!   different defaults (or both as boolean and as value option) are **ambiguous**
//!   and dropped with a warning.
//!
//! Iris itself currently treats the whole pack as one component (its
//! `computeWeaklyConnectedComponents` is unimplemented); use
//! [`ReferenceScope::WholePack`] to reproduce that exactly.

pub mod annotate;
mod edited;
mod model;
mod profiles;
mod values;

pub use edited::EditedSources;
pub use model::build_options_model;
pub use profiles::{Profile, detect_profile, resolve_profiles};
pub use values::OptionValues;

use crate::ShaderPack;
use crate::includes::IncludeGraph;
use crate::text::lines;
use annotate::{LineAnnotation, annotate_line_with};
use indexmap::IndexMap;
use sb_core::model::{OptionKind, PackOption};
use sb_core::{Diagnostic, Diagnostics, SourceLocation};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Where `#ifdef`/`#ifndef` references confirm boolean `#define` options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ReferenceScope {
    /// Files of the same weakly connected include component (the documented rule).
    #[default]
    Component,
    /// Any file of the pack (what Iris 1.11 actually does).
    WholePack,
}

/// Discovery settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DiscoverConfig {
    pub reference_scope: ReferenceScope,
    /// Also accept OptiFine's value syntax `#define NAME 5. // [1. 5.]` (numbers ending
    /// in `.`), which Iris rejects. Off by default (Iris behaviour).
    pub optifine_numbers: bool,
    /// Accept whitelisted `const bool` options without an `#ifdef`/`#ifndef` reference
    /// (OptiFine). Off by default: Iris requires the reference for every boolean option.
    #[serde(default)]
    pub optifine_const_bools: bool,
}

/// One option declaration line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionOccurrence {
    pub name: String,
    pub kind: OptionKind,
    /// Boolean option (`true`/`false` values) vs value option.
    pub boolean: bool,
    /// Default as written on this line.
    pub default: String,
}

/// The options of a pack.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DiscoveredOptions {
    /// Options in order of first occurrence (files sorted by path, then line order).
    /// `value` equals `default` until [`DiscoveredOptions::apply_values`] is called.
    pub options: Vec<PackOption>,
    /// file -> 1-based line -> occurrence, for every accepted option declaration.
    pub occurrences: BTreeMap<String, BTreeMap<u32, OptionOccurrence>>,
    /// Names dropped because their declarations disagree.
    pub ambiguous: Vec<String>,
    /// Every scanned file (the include graph), sorted.
    pub files: Vec<String>,
    /// The settings discovery ran with (editing must use the same syntax rules).
    #[serde(default)]
    pub config: DiscoverConfig,
}

/// Whether a [`PackOption`] takes `true`/`false` values.
pub fn is_boolean_option(o: &PackOption) -> bool {
    match o.kind {
        OptionKind::BooleanDefine => true,
        OptionKind::ValueDefine => false,
        OptionKind::Const => o.allowed.is_empty(),
    }
}

/// Discover the options of `pack` reachable from `start_files` (usually
/// [`ShaderPack::option_start_files`]).
pub fn discover(pack: &ShaderPack, start_files: &[String]) -> (DiscoveredOptions, Diagnostics) {
    discover_with(pack, start_files, DiscoverConfig::default())
}

/// [`discover`] with explicit settings.
pub fn discover_with(
    pack: &ShaderPack,
    start_files: &[String],
    config: DiscoverConfig,
) -> (DiscoveredOptions, Diagnostics) {
    let (graph, mut diags) = IncludeGraph::build(pack, start_files);
    let (options, d) = discover_in_graph(&graph, config);
    diags.extend(d);
    (options, diags)
}

/// A merged option: (option, is boolean, accepted `(file, line)` declarations).
type Merged = (PackOption, bool, Vec<(String, u32)>);

struct Candidate {
    file: String,
    line: u32,
    name: String,
    kind: OptionKind,
    boolean: bool,
    default: String,
    allowed: Vec<String>,
    comment: Option<String>,
}

/// Discover options in an already built include graph.
pub fn discover_in_graph(
    graph: &IncludeGraph,
    config: DiscoverConfig,
) -> (DiscoveredOptions, Diagnostics) {
    let mut diags = Diagnostics::new();

    // Annotate every file once.
    let mut annotations: BTreeMap<&str, Vec<(u32, LineAnnotation)>> = BTreeMap::new();
    for (path, node) in &graph.files {
        let anns: Vec<(u32, LineAnnotation)> = lines(&node.text)
            .into_iter()
            .enumerate()
            .filter_map(|(i, l)| {
                annotate_line_with(l, config.optifine_numbers).map(|a| ((i + 1) as u32, a))
            })
            .collect();
        annotations.insert(path.as_str(), anns);
    }

    // References per component.
    let components: Vec<Vec<String>> = match config.reference_scope {
        ReferenceScope::Component => graph.components(),
        ReferenceScope::WholePack => vec![graph.files.keys().cloned().collect()],
    };
    let mut referenced_by_file: HashMap<&str, usize> = HashMap::new();
    let mut referenced: Vec<HashSet<&str>> = Vec::with_capacity(components.len());
    for (ci, comp) in components.iter().enumerate() {
        let mut refs = HashSet::new();
        for f in comp {
            if let Some((key, anns)) = annotations.get_key_value(f.as_str()) {
                referenced_by_file.insert(key, ci);
                for (_, a) in anns {
                    if let LineAnnotation::Reference(n) = a {
                        refs.insert(n.as_str());
                    }
                }
            }
        }
        referenced.push(refs);
    }

    // Collect candidates in deterministic order.
    let mut candidates = Vec::new();
    for (file, anns) in &annotations {
        let refs = referenced_by_file.get(file).map(|&ci| &referenced[ci]);
        for (line, a) in anns {
            let c = match a {
                LineAnnotation::Reference(_) => continue,
                LineAnnotation::BoolDefine {
                    name,
                    default,
                    comment,
                } => {
                    if !refs.is_some_and(|r| r.contains(name.as_str())) {
                        continue;
                    }
                    Candidate {
                        file: file.to_string(),
                        line: *line,
                        name: name.clone(),
                        kind: OptionKind::BooleanDefine,
                        boolean: true,
                        default: default.to_string(),
                        allowed: Vec::new(),
                        comment: comment.clone(),
                    }
                }
                LineAnnotation::ValueDefine {
                    name,
                    value,
                    allowed,
                    comment,
                    ..
                } => Candidate {
                    file: file.to_string(),
                    line: *line,
                    name: name.clone(),
                    kind: OptionKind::ValueDefine,
                    boolean: false,
                    default: value.clone(),
                    allowed: allowed.clone(),
                    comment: comment.clone(),
                },
                LineAnnotation::ConstBool {
                    name,
                    default,
                    comment,
                    ..
                } => {
                    if !config.optifine_const_bools
                        && !refs.is_some_and(|r| r.contains(name.as_str()))
                    {
                        continue;
                    }
                    Candidate {
                        file: file.to_string(),
                        line: *line,
                        name: name.clone(),
                        kind: OptionKind::Const,
                        boolean: true,
                        default: default.to_string(),
                        allowed: Vec::new(),
                        comment: comment.clone(),
                    }
                }
                LineAnnotation::ConstValue {
                    name,
                    value,
                    allowed,
                    comment,
                    ..
                } => Candidate {
                    file: file.to_string(),
                    line: *line,
                    name: name.clone(),
                    kind: OptionKind::Const,
                    boolean: false,
                    default: value.clone(),
                    allowed: allowed.clone(),
                    comment: comment.clone(),
                },
            };
            candidates.push(c);
        }
    }

    // Merge.
    // name -> (merged option, is boolean, accepted declaration lines)
    let mut merged: IndexMap<String, Merged> = IndexMap::new();
    let mut ambiguous: BTreeMap<String, (String, u32)> = BTreeMap::new();
    for c in &candidates {
        if ambiguous.contains_key(&c.name) {
            continue;
        }
        match merged.get_mut(&c.name) {
            Some((opt, boolean, locs)) => {
                if *boolean != c.boolean || opt.default != c.default {
                    let first = (opt.file.clone(), opt.line);
                    let what = if *boolean != c.boolean {
                        "is declared both as a boolean and as a value option".to_string()
                    } else {
                        format!(
                            "has different defaults (`{}` at {}:{}, `{}` here)",
                            opt.default, opt.file, opt.line, c.default
                        )
                    };
                    diags.push(
                        Diagnostic::warning(
                            "opt.ambiguous",
                            format!("option `{}` {what}; it is ignored", c.name),
                        )
                        .at(SourceLocation::new(c.file.clone(), c.line)),
                    );
                    merged.shift_remove(&c.name);
                    ambiguous.insert(c.name.clone(), first);
                    continue;
                }
                if opt.comment.is_none() {
                    opt.comment = c.comment.clone();
                }
                for v in &c.allowed {
                    if !opt.allowed.contains(v) {
                        opt.allowed.push(v.clone());
                    }
                }
                locs.push((c.file.clone(), c.line));
            }
            None => {
                let opt = PackOption {
                    name: c.name.clone(),
                    kind: c.kind,
                    default: c.default.clone(),
                    value: c.default.clone(),
                    allowed: c.allowed.clone(),
                    comment: c.comment.clone(),
                    file: c.file.clone(),
                    line: c.line,
                };
                merged.insert(
                    c.name.clone(),
                    (opt, c.boolean, vec![(c.file.clone(), c.line)]),
                );
            }
        }
    }

    let mut out = DiscoveredOptions {
        files: graph.files.keys().cloned().collect(),
        ambiguous: ambiguous.keys().cloned().collect(),
        config,
        ..Default::default()
    };
    let accepted: HashSet<(&str, u32)> = merged
        .values()
        .flat_map(|(_, _, locs)| locs.iter().map(|(f, l)| (f.as_str(), *l)))
        .collect();
    for c in &candidates {
        if accepted.contains(&(c.file.as_str(), c.line)) {
            out.occurrences.entry(c.file.clone()).or_default().insert(
                c.line,
                OptionOccurrence {
                    name: c.name.clone(),
                    kind: c.kind,
                    boolean: c.boolean,
                    default: c.default.clone(),
                },
            );
        }
    }
    out.options = merged.into_values().map(|(o, _, _)| o).collect();
    (out, diags)
}

impl DiscoveredOptions {
    /// Look an option up by name.
    pub fn option(&self, name: &str) -> Option<&PackOption> {
        self.options.iter().find(|o| o.name == name)
    }

    /// `Some(true)` for boolean options, `Some(false)` for value options.
    pub fn is_boolean(&self, name: &str) -> Option<bool> {
        self.option(name).map(is_boolean_option)
    }

    /// Effective value of `name` under `values`: the user value if valid (`true` /
    /// `false` for booleans, anything without line breaks for value options), else the
    /// default. `None` for unknown options.
    pub fn effective_value(&self, name: &str, values: &OptionValues) -> Option<String> {
        let o = self.option(name)?;
        Some(effective(o, values.get(name)))
    }

    /// Set every option's `value` from `values`.
    pub fn apply_values(&mut self, values: &OptionValues) {
        for o in &mut self.options {
            o.value = effective(o, values.get(&o.name));
        }
    }

    /// Macros for preprocessing `.properties` files (Iris `PropertiesPreprocessor`):
    /// boolean options that are on (no value) and every value option with its value.
    pub fn property_macros(&self, values: &OptionValues) -> IndexMap<String, Option<String>> {
        let mut out = IndexMap::new();
        for o in &self.options {
            let v = effective(o, values.get(&o.name));
            if is_boolean_option(o) {
                if v == "true" {
                    out.insert(o.name.clone(), None);
                }
            } else {
                out.insert(o.name.clone(), Some(v));
            }
        }
        out
    }

    /// Occurrences in one file (1-based line -> occurrence).
    pub fn occurrences_in(&self, file: &str) -> Option<&BTreeMap<u32, OptionOccurrence>> {
        self.occurrences.get(file)
    }

    /// Names of all options, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.options.iter().map(|o| o.name.as_str())
    }

    /// Number of option declarations accepted (all files).
    pub fn occurrence_count(&self) -> usize {
        self.occurrences.values().map(BTreeMap::len).sum()
    }

    /// Files that declare at least one option, sorted.
    pub fn declaring_files(&self) -> BTreeSet<&str> {
        self.occurrences.keys().map(String::as_str).collect()
    }
}

/// The effective value of an option for a raw user value.
pub(crate) fn effective(o: &PackOption, user: Option<&str>) -> String {
    match user {
        Some(v) if is_boolean_option(o) => match v {
            "true" | "false" => v.to_string(),
            _ => o.default.clone(),
        },
        Some(v) if !v.contains(['\n', '\r']) && !v.trim().is_empty() => v.to_string(),
        _ => o.default.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn pack(files: &[(&str, &str)]) -> ShaderPack {
        ShaderPack::from_files("t", files.iter().map(|(p, c)| (*p, c.as_bytes().to_vec())))
    }

    fn starts(files: &[&str]) -> Vec<String> {
        files.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn boolean_defines_need_a_reference_in_the_component() {
        let p = pack(&[
            (
                "composite.fsh",
                "#include \"/lib/settings.glsl\"\n#ifdef BLOOM\n#endif\n#if defined DOF\n#endif",
            ),
            (
                "lib/settings.glsl",
                "#define BLOOM // Bloom\n//#define DOF\n#define UNUSED",
            ),
            ("final.fsh", "#ifdef ISOLATED\n#endif\n#define LONELY"),
            ("other.fsh", "#define ISOLATED\n#ifdef LONELY\n#endif"),
        ]);
        let (o, d) = discover(&p, &starts(&["composite.fsh", "final.fsh", "other.fsh"]));
        assert_eq!(o.names().collect::<Vec<_>>(), vec!["BLOOM"]);
        assert_eq!(o.option("BLOOM").unwrap().comment.as_deref(), Some("Bloom"));
        assert_eq!(o.option("BLOOM").unwrap().file, "lib/settings.glsl");
        assert_eq!(o.option("BLOOM").unwrap().line, 1);
        assert!(d.is_empty());

        // Iris's actual behaviour: one component for the whole pack.
        let config = DiscoverConfig {
            reference_scope: ReferenceScope::WholePack,
            ..Default::default()
        };
        let (o, _) = discover_with(
            &p,
            &starts(&["composite.fsh", "final.fsh", "other.fsh"]),
            config,
        );
        assert_eq!(
            o.names().collect::<Vec<_>>(),
            vec!["LONELY", "BLOOM", "ISOLATED"]
        );
    }

    #[test]
    fn value_and_const_options() {
        let p = pack(&[(
            "composite.fsh",
            "#define QUALITY 2 // [1 2 3] Quality\nconst int shadowMapResolution = 2048; // [1024 2048]\n\
             const bool shadowHardwareFiltering = true;\n#define NOT_AN_OPTION 3\nconst float sunPathRotation = 30.0;\n\
             #ifdef shadowHardwareFiltering\n#endif",
        )]);
        let (o, _) = discover(&p, &starts(&["composite.fsh"]));
        let q = o.option("QUALITY").unwrap();
        assert_eq!(
            (q.kind, q.default.as_str(), q.allowed.clone()),
            (
                OptionKind::ValueDefine,
                "2",
                vec!["1".into(), "2".into(), "3".into()]
            )
        );
        assert_eq!(
            o.option("shadowMapResolution").unwrap().kind,
            OptionKind::Const
        );
        assert_eq!(o.is_boolean("shadowHardwareFiltering"), Some(true));
        assert_eq!(o.is_boolean("shadowMapResolution"), Some(false));
        assert_eq!(o.option("sunPathRotation"), None, "no list");
        assert_eq!(o.occurrence_count(), 3);
    }

    #[test]
    fn const_bool_options_need_a_reference_like_boolean_defines() {
        // Regression: Iris's `getOptionSet` filters *every* boolean option (including
        // `const bool`) by `#ifdef`/`#ifndef` references; photon's unreferenced
        // `shadowHardwareFiltering1` is therefore not an option in Iris.
        let p = pack(&[(
            "shadow.fsh",
            "const bool shadowHardwareFiltering1 = true;\nconst bool shadowtex0Mipmap = false; // m\n\
             #ifndef shadowtex0Mipmap\n#endif",
        )]);
        let (o, _) = discover(&p, &starts(&["shadow.fsh"]));
        assert_eq!(o.names().collect::<Vec<_>>(), vec!["shadowtex0Mipmap"]);
        assert_eq!(
            o.option("shadowtex0Mipmap").unwrap().kind,
            OptionKind::Const
        );
        // OptiFine accepts them without a reference.
        let config = DiscoverConfig {
            optifine_const_bools: true,
            ..Default::default()
        };
        let (o, _) = discover_with(&p, &starts(&["shadow.fsh"]), config);
        assert_eq!(
            o.names().collect::<Vec<_>>(),
            vec!["shadowHardwareFiltering1", "shadowtex0Mipmap"]
        );
    }

    #[test]
    fn merging_and_ambiguity() {
        let p = pack(&[
            (
                "a.fsh",
                "#include \"b.glsl\"\n#define SAME 1 // [1 2]\n#define DIFF 1 // [1 2]\n#define MIX\n#ifdef MIX\n#endif",
            ),
            (
                "b.glsl",
                "#define SAME 1 // [1 3] More\n#define DIFF 2 // [1 2]\n#define MIX 1 // [1 2]\n#define DIFF 1 // [1 2]",
            ),
        ]);
        let (o, d) = discover(&p, &starts(&["a.fsh"]));
        assert_eq!(o.names().collect::<Vec<_>>(), vec!["SAME"]);
        let same = o.option("SAME").unwrap();
        assert_eq!(same.allowed, vec!["1", "2", "3"]);
        assert_eq!(same.comment.as_deref(), Some("More"));
        assert_eq!(o.ambiguous, vec!["DIFF", "MIX"]);
        assert_eq!(d.iter().filter(|d| d.code == "opt.ambiguous").count(), 2);
        // Only lines of accepted options are recorded.
        assert_eq!(
            o.occurrences["a.fsh"].keys().copied().collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(
            o.occurrences["b.glsl"].keys().copied().collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn effective_values_and_macros() {
        let p = pack(&[(
            "a.fsh",
            "#define ON\n//#define OFF\n#ifdef ON\n#endif\n#ifdef OFF\n#endif\n#define N 2 // [1 2]",
        )]);
        let (mut o, _) = discover(&p, &starts(&["a.fsh"]));
        let mut v = OptionValues::new();
        v.set("OFF", "true");
        v.set("ON", "yes");
        v.set("N", "1");
        assert_eq!(
            o.effective_value("ON", &v).as_deref(),
            Some("true"),
            "invalid boolean -> default"
        );
        assert_eq!(o.effective_value("OFF", &v).as_deref(), Some("true"));
        assert_eq!(o.effective_value("N", &v).as_deref(), Some("1"));
        assert_eq!(o.effective_value("MISSING", &v), None);
        let m = o.property_macros(&v);
        assert_eq!(m.get("ON"), Some(&None));
        assert_eq!(m.get("OFF"), Some(&None));
        assert_eq!(m.get("N"), Some(&Some("1".to_string())));
        o.apply_values(&v);
        assert_eq!(o.option("N").unwrap().value, "1");
        assert_eq!(o.option("N").unwrap().default, "2");
    }
}
