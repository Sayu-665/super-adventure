//! Integration tests over the real shader pack corpus.
//!
//! The corpus location is `$SB_CORPUS_DIR` or the session scratchpad default; every
//! test is skipped (passes with a note) when it is missing.

use sb_core::model::OptionKind;
use sb_core::{Severity, SourceProvider};
use sb_pack::options::{self, DiscoveredOptions, annotate};
use sb_pack::{
    EditedSources, MemVfs, OptionValues, ShaderPack, ZipVfs, idmap, properties, shaders_properties,
};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const DEFAULT_CORPUS: &str = "/tmp/claude-0/-home-user-super-adventure/14a9b258-2170-5e12-9e2c-1c1a40e3de07/scratchpad/corpus";

/// (directory name, minimum expected option count)
const PACKS: &[(&str, usize)] = &[
    ("photon", 300),
    ("Bliss-Shader", 300),
    ("ComplementaryReimagined", 300),
    ("Super-Duper-Vanilla", 100),
    ("glimmer-shaders", 80),
    ("spectrum", 100),
    ("Ominous-Shaderpack", 5),
    ("RethinkingVoxels", 300),
];

fn corpus_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("SB_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CORPUS));
    if dir.is_dir() {
        Some(dir)
    } else {
        eprintln!("corpus not found at {}; skipping", dir.display());
        None
    }
}

/// Every pack directory of the corpus: the main packs plus each tutorial pack of
/// MinecraftShaderProgramming.
fn pack_dirs(corpus: &Path) -> Vec<(PathBuf, usize)> {
    let mut out: Vec<(PathBuf, usize)> = PACKS
        .iter()
        .map(|(n, min)| (corpus.join(n), *min))
        .filter(|(p, _)| p.is_dir())
        .collect();
    if let Ok(entries) = std::fs::read_dir(corpus.join("MinecraftShaderProgramming")) {
        let mut tutorials: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("shaders").is_dir())
            .collect();
        tutorials.sort();
        out.extend(tutorials.into_iter().map(|p| (p, 0)));
    }
    out
}

fn open(dir: &Path) -> ShaderPack {
    let mut pack = ShaderPack::open(dir).unwrap_or_else(|e| panic!("open {}: {e}", dir.display()));
    if let Some(text) = pack.read_latin1("dimension.properties") {
        pack.set_dimension_map(idmap::parse_dimension_properties(&text));
    }
    pack
}

#[test]
fn programs_of_every_folder() {
    let Some(corpus) = corpus_dir() else { return };
    let dirs = pack_dirs(&corpus);
    assert!(!dirs.is_empty());
    for (dir, _) in dirs {
        let pack = open(&dir);
        let sets = pack.program_sets();
        assert!(!sets.is_empty(), "{}: no programs anywhere", pack.name());
        for set in &sets {
            assert!(!set.is_empty());
            for (name, p) in &set.programs {
                assert_eq!(&p.file_name(), name);
                assert!(
                    p.has_graphics() || !p.computes.is_empty() || !p.ignored_computes.is_empty(),
                    "{}: empty program {name}",
                    pack.name()
                );
                for f in p.files() {
                    assert!(
                        pack.exists(f),
                        "{}: listed file {f} does not exist",
                        pack.name()
                    );
                }
            }
            assert!(
                set.diagnostics
                    .iter()
                    .all(|d| d.severity != Severity::Error)
            );
        }
        // Folders with programs: a world folder exists iff the assignments mention it.
        let assigned = pack.dimension_assignments();
        for f in pack.world_folders() {
            assert!(
                assigned.contains_key(&f),
                "{}: {f} not assigned",
                pack.name()
            );
        }
    }
}

#[test]
fn world_folders_of_known_packs() {
    let Some(corpus) = corpus_dir() else { return };
    let expect: &[(&str, &[&str], &str)] = &[
        ("photon", &["world0", "world-1", "world1"], "world0"),
        (
            "ComplementaryReimagined",
            &["world0", "world-1", "world1"],
            "world0",
        ),
        ("spectrum", &["world0"], "world0"),
        ("Ominous-Shaderpack", &[], ""),
    ];
    for (name, folders, base) in expect {
        let dir = corpus.join(name);
        if !dir.is_dir() {
            continue;
        }
        let pack = open(&dir);
        assert_eq!(pack.world_folders(), *folders, "{name}");
        assert_eq!(pack.base_folder(), *base, "{name}");
    }
    let complementary = corpus.join("ComplementaryReimagined");
    if complementary.is_dir() {
        let pack = open(&complementary);
        assert_eq!(
            pack.dimension_assignments()["world-1"],
            vec!["minecraft:the_nether", "minecraft:nether"]
        );
    }
}

