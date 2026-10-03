//! Distant Horizons helpers: the representative block of each DH material, used to feed
//! `mc_Entity.x` of synthesized DH programs with the pack's own `block.properties` ids.

use indexmap::IndexMap;
use sb_core::model::IdMaps;

/// DH material (`DH_BLOCK_*` value) → representative vanilla block. `UNKNOWN` (0) and
/// `AIR` (14) have none.
pub const DH_MATERIAL_BLOCKS: [(u32, &str); 14] = [
    (1, "oak_leaves"),
    (2, "stone"),
    (3, "oak_log"),
    (4, "iron_block"),
    (5, "dirt"),
    (6, "lava"),
    (7, "deepslate"),
    (8, "snow_block"),
    (9, "sand"),
    (10, "terracotta"),
    (11, "netherrack"),
    (12, "water"),
    (13, "grass_block"),
    (15, "glowstone"),
];

/// Whether a `block.properties` entry names `block` (a vanilla block path): accepts
/// `block`, `minecraft:block`, and either with block-state filters
/// (`minecraft:grass_block:snowy=false`). Tags (`%...`) and other namespaces never match.
pub fn entry_matches_block(entry: &str, block: &str) -> bool {
    if entry.starts_with('%') {
        return false;
    }
    let rest = entry.strip_prefix("minecraft:").unwrap_or(entry);
    let id = rest.split(':').next().unwrap_or("");
    // `other_mod:block` has a namespace that is not minecraft: the first segment is the
    // namespace and the second segment a path; it never matches a vanilla block.
    if entry.split(':').nth(1).is_some_and(|second| !second.contains('=')) && !entry.starts_with("minecraft:") {
        return false;
    }
    id == block
}

/// The pack block id whose `block.properties` entry contains `block` (first id in file
/// order), if any.
pub fn block_id_of(maps: &IdMaps, block: &str) -> Option<i32> {
    maps.blocks.iter().find(|(_, entries)| entries.iter().any(|e| entry_matches_block(e, block))).map(|(id, _)| *id)
}

/// `SB_DH_BLOCK_ID_<n>` profile constants for synthesized DH programs: every DH material
/// whose representative block has a pack id. Missing materials are left out (the profile
/// code then uses -1, "no id", like Iris for unmapped blocks).
pub fn dh_block_constants(maps: &IdMaps) -> IndexMap<String, i64> {
    let mut out = IndexMap::new();
    for (material, block) in DH_MATERIAL_BLOCKS {
        if let Some(id) = block_id_of(maps, block) {
            out.insert(format!("SB_DH_BLOCK_ID_{material}"), i64::from(id));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_forms() {
        assert!(entry_matches_block("oak_leaves", "oak_leaves"));
        assert!(entry_matches_block("minecraft:oak_leaves", "oak_leaves"));
        assert!(entry_matches_block("minecraft:grass_block:snowy=false", "grass_block"));
        assert!(entry_matches_block("grass_block:snowy=false", "grass_block"));
        assert!(entry_matches_block("minecraft:water:level=0:falling=false", "water"));
        assert!(!entry_matches_block("%minecraft:leaves", "oak_leaves"));
        assert!(!entry_matches_block("minecraft:dark_oak_leaves", "oak_leaves"));
        assert!(!entry_matches_block("othermod:stone", "stone"));
        assert!(!entry_matches_block("stone_bricks", "stone"));
    }

    #[test]
    fn material_mapping() {
        let mut maps = IdMaps::default();
        maps.blocks.insert(10001, vec!["minecraft:stone".into(), "granite".into()]);
        maps.blocks.insert(10002, vec!["oak_leaves".into(), "birch_leaves".into()]);
        maps.blocks.insert(10003, vec!["minecraft:grass_block:snowy=false".into()]);
        maps.blocks.insert(10004, vec!["%minecraft:logs".into()]);
        maps.blocks.insert(10005, vec!["stone".into()]); // later duplicate: first id wins
        maps.blocks.insert(10006, vec!["water".into(), "flowing_water".into()]);
        let c = dh_block_constants(&maps);
        assert_eq!(c.get("SB_DH_BLOCK_ID_1"), Some(&10002));
        assert_eq!(c.get("SB_DH_BLOCK_ID_2"), Some(&10001));
        assert_eq!(c.get("SB_DH_BLOCK_ID_12"), Some(&10006));
        assert_eq!(c.get("SB_DH_BLOCK_ID_13"), Some(&10003));
        assert_eq!(c.get("SB_DH_BLOCK_ID_3"), None); // only a tag names logs
        assert_eq!(c.len(), 4);
    }
}
