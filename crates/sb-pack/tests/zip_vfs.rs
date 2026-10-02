//! ZipVfs: shaders-root detection, reading, case fallback, and parity with DirVfs /
//! MemVfs on the same fixture pack.

use sb_core::SourceProvider;
use sb_pack::{EditedSources, MemVfs, OptionValues, PackError, ShaderPack, Vfs, ZipVfs, options};
use std::io::{Cursor, Write};
use std::sync::Arc;
use zip::write::SimpleFileOptions;

/// The fixture pack, as (path relative to `shaders/`, contents).
const FIXTURE: &[(&str, &str)] = &[
    (
        "shaders.properties",
        "sun=false\nscreen=SHADOWS QUALITY\nprogram.world0/composite1.enabled=SHADOWS\n",
    ),
    (
        "composite.vsh",
        "#version 120\nvoid main() { gl_Position = ftransform(); }\n",
    ),
    (
        "composite.fsh",
        "#version 120\n#include \"/lib/Settings.glsl\"\nvoid main() {}\n",
    ),
    (
        "lib/settings.glsl",
        "#define SHADOWS // Shadows\n#define QUALITY 2 // [1 2 3]\n#ifdef SHADOWS\n#endif\n",
    ),
    (
        "world0/gbuffers_terrain.fsh",
        "#include \"/lib/settings.glsl\"\nvoid main() {}\n",
    ),
    ("world0/composite1.fsh", "void main() {}\n"),
    ("world0/composite1_a.csh", "void main() {}\n"),
    ("lang/en_US.lang", "option.SHADOWS=Shadows\n"),
    ("block.properties", "block.10=stone dirt\n"),
];

