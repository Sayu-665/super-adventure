//! ID map files: `block.properties`, `item.properties`, `entity.properties` and
//! `dimension.properties`.
//!
//! The caller preprocesses the files (options + environment macros) before parsing.
//! On top of `.properties` syntax this module repairs what preprocessing commonly
//! breaks, as Iris does in `IdMap.loadProperties`:
//!
//! * a `\` continuation followed by blank lines (left behind by removed `#if` lines)
//!   or `#` comment lines continues on the next content line;
//! * a continuation is **not** joined with a following line that starts a new entry
//!   (`block.<n>=`, `layer.<x>=`, `item.<n>=`, `entity.<n>=`, `dimension.<x>=`),
//!   so a stray trailing backslash on the last entry of a block does not swallow the
//!   next key. (Iris handles this with a regex that also drops one character of the
//!   previous entry; ShaderBridge does not reproduce that bug.)
//! * continued lines are joined with a space, so `stone\` followed by `dirt` yields two
//!   entries (Java's `Properties` would glue them into `stonedirt`; no corpus pack
//!   relies on that, every continuation there has whitespace before the `\`).
//!
//! Entries are kept as raw strings (`minecraft:wheat:age=7`, `%minecraft:logs`,
//! `stone`); a namespace-less id means `minecraft:`.
//!
//! Diagnostic line numbers refer to the text passed in. Iris-style preprocessing drops
//! blank lines, so map them back with [`properties::remap_diagnostic_lines`] when the
//! preprocessor provides a line map.

use crate::properties::{self, PropEntry};
use crate::text::lines;
use indexmap::IndexMap;
use sb_core::model::IdMaps;
use sb_core::{Diagnostic, Diagnostics, SourceLocation};

/// Parsed `block.properties`: (block id -> entries, render layer -> blocks, diagnostics).
pub type BlockProperties = (
    IndexMap<i32, Vec<String>>,
    IndexMap<String, Vec<String>>,
    Diagnostics,
);

/// Render layers accepted by `layer.<name>=`.
pub const LAYERS: [&str; 4] = ["solid", "cutout", "cutout_mipped", "translucent"];

const ENTRY_PREFIXES: [&str; 5] = ["block.", "layer.", "item.", "entity.", "dimension."];

/// Parse `block.properties`: `block.<id>=<entries>` and `layer.<layer>=<blocks>`.
pub fn parse_block_properties(text: &str) -> BlockProperties {
    let file = "block.properties";
    let mut diags = Diagnostics::new();
    let mut blocks = IndexMap::new();
    let mut layers: IndexMap<String, Vec<String>> = IndexMap::new();
    for e in parse_entries(text, file, &mut diags) {
        if let Some(id) = e.key.strip_prefix("block.") {
            let Some(id) = parse_id(id, &e, file, &mut diags) else {
                continue;
            };
            let entries: Vec<String> = e
                .value
                .split_whitespace()
                .filter(|part| validate_block_entry(part, &e, file, &mut diags))
                .map(str::to_string)
                .collect();
            blocks.insert(id, entries);
        } else if let Some(layer) = e.key.strip_prefix("layer.") {
            if !LAYERS.contains(&layer) {
                diags.push(
                    Diagnostic::warning(
                        "idmap.bad-layer",
                        format!(
                            "unknown render layer `{layer}` (expected one of {})",
                            LAYERS.join(", ")
                        ),
                    )
                    .at(SourceLocation::new(file, e.line)),
                );
                continue;
            }
            let mut entries = Vec::new();
            for part in e.value.split_whitespace() {
                if part.starts_with('%') {
                    diags.push(
                        Diagnostic::warning(
                            "idmap.layer-tag",
                            format!(
                                "block tags cannot be used in render layer overrides: `{part}`"
                            ),
                        )
                        .at(SourceLocation::new(file, e.line)),
                    );
                    continue;
                }
                entries.push(part.to_string());
            }
            layers.insert(layer.to_string(), entries);
        } else {
            unexpected_key(&e, file, "block.<id> or layer.<layer>", &mut diags);
        }
    }
    (blocks, layers, diags)
}

/// Parse `item.properties`: `item.<id>=<item ids>`.
pub fn parse_item_properties(text: &str) -> (IndexMap<i32, Vec<String>>, Diagnostics) {
    parse_simple_id_map(text, "item.", "item.properties")
}