#[test]
fn shaders_properties_of_every_pack() {
    let Some(corpus) = corpus_dir() else { return };
    for (dir, _) in pack_dirs(&corpus) {
        let pack = open(&dir);
        let Some(text) = pack.read_latin1("shaders.properties") else {
            continue;
        };
        let (raw, d) = properties::parse_with_diagnostics(&text, "shaders.properties");
        assert!(d.is_empty(), "{}: {d:?}", pack.name());
        // Without a preprocessor at hand, feed the raw entries as the preprocessed ones.
        let (props, diags) = shaders_properties::parse(&raw, &raw);
        let errors: Vec<_> = diags.errors().collect();
        assert!(errors.is_empty(), "{}: {errors:?}", pack.name());
        // Everything maps to the model without panicking.
        let _ = props.settings();
        let _ = props.custom_textures_model();
        let (_, tex_diags) = props.resolve_custom_textures(&pack);
        let missing: Vec<_> = tex_diags
            .iter()
            .filter(|d| d.code == "props.texture-missing")
            .collect();
        assert!(missing.is_empty(), "{}: {missing:?}", pack.name());
        let _ = props.screens_model();
        for (folder, prog) in props
            .program_enabled
            .keys()
            .filter_map(|k| k.rsplit_once('/'))
        {
            assert!(!folder.is_empty() && !prog.is_empty());
        }
    }

    // Spot checks on packs with rich properties.
    let complementary = corpus.join("ComplementaryReimagined");
    if complementary.is_dir() {
        let pack = open(&complementary);
        let raw = properties::parse(&pack.read_latin1("shaders.properties").unwrap());
        let (props, _) = shaders_properties::parse(&raw, &raw);
        assert!(props.custom_uniforms.len() >= 10);
        assert!(props.profiles.len() >= 5);
        assert!(props.main_screen.as_ref().is_some_and(|s| !s.is_empty()));
        assert!(props.screens.len() >= 20);
        assert!(!props.images.is_empty());
        assert!(!props.buffer_objects.is_empty());
        assert!(!props.features_required.is_empty() || !props.features_optional.is_empty());
    }
    // `program.<path>.enabled` keys name exact program paths: packs with world folders
    // list each folder, and a bare name only applies to root programs.
    let bliss = corpus.join("Bliss-Shader");
    if bliss.is_dir() {
        let pack = open(&bliss);
        let raw = properties::parse(&pack.read_latin1("shaders.properties").unwrap());
        let (props, _) = shaders_properties::parse(&raw, &raw);
        assert!(props.program_enabled.contains_key("composite5"));
        assert!(
            pack.program_set("").is_empty(),
            "Bliss has no root programs"
        );
        assert_eq!(props.program_enabled_expr("world0", "composite5"), None);
        for folder in ["world0", "world-1", "world1"] {
            assert!(props.program_enabled_expr(folder, "deferred2").is_some());
        }
    }
    let complementary = corpus.join("ComplementaryReimagined");
    if complementary.is_dir() {
        let pack = open(&complementary);
        let raw = properties::parse(&pack.read_latin1("shaders.properties").unwrap());
        let (props, _) = shaders_properties::parse(&raw, &raw);
        assert_eq!(
            props.program_enabled_expr("world-1", "shadow"),
            Some("false")
        );
        assert_eq!(props.program_enabled_expr("", "shadow"), None);
    }
    let photon = corpus.join("photon");
    if photon.is_dir() {
        let pack = open(&photon);
        let raw = properties::parse(&pack.read_latin1("shaders.properties").unwrap());
        let (props, _) = shaders_properties::parse(&raw, &raw);
        assert!(props.custom_uniforms.len() >= 30);
        assert!(
            props
                .program_enabled
                .keys()
                .any(|k| k.ends_with("deferred4_a")),
            "compute programs can be toggled"
        );
    }
}

fn discover(pack: &ShaderPack) -> DiscoveredOptions {
    let starts = pack.option_start_files();
    let (opts, diags) = options::discover(pack, &starts);
    let bad: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == Severity::Error || d.code == "pack.include-missing")
        .collect();
    assert!(bad.is_empty(), "{}: {bad:?}", pack.name());
    opts
}

#[test]
fn start_files_come_from_the_folders_iris_loads() {
    let Some(corpus) = corpus_dir() else { return };
    for (dir, _) in pack_dirs(&corpus) {
        let pack = open(&dir);
        let folders = pack.program_folders();
        assert_eq!(folders[0], "", "{}", pack.name());
        let assigned = pack.dimension_assignments();
        assert_eq!(folders.len(), assigned.len() + 1, "{}", pack.name());
        for f in pack.option_start_files() {
            let folder = f.rsplit_once('/').map_or("", |(d, _)| d);
            assert!(
                folders.iter().any(|x| x == folder),
                "{}: start file {f} outside the program folders {folders:?}",
                pack.name()
            );
            assert!(pack.exists(&f), "{}: {f}", pack.name());
        }
    }
}