fn zip_bytes(prefix: &str, files: &[(&str, &str)], extra_dirs: &[&str]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default();
    for d in extra_dirs {
        w.add_directory(*d, opts).unwrap();
    }
    for (p, c) in files {
        w.start_file(format!("{prefix}{p}"), opts).unwrap();
        w.write_all(c.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

#[test]
fn nested_root_is_detected() {
    let mut files: Vec<(&str, &str)> = FIXTURE.to_vec();
    files.push(("../README.md", "outside the shaders root"));
    let bytes = zip_bytes(
        "MyPack-1.0/shaders/",
        &files,
        &["MyPack-1.0/", "MyPack-1.0/shaders/empty/"],
    );
    let vfs = ZipVfs::from_bytes(bytes).unwrap();
    assert_eq!(vfs.root_prefix(), "MyPack-1.0/shaders/");
    assert_eq!(
        vfs.read("composite.fsh").as_deref(),
        Some(FIXTURE[2].1.as_bytes())
    );
    assert!(
        vfs.read("README.md").is_none(),
        "files outside shaders/ are not visible"
    );
    assert_eq!(
        vfs.list_dir(""),
        vec![
            "block.properties",
            "composite.fsh",
            "composite.vsh",
            "empty/",
            "lang/",
            "lib/",
            "shaders.properties",
            "world0/"
        ]
    );
    assert_eq!(
        vfs.list_dir("world0"),
        vec!["composite1.fsh", "composite1_a.csh", "gbuffers_terrain.fsh"]
    );
    assert!(vfs.is_dir("empty"));
    assert!(vfs.list_dir("empty").is_empty());
    assert_eq!(vfs.all_files().len(), FIXTURE.len());
    // Case-insensitive fallback (the fixture includes `/lib/Settings.glsl`).
    assert!(vfs.read("lib/Settings.glsl").is_some());
    assert_eq!(vfs.case_fallbacks().len(), 1);
    assert_eq!(vfs.list_dir("WORLD0").len(), 3);
}

#[test]
fn root_level_shaders_dir_wins() {
    let mut bytes_files: Vec<(String, &str)> = FIXTURE
        .iter()
        .map(|(p, c)| (format!("shaders/{p}"), *c))
        .collect();
    bytes_files.push(("Other/shaders/composite.fsh".to_string(), "other"));
    let refs: Vec<(&str, &str)> = bytes_files.iter().map(|(p, c)| (p.as_str(), *c)).collect();
    let vfs = ZipVfs::from_bytes(zip_bytes("", &refs, &[])).unwrap();
    assert_eq!(vfs.root_prefix(), "shaders/");
    assert_eq!(
        vfs.read("composite.fsh").as_deref(),
        Some(FIXTURE[2].1.as_bytes())
    );
}

#[test]
fn several_nested_candidates_pick_the_first_sorted() {
    let bytes = zip_bytes(
        "",
        &[("B/shaders/final.fsh", "b"), ("A/shaders/final.fsh", "a")],
        &[],
    );
    let vfs = ZipVfs::from_bytes(bytes).unwrap();
    assert_eq!(vfs.root_prefix(), "A/shaders/");
    assert_eq!(vfs.read("final.fsh").as_deref(), Some(&b"a"[..]));
}

#[test]
fn macos_metadata_folder_is_never_the_root() {
    // `__MACOSX` sorts before lower-case names; it must not be picked.
    let bytes = zip_bytes(
        "",
        &[
            ("__MACOSX/shaders/._final.fsh", "resource fork"),
            ("pack/shaders/final.fsh", "real"),
        ],
        &[],
    );
    let vfs = ZipVfs::from_bytes(bytes).unwrap();
    assert_eq!(vfs.root_prefix(), "pack/shaders/");
    assert_eq!(vfs.read("final.fsh").as_deref(), Some(&b"real"[..]));
    let only_metadata = zip_bytes("", &[("__MACOSX/shaders/._final.fsh", "x")], &[]);
    assert!(matches!(
        ZipVfs::from_bytes(only_metadata),
        Err(PackError::NotAShaderPack { .. })
    ));
}

#[test]
fn root_is_a_directory_and_entries_read_intact() {
    let bytes = zip_bytes("shaders/", &[("final.fsh", "void main() {}")], &[]);
    let vfs = ZipVfs::from_bytes(bytes).unwrap();
    assert_eq!(
        vfs.read("final.fsh").as_deref(),
        Some(&b"void main() {}"[..])
    );
    assert!(vfs.is_dir(""));
    assert!(!vfs.is_dir("final.fsh"));
}

#[test]
fn deeper_nesting_or_no_shaders_dir_is_rejected() {
    let bytes = zip_bytes(
        "",
        &[("a/b/shaders/final.fsh", "x"), ("final.fsh", "y")],
        &[],
    );
    assert!(matches!(
        ZipVfs::from_bytes(bytes),
        Err(PackError::NotAShaderPack { .. })
    ));
    assert!(matches!(
        ZipVfs::from_bytes(b"PK\x03\x04 broken".to_vec()),
        Err(PackError::Zip { .. })
    ));
}

#[test]
fn zip_dir_and_memory_packs_agree() {
    // Zip on disk.
    let tmp = tempfile::tempdir().unwrap();
    let zip_path = tmp.path().join("Fixture Pack.zip");
    std::fs::write(&zip_path, zip_bytes("Fixture Pack/shaders/", FIXTURE, &[])).unwrap();
    let zip_pack = ShaderPack::open(&zip_path).unwrap();
    assert_eq!(zip_pack.name(), "Fixture Pack");

    // Same files as a directory.
    let dir = tmp.path().join("Fixture Dir");
    for (p, c) in FIXTURE {
        let full = dir.join("shaders").join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, c).unwrap();
    }
    let dir_pack = ShaderPack::open(&dir).unwrap();
    assert_eq!(dir_pack.name(), "Fixture Dir");
    let shaders_dir_pack = ShaderPack::open(&dir.join("shaders")).unwrap();
    assert_eq!(
        shaders_dir_pack.name(),
        "Fixture Dir",
        "a path to `shaders/` is named after its parent"
    );

    // And in memory.
    let mut mem = MemVfs::new();
    for (p, c) in FIXTURE {
        mem.insert(p, c.as_bytes());
    }
    let mem_pack = ShaderPack::from_vfs("mem", Box::new(mem));

    let hash = mem_pack.content_hash();
    assert_eq!(zip_pack.content_hash(), hash);
    assert_eq!(dir_pack.content_hash(), hash);

    for pack in [&zip_pack, &dir_pack, &mem_pack] {
        assert_eq!(pack.world_folders(), vec!["world0"]);
        let root = pack.program_set("");
        assert_eq!(root.programs.keys().collect::<Vec<_>>(), vec!["composite"]);
        let w0 = pack.program_set("world0");
        assert_eq!(
            w0.programs.keys().collect::<Vec<_>>(),
            vec!["composite1", "gbuffers_terrain"]
        );
        assert_eq!(w0.get("composite1").unwrap().computes.len(), 1);
        assert_eq!(pack.lang("en_us")["option.SHADOWS"], "Shadows");
        let (opts, _) = options::discover(pack, &pack.option_start_files());
        assert_eq!(opts.names().collect::<Vec<_>>(), vec!["SHADOWS", "QUALITY"]);
    }
}

#[test]
fn edited_sources_over_a_zip_pack() {
    let pack = ShaderPack::from_vfs(
        "z",
        Box::new(ZipVfs::from_bytes(zip_bytes("shaders/", FIXTURE, &[])).unwrap()),
    );
    let (opts, _) = options::discover(&pack, &pack.option_start_files());
    // `composite.fsh` includes `/lib/Settings.glsl` (wrong case): the graph holds both
    // spellings and both get edited.
    assert!(opts.files.contains(&"lib/Settings.glsl".to_string()));
    let values = OptionValues::from_pairs([("SHADOWS", "false"), ("QUALITY", "3")]);
    let sources = EditedSources::new(Arc::new(pack), opts, values);
    for path in ["lib/settings.glsl", "lib/Settings.glsl"] {
        let text = sources.read(path).unwrap();
        assert!(
            text.starts_with("//#define SHADOWS // Shadows\n#define QUALITY 3 // [1 2 3]\n"),
            "{path}: {text}"
        );
    }
    assert_eq!(sources.read("composite.vsh").as_deref(), Some(FIXTURE[1].1));
}
