//! The `#include` graph of a pack (Iris `IncludeGraph`), used by option discovery.
//!
//! Every trimmed line starting with `#include` is an include, regardless of
//! preprocessor conditionals or comments (Iris resolves includes unconditionally).
//! Targets starting with `/` are relative to the shaders root, others to the including
//! file's directory ([`sb_core::normalize_pack_path`]).

use crate::ShaderPack;
use crate::text::lines;
use sb_core::{Diagnostic, Diagnostics, SourceLocation, normalize_pack_path};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

/// One file of the graph.
#[derive(Debug, Clone, PartialEq)]
pub struct FileNode {
    /// Normalized path as requested (may differ in case from the file on disk).
    pub path: String,
    /// File text (UTF-8, BOM stripped).
    pub text: Arc<str>,
    /// `(1-based line, resolved target)` for every include line.
    pub includes: Vec<(u32, String)>,
}

/// The include graph reachable from a set of start files.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IncludeGraph {
    /// Every readable file, by normalized path.
    pub files: BTreeMap<String, FileNode>,
}

/// Extract the target of an `#include` line, if the line is one.
///
/// Accepts `#include "path"`, `#include <path>` and `#include path`.
pub fn parse_include_line(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("#include")?;
    if !rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '<') {
        return None;
    }
    let rest = rest.trim();
    let target = if let Some(q) = rest.strip_prefix('"') {
        q.split('"').next().unwrap_or(q)
    } else if let Some(a) = rest.strip_prefix('<') {
        a.split('>').next().unwrap_or(a)
    } else {
        rest.split_whitespace().next().unwrap_or("")
    };
    let target = target.trim();
    (!target.is_empty()).then_some(target)
}

impl IncludeGraph {
    /// Read `start_files` and everything they (transitively) include.
    ///
    /// Problems (unreadable start file, missing include, include escaping the root,
    /// self-include) are reported as warnings; the preprocessor reports them again as
    /// errors when it expands the includes.
    pub fn build(pack: &ShaderPack, start_files: &[String]) -> (IncludeGraph, Diagnostics) {
        Self::build_with(|p| pack.read_text(p), start_files)
    }

    /// Like [`IncludeGraph::build`] with a custom file reader.
    pub fn build_with(
        mut read: impl FnMut(&str) -> Option<String>,
        start_files: &[String],
    ) -> (IncludeGraph, Diagnostics) {
        let mut diags = Diagnostics::new();
        let mut graph = IncludeGraph::default();
        let mut queue: VecDeque<String> = VecDeque::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for s in start_files {
            match normalize_pack_path("", s) {
                Some(p) => {
                    if seen.insert(p.clone()) {
                        queue.push_back(p);
                    }
                }
                None => diags.push(Diagnostic::warning(
                    "pack.bad-path",
                    format!("invalid start file path `{s}`"),
                )),
            }
        }
        let mut is_start = seen.clone();
        while let Some(path) = queue.pop_front() {
            let Some(text) = read(&path) else {
                if is_start.remove(&path) {
                    diags.push(Diagnostic::warning(
                        "pack.file-missing",
                        format!("cannot read `{path}`"),
                    ));
                }
                continue;
            };
            let mut includes = Vec::new();
            for (i, line) in lines(&text).into_iter().enumerate() {
                let Some(target) = parse_include_line(line) else {
                    continue;
                };
                let line_no = (i + 1) as u32;
                match normalize_pack_path(&path, target) {
                    Some(resolved) if resolved == path => diags.push(
                        Diagnostic::warning(
                            "pack.include-cycle",
                            format!("`{path}` includes itself"),
                        )
                        .at(SourceLocation::new(path.clone(), line_no)),
                    ),
                    Some(resolved) => {
                        if seen.insert(resolved.clone()) {
                            queue.push_back(resolved.clone());
                        }
                        includes.push((line_no, resolved));
                    }
                    None => diags.push(
                        Diagnostic::warning(
                            "pack.include-escape",
                            format!("#include \"{target}\" leaves the shaders directory"),
                        )
                        .at(SourceLocation::new(path.clone(), line_no)),
                    ),
                }
            }
            graph.files.insert(
                path.clone(),
                FileNode {
                    path,
                    text: Arc::from(text),
                    includes,
                },
            );
        }
        // Report includes of files that could not be read.
        for node in graph.files.values() {
            for (line, target) in &node.includes {
                if !graph.files.contains_key(target) {
                    diags.push(
                        Diagnostic::warning(
                            "pack.include-missing",
                            format!("included file `{target}` does not exist"),
                        )
                        .at(SourceLocation::new(node.path.clone(), *line)),
                    );
                }
            }
        }
        (graph, diags)
    }