#[test]
fn option_discovery_counts() {
    let Some(corpus) = corpus_dir() else { return };
    for (dir, min) in pack_dirs(&corpus) {
        let pack = open(&dir);
        let opts = discover(&pack);
        assert!(
            opts.options.len() >= min,
            "{}: only {} options (expected >= {min})",
            pack.name(),
            opts.options.len()
        );
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for o in &opts.options {
            *kinds.entry(format!("{:?}", o.kind)).or_default() += 1;
            assert_eq!(o.value, o.default);
            if options::is_boolean_option(o) {
                assert!(
                    o.default == "true" || o.default == "false",
                    "{}: {}",
                    pack.name(),
                    o.name
                );
            } else {
                assert!(
                    o.allowed.contains(&o.default),
                    "{}: {} default missing from allowed",
                    pack.name(),
                    o.name
                );
            }
            assert!(
                opts.occurrences_in(&o.file)
                    .is_some_and(|occ| occ.contains_key(&o.line)),
                "{}: {}",
                pack.name(),
                o.name
            );
            assert!(!opts.ambiguous.contains(&o.name));
        }
        eprintln!(
            "{}: {} options {:?}, {} ambiguous, {} files",
            pack.name(),
            opts.options.len(),
            kinds,
            opts.ambiguous.len(),
            opts.files.len()
        );
        if min >= 100 {
            assert!(
                kinds.get("BooleanDefine").copied().unwrap_or(0) >= 20,
                "{}",
                pack.name()
            );
            assert!(
                kinds.get("ValueDefine").copied().unwrap_or(0) >= 20,
                "{}",
                pack.name()
            );
        }
    }
}

#[test]
fn known_options_of_complementary() {
    let Some(corpus) = corpus_dir() else { return };
    let dir = corpus.join("ComplementaryReimagined");
    if !dir.is_dir() {
        return;
    }
    let pack = open(&dir);
    let opts = discover(&pack);
    // Options its shaders.properties profiles and screens refer to.
    for name in [
        "SHADOW_QUALITY",
        "WATER_REFLECT_QUALITY",
        "BLOOM_ENABLED",
        "shadowDistance",
    ] {
        assert!(opts.option(name).is_some(), "missing option {name}");
    }
    assert_eq!(
        opts.option("shadowDistance").unwrap().kind,
        OptionKind::Const
    );
    // Profiles resolve and the defaults match one of them.
    let raw = properties::parse(&pack.read_latin1("shaders.properties").unwrap());
    let (props, _) = shaders_properties::parse(&raw, &raw);
    let (profiles, diags) = options::resolve_profiles(&props.profiles, &opts);
    assert!(diags.is_empty(), "{diags:?}");
    assert!(options::detect_profile(&profiles, &opts, &OptionValues::new()).is_some());
    // Selecting another profile changes values and is detected.
    let low = &profiles["LOW"];
    let mut values = OptionValues::new();
    values.apply_profile(low, &opts);
    assert_eq!(
        options::detect_profile(&profiles, &opts, &values).map(|p| p.name.as_str()),
        Some("LOW")
    );
    let (model, _) = options::build_options_model(&props, &opts, &values, pack.lang("en_us"));
    assert_eq!(model.current_profile.as_deref(), Some("LOW"));
    assert!(model.lang.len() > 100);
}

/// A non-default value for every option that survives re-parsing.
fn non_default_values(opts: &DiscoveredOptions) -> OptionValues {
    let mut v = OptionValues::new();
    for o in &opts.options {
        if options::is_boolean_option(o) {
            v.set(
                o.name.clone(),
                if o.default == "true" { "false" } else { "true" },
            );
        } else if let Some(alt) = o.allowed.iter().find(|a| {
            *a != &o.default
                && annotate::annotate_line(&format!("#define X {a} // [{a}]")).is_some()
        }) {
            v.set(o.name.clone(), alt.clone());
        }
    }
    v
}