/// Parse `entity.properties`: `entity.<id>=<entity ids>`.
pub fn parse_entity_properties(text: &str) -> (IndexMap<i32, Vec<String>>, Diagnostics) {
    parse_simple_id_map(text, "entity.", "entity.properties")
}

/// Parse `dimension.properties`: `dimension.<folder>=<dimension ids or *>`.
pub fn parse_dimension_properties(text: &str) -> IndexMap<String, Vec<String>> {
    parse_dimension_properties_with_diagnostics(text).0
}

/// [`parse_dimension_properties`] with diagnostics.
pub fn parse_dimension_properties_with_diagnostics(
    text: &str,
) -> (IndexMap<String, Vec<String>>, Diagnostics) {
    let file = "dimension.properties";
    let mut diags = Diagnostics::new();
    let mut map = IndexMap::new();
    for e in parse_entries(text, file, &mut diags) {
        match e.key.strip_prefix("dimension.") {
            Some(folder) if !folder.is_empty() => {
                map.insert(
                    folder.to_string(),
                    e.value.split_whitespace().map(str::to_string).collect(),
                );
            }
            _ => unexpected_key(&e, file, "dimension.<folder>", &mut diags),
        }
    }
    (map, diags)
}

/// Parse all four files (each optional, already preprocessed) into the model's
/// [`IdMaps`].
pub fn build_id_maps(
    block: Option<&str>,
    item: Option<&str>,
    entity: Option<&str>,
    dimension: Option<&str>,
) -> (IdMaps, Diagnostics) {
    let mut maps = IdMaps::default();
    let mut diags = Diagnostics::new();
    if let Some(t) = block {
        let (b, l, d) = parse_block_properties(t);
        maps.blocks = b;
        maps.layers = l;
        diags.extend(d);
    }
    if let Some(t) = item {
        let (m, d) = parse_item_properties(t);
        maps.items = m;
        diags.extend(d);
    }
    if let Some(t) = entity {
        let (m, d) = parse_entity_properties(t);
        maps.entities = m;
        diags.extend(d);
    }
    if let Some(t) = dimension {
        let (m, d) = parse_dimension_properties_with_diagnostics(t);
        maps.dimensions = m;
        diags.extend(d);
    }
    (maps, diags)
}

fn parse_simple_id_map(
    text: &str,
    prefix: &str,
    file: &str,
) -> (IndexMap<i32, Vec<String>>, Diagnostics) {
    let mut diags = Diagnostics::new();
    let mut map = IndexMap::new();
    for e in parse_entries(text, file, &mut diags) {
        let Some(id) = e.key.strip_prefix(prefix) else {
            unexpected_key(&e, file, &format!("{prefix}<id>"), &mut diags);
            continue;
        };
        let Some(id) = parse_id(id, &e, file, &mut diags) else {
            continue;
        };
        let mut entries = Vec::new();
        for part in e.value.split_whitespace() {
            if part.contains('=') {
                diags.push(
                    Diagnostic::warning(
                        "idmap.state-unsupported",
                        format!("state properties are not supported in {file}: `{part}`"),
                    )
                    .at(SourceLocation::new(file, e.line)),
                );
                continue;
            }
            entries.push(part.to_string());
        }
        map.insert(id, entries);
    }
    (map, diags)
}

fn parse_entries(text: &str, file: &str, diags: &mut Diagnostics) -> Vec<PropEntry> {
    let joined = join_continuations(text);
    let (entries, d) = properties::parse_with_diagnostics(&joined, file);
    diags.extend(d);
    entries
}

fn parse_id(id: &str, e: &PropEntry, file: &str, diags: &mut Diagnostics) -> Option<i32> {
    match id.trim().parse::<i32>() {
        Ok(v) => Some(v),
        Err(_) => {
            diags.push(
                Diagnostic::warning(
                    "idmap.bad-id",
                    format!("invalid numeric id in key `{}`", e.key),
                )
                .at(SourceLocation::new(file, e.line)),
            );
            None
        }
    }
}