    /// Weakly connected components (files linked by includes in either direction),
    /// each sorted, ordered by their first file.
    pub fn components(&self) -> Vec<Vec<String>> {
        let names: Vec<&String> = self.files.keys().collect();
        let index: BTreeMap<&str, usize> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect();
        let mut parent: Vec<usize> = (0..names.len()).collect();
        fn find(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }
        for (i, name) in names.iter().enumerate() {
            for (_, target) in &self.files[*name].includes {
                if let Some(&j) = index.get(target.as_str()) {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    if a != b {
                        parent[a.max(b)] = a.min(b);
                    }
                }
            }
        }
        let mut groups: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (i, name) in names.iter().enumerate() {
            let root = find(&mut parent, i);
            groups.entry(root).or_default().push((*name).clone());
        }
        groups.into_values().collect()
    }

    /// Files including `path` directly.
    pub fn includers_of(&self, path: &str) -> Vec<&str> {
        self.files
            .values()
            .filter(|n| n.includes.iter().any(|(_, t)| t == path))
            .map(|n| n.path.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn include_line_forms() {
        assert_eq!(
            parse_include_line("#include \"/lib/a.glsl\""),
            Some("/lib/a.glsl")
        );
        assert_eq!(
            parse_include_line("   #include   \"b.glsl\"  // comment"),
            Some("b.glsl")
        );
        assert_eq!(parse_include_line("#include <c.glsl>"), Some("c.glsl"));
        assert_eq!(parse_include_line("#include d.glsl"), Some("d.glsl"));
        assert_eq!(parse_include_line("#include\"e.glsl\""), Some("e.glsl"));
        assert_eq!(
            parse_include_line("#include \"unterminated"),
            Some("unterminated")
        );
        assert_eq!(parse_include_line("#includes x"), None);
        assert_eq!(parse_include_line("#include"), None);
        assert_eq!(parse_include_line("// #include \"x\""), None);
    }

    #[test]
    fn builds_graph_and_components() {
        let pack = ShaderPack::from_files(
            "g",
            [
                (
                    "composite.fsh",
                    "#include \"/lib/common.glsl\"\nvoid main(){}",
                ),
                (
                    "final.fsh",
                    "#include \"lib/common.glsl\"\n#include \"lib/missing.glsl\"",
                ),
                (
                    "lib/common.glsl",
                    "#include \"settings.glsl\"\n#include \"common.glsl\"",
                ),
                ("lib/settings.glsl", "#define X"),
                ("other.fsh", "#include \"/../escape.glsl\""),
            ],
        );
        let starts: Vec<String> = ["composite.fsh", "final.fsh", "other.fsh", "nope.fsh"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (g, d) = IncludeGraph::build(&pack, &starts);
        assert_eq!(
            g.files.keys().collect::<Vec<_>>(),
            vec![
                "composite.fsh",
                "final.fsh",
                "lib/common.glsl",
                "lib/settings.glsl",
                "other.fsh"
            ]
        );
        assert_eq!(
            g.files["lib/common.glsl"].includes,
            vec![(1, "lib/settings.glsl".to_string())]
        );
        let codes: BTreeSet<&str> = d.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(
            codes,
            [
                "pack.file-missing",
                "pack.include-cycle",
                "pack.include-escape",
                "pack.include-missing"
            ]
            .into_iter()
            .collect()
        );
        let comps = g.components();
        assert_eq!(
            comps,
            vec![
                vec![
                    "composite.fsh".to_string(),
                    "final.fsh".into(),
                    "lib/common.glsl".into(),
                    "lib/settings.glsl".into()
                ],
                vec!["other.fsh".to_string()],
            ]
        );
        assert_eq!(
            g.includers_of("lib/common.glsl"),
            vec!["composite.fsh", "final.fsh"]
        );
    }
}