#[test]
fn edited_sources_round_trip() {
    let Some(corpus) = corpus_dir() else { return };
    for (dir, _) in pack_dirs(&corpus) {
        let pack = Arc::new(open(&dir));
        let starts = pack.option_start_files();
        let opts = discover(&pack);

        // Defaults: every file comes back byte-identical.
        let unchanged = EditedSources::new(pack.clone(), opts.clone(), OptionValues::new());
        assert!(unchanged.edited_files().is_empty());
        for f in &opts.files {
            assert_eq!(
                unchanged.read(f).as_deref(),
                pack.read_text(f).as_deref(),
                "{}: {f}",
                pack.name()
            );
        }

        // Non-default values: line structure is preserved and re-discovery sees them.
        let values = non_default_values(&opts);
        let edited = EditedSources::new(pack.clone(), opts.clone(), values.clone());
        let mut mem = MemVfs::new();
        for f in &opts.files {
            let original = pack.read_text(f).unwrap();
            let text = edited.read(f).unwrap();
            assert_eq!(
                text.lines().count(),
                original.lines().count(),
                "{}: {f}",
                pack.name()
            );
            mem.insert(f, text.as_bytes());
        }
        let edited_pack = ShaderPack::from_vfs("edited", Box::new(mem));
        let (again, _) = options::discover(&edited_pack, &starts);
        assert_eq!(
            again.names().collect::<Vec<_>>().len(),
            opts.options.len(),
            "{}",
            pack.name()
        );
        for o in &opts.options {
            let now = again
                .option(&o.name)
                .unwrap_or_else(|| panic!("{}: option {} lost after editing", pack.name(), o.name));
            let expected = opts.effective_value(&o.name, &values).unwrap();
            assert_eq!(now.default, expected, "{}: option {}", pack.name(), o.name);
        }
        eprintln!(
            "{}: {} options edited in {} files",
            pack.name(),
            values.len(),
            edited.edited_files().len()
        );
    }
}

#[test]
fn id_maps_of_every_pack() {
    let Some(corpus) = corpus_dir() else { return };
    let mut total_blocks = 0;
    for (dir, _) in pack_dirs(&corpus) {
        let pack = open(&dir);
        let block = pack.read_latin1("block.properties");
        let item = pack.read_latin1("item.properties");
        let entity = pack.read_latin1("entity.properties");
        let dimension = pack.read_latin1("dimension.properties");
        let (maps, diags) = idmap::build_id_maps(
            block.as_deref(),
            item.as_deref(),
            entity.as_deref(),
            dimension.as_deref(),
        );
        assert!(!diags.has_errors(), "{}: {diags:?}", pack.name());
        if block.is_some() {
            assert!(!maps.blocks.is_empty(), "{}", pack.name());
            for entries in maps.blocks.values() {
                assert!(
                    entries
                        .iter()
                        .all(|e| !e.is_empty() && !e.contains(char::is_whitespace))
                );
            }
        }
        total_blocks += maps.blocks.len();
    }
    assert!(
        total_blocks > 500,
        "only {total_blocks} block ids in the whole corpus"
    );

    // Complementary's raw block.properties has `#if` branches; both are parsed and the
    // later definition of a duplicated key wins.
    let dir = corpus.join("ComplementaryReimagined");
    if dir.is_dir() {
        let pack = open(&dir);
        let (blocks, layers, _) =
            idmap::parse_block_properties(&pack.read_latin1("block.properties").unwrap());
        assert!(blocks.len() > 300);
        assert!(blocks[&5008].contains(&"chest".to_string()));
        assert!(layers.contains_key("translucent"));
    }
}

#[test]
fn lang_files() {
    let Some(corpus) = corpus_dir() else { return };
    let dir = corpus.join("Super-Duper-Vanilla");
    if !dir.is_dir() {
        return;
    }
    let pack = open(&dir);
    let langs = pack.languages();
    assert!(
        langs.contains(&"en_us".to_string()) && langs.contains(&"zh_cn".to_string()),
        "{langs:?}"
    );
    let en = pack.lang("en_US");
    let zh = pack.lang("zh_cn");
    assert!(en.len() > 50);
    assert!(zh.len() >= en.len(), "fallback fills missing keys");
    assert!(
        zh.values()
            .any(|v| v.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))),
        "UTF-8 decoded"
    );
}

#[test]
fn real_pack_as_zip_matches_directory() {
    let Some(corpus) = corpus_dir() else { return };
    let dir = corpus.join("spectrum");
    if !dir.is_dir() {
        return;
    }
    let dir_pack = open(&dir);
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for f in dir_pack.all_files() {
        w.start_file(format!("spectrum-main/shaders/{f}"), opts)
            .unwrap();
        w.write_all(&dir_pack.read_bytes(&f).unwrap()).unwrap();
    }
    let bytes = w.finish().unwrap().into_inner();
    let zip_pack = ShaderPack::from_vfs("spectrum", Box::new(ZipVfs::from_bytes(bytes).unwrap()));
    assert_eq!(zip_pack.content_hash(), dir_pack.content_hash());
    assert_eq!(zip_pack.option_start_files(), dir_pack.option_start_files());
    assert_eq!(discover(&zip_pack).options, discover(&dir_pack).options);
    assert_eq!(
        zip_pack.vfs().all_files().len(),
        dir_pack.vfs().all_files().len()
    );
}