fn unexpected_key(e: &PropEntry, file: &str, expected: &str, diags: &mut Diagnostics) {
    let hint = if ENTRY_PREFIXES.iter().any(|p| e.key.starts_with(p)) {
        format!("this kind of entry does not belong in {file} and is ignored")
    } else {
        "a `\\` line continuation may be missing on the previous line".to_string()
    };
    diags.push(
        Diagnostic::warning(
            "idmap.unexpected-key",
            format!("unexpected key `{}` (expected {expected}); {hint}", e.key),
        )
        .at(SourceLocation::new(file, e.line)),
    );
}

/// Check the state-filter syntax of a block entry (`[ns:]id[:k=v]...` or `%tag...`).
/// Malformed filters only produce a warning; the entry is kept.
fn validate_block_entry(part: &str, e: &PropEntry, file: &str, diags: &mut Diagnostics) -> bool {
    let body = part.strip_prefix('%').unwrap_or(part);
    if body.is_empty() {
        diags.push(
            Diagnostic::warning("idmap.bad-entry", format!("empty block entry `{part}`"))
                .at(SourceLocation::new(file, e.line)),
        );
        return false;
    }
    let segments: Vec<&str> = body.split(':').collect();
    let states_start = match segments.len() {
        1 => return true,
        2 if !segments[1].contains('=') => return true,
        _ if segments[1].contains('=') => 1,
        _ => 2,
    };
    for s in &segments[states_start..] {
        if s.split('=').count() != 2 || s.starts_with('=') || s.ends_with('=') {
            diags.push(
                Diagnostic::warning(
                    "idmap.bad-state",
                    format!("block state filter `{s}` in `{part}` is not of the form key=value[,value...]; it is ignored"),
                )
                .at(SourceLocation::new(file, e.line)),
            );
        }
    }
    true
}

fn odd_trailing_backslashes(line: &str) -> bool {
    line.bytes().rev().take_while(|&b| b == b'\\').count() % 2 == 1
}

fn starts_new_entry(line: &str) -> bool {
    let t = line.trim_start();
    ENTRY_PREFIXES.iter().any(|p| {
        t.strip_prefix(p).is_some_and(|rest| {
            let key_end = rest
                .find(|c: char| c == '=' || c == ':' || c.is_whitespace())
                .unwrap_or(rest.len());
            let after = rest[key_end..].trim_start();
            key_end > 0 && (after.starts_with('=') || after.starts_with(':'))
        })
    })
}

fn is_skippable(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#') || t.starts_with('!')
}

/// Join `\` continuations across blank/comment lines, but never into a line that
/// starts a new entry. Consumed lines are replaced by empty lines so line numbers
/// are preserved.
fn join_continuations(text: &str) -> String {
    let src = lines(text);
    let mut out: Vec<String> = Vec::with_capacity(src.len());
    let mut i = 0;
    while i < src.len() {
        let line = src[i];
        if is_skippable(line) || !odd_trailing_backslashes(line.trim_end()) {
            out.push(line.to_string());
            i += 1;
            continue;
        }
        let mut cur = line.trim_end().to_string();
        let mut consumed = 0;
        loop {
            if !odd_trailing_backslashes(&cur) {
                break;
            }
            let mut j = i + consumed + 1;
            while j < src.len() && is_skippable(src[j]) {
                j += 1;
            }
            cur.pop(); // the continuation backslash
            if j >= src.len() || starts_new_entry(src[j]) {
                break;
            }
            cur.push(' ');
            cur.push_str(src[j].trim());
            consumed = j - i;
        }
        out.push(cur);
        out.extend(std::iter::repeat_n(String::new(), consumed));
        i += consumed + 1;
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn blocks_and_layers() {
        let text = "\
# comment
block.10=minecraft:stone dirt \\
    %minecraft:logs minecraft:wheat:age=7
block.11 = oak_leaves
layer.translucent=glass_pane %minecraft:glass
layer.cutout=terrestria:hemlock_leaves
layer.bogus=x
";
        let (blocks, layers, diags) = parse_block_properties(text);
        assert_eq!(
            blocks[&10],
            vec![
                "minecraft:stone",
                "dirt",
                "%minecraft:logs",
                "minecraft:wheat:age=7"
            ]
        );
        assert_eq!(blocks[&11], vec!["oak_leaves"]);
        assert_eq!(layers["translucent"], vec!["glass_pane"]);
        assert_eq!(layers["cutout"], vec!["terrestria:hemlock_leaves"]);
        assert!(!layers.contains_key("bogus"));
        let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes, vec!["idmap.layer-tag", "idmap.bad-layer"]);
    }

    #[test]
    fn continuation_across_blank_lines_left_by_preprocessing() {
        let text = "block.10=a \\\n\n\n   b \\\n\nc\nblock.11=d\n";
        let (blocks, _, diags) = parse_block_properties(text);
        assert_eq!(blocks[&10], vec!["a", "b", "c"]);
        assert_eq!(blocks[&11], vec!["d"]);
        assert!(diags.is_empty());
    }

    #[test]
    fn stray_backslash_does_not_swallow_next_key() {
        let text = "block.10=a b \\\n\nblock.11=c \\\nblock.12=d";
        let (blocks, _, diags) = parse_block_properties(text);
        assert_eq!(blocks[&10], vec!["a", "b"]);
        assert_eq!(blocks[&11], vec!["c"]);
        assert_eq!(blocks[&12], vec!["d"]);
        assert!(diags.is_empty());
    }

    #[test]
    fn comment_lines_inside_continuations_are_skipped() {
        let text = "block.10=a \\\n# disabled branch\n  b\n";
        let (blocks, _, _) = parse_block_properties(text);
        assert_eq!(blocks[&10], vec!["a", "b"]);
    }

    #[test]
    fn line_numbers_survive_joining() {
        let text = "block.1=a \\\n\n b\nbogus=1\n";
        let (_, _, diags) = parse_block_properties(text);
        let d = diags.iter().next().unwrap();
        assert_eq!(d.code, "idmap.unexpected-key");
        assert_eq!(d.location.as_ref().unwrap().line, 4);
    }

    #[test]
    fn bad_ids_and_states() {
        let (blocks, _, diags) = parse_block_properties(
            "block.x=stone\nblock.5=lantern:hanging wheat:age=7:bad\nblock.-1=air",
        );
        assert!(!blocks.contains_key(&0));
        assert_eq!(blocks[&5], vec!["lantern:hanging", "wheat:age=7:bad"]);
        assert_eq!(blocks[&-1], vec!["air"]);
        let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes, vec!["idmap.bad-id", "idmap.bad-state"]);
    }

    #[test]
    fn empty_value_is_an_empty_list() {
        let (blocks, _, _) = parse_block_properties("block.5000=");
        assert_eq!(blocks[&5000], Vec::<String>::new());
    }

    #[test]
    fn items_and_entities() {
        let (items, d) =
            parse_item_properties("item.100=minecraft:torch lantern foo:bar=1\nitem.101=trim_gold");
        assert_eq!(items[&100], vec!["minecraft:torch", "lantern"]);
        assert_eq!(items[&101], vec!["trim_gold"]);
        assert_eq!(d.len(), 1);
        let (_, d) = parse_item_properties("block.1=stone");
        assert!(
            d.iter()
                .next()
                .unwrap()
                .message
                .contains("does not belong in item.properties")
        );
        let (entities, d) = parse_entity_properties(
            "entity.50=zombie minecraft:skeleton\nentity.51=minecraft:name_tag",
        );
        assert_eq!(entities[&50], vec!["zombie", "minecraft:skeleton"]);
        assert_eq!(entities[&51], vec!["minecraft:name_tag"]);
        assert!(d.is_empty());
    }

    #[test]
    fn dimensions() {
        let text = "dimension.world0 = minecraft:overworld *\ndimension.world-1=minecraft:the_nether\ndimension.worldx =\n";
        let map = parse_dimension_properties(text);
        assert_eq!(map["world0"], vec!["minecraft:overworld", "*"]);
        assert_eq!(map["world-1"], vec!["minecraft:the_nether"]);
        assert!(map["worldx"].is_empty());
    }

    #[test]
    fn build_all() {
        let (maps, diags) = build_id_maps(
            Some("block.1=stone\nlayer.solid=glass"),
            Some("item.2=torch"),
            Some("entity.3=pig"),
            Some("dimension.world1=minecraft:the_end"),
        );
        assert_eq!(maps.blocks[&1], vec!["stone"]);
        assert_eq!(maps.layers["solid"], vec!["glass"]);
        assert_eq!(maps.items[&2], vec!["torch"]);
        assert_eq!(maps.entities[&3], vec!["pig"]);
        assert_eq!(maps.dimensions["world1"], vec!["minecraft:the_end"]);
        assert!(diags.is_empty());
    }
}
