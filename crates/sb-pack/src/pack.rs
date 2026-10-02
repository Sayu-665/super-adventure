//! [`ShaderPack`]: an opened pack (directory or zip) with typed access to its files,
//! program sets, world folders and lang files.

use crate::error::PackError;
use crate::programs::ProgramSet;
use crate::text::{decode_latin1, decode_utf8};
use crate::vfs::{CaseFallback, DirVfs, MemVfs, Vfs, ZipVfs, clean_path};
use crate::{lang, properties};
use indexmap::IndexMap;
use sb_core::{Diagnostic, Diagnostics, SourceLocation};
use std::path::Path;

/// The last component of `path` if it is a plain name (not `.`, `..` or a root).
fn literal_name(path: &Path) -> Option<String> {
    match path.components().next_back() {
        Some(std::path::Component::Normal(name)) => Some(name.to_string_lossy().into_owned()),
        _ => None,
    }
}

/// The name of `path`: its literal last component, or for paths such as `.` or `x/..`
/// the name of the resolved directory. (The literal name is preferred so that a
/// symlinked pack keeps the name the user gave it.)
fn path_name(path: &Path) -> Option<String> {
    literal_name(path).or_else(|| literal_name(&std::fs::canonicalize(path).ok()?))
}

/// The name of the directory containing `path` (see [`path_name`]).
fn parent_name(path: &Path) -> Option<String> {
    path.parent()
        .and_then(literal_name)
        .or_else(|| literal_name(std::fs::canonicalize(path).ok()?.parent()?))
}

/// The world folders Iris recognizes without `dimension.properties`, in order, with the
/// dimension ids they serve.
pub const STANDARD_WORLD_FOLDERS: [(&str, &[&str]); 3] = [
    ("world0", &["minecraft:overworld", "*"]),
    ("world-1", &["minecraft:the_nether"]),
    ("world1", &["minecraft:the_end"]),
];

/// An opened shader pack. Paths are relative to the pack's `shaders/` root.
pub struct ShaderPack {
    name: String,
    vfs: Box<dyn Vfs>,
    /// `dimension.properties` map (folder -> dimension ids), set by the caller after
    /// preprocessing the file.
    dimension_map: Option<IndexMap<String, Vec<String>>>,
}

impl std::fmt::Debug for ShaderPack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShaderPack")
            .field("name", &self.name)
            .field("vfs", &self.vfs.describe())
            .field("dimension_map", &self.dimension_map)
            .finish()
    }
}

impl ShaderPack {
    /// Open a pack from a directory (`<dir>/shaders`, or `dir` itself if it is a
    /// shaders root) or a zip file (`shaders/` at the root or one level nested).
    ///
    /// The pack name is the directory name or the zip file name without extension.
    pub fn open(path: &Path) -> Result<ShaderPack, PackError> {
        let meta = std::fs::metadata(path).map_err(|source| PackError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let file_name = path_name(path).unwrap_or_default();
        if meta.is_dir() {
            let vfs = DirVfs::open_pack(path)?;
            // A path pointing at the `shaders` directory itself is named after its parent.
            let name = if vfs.root() == path && file_name == "shaders" {
                parent_name(path).unwrap_or(file_name)
            } else {
                file_name
            };
            return Ok(Self::from_vfs(name, Box::new(vfs)));
        }
        let vfs = ZipVfs::open(path)?;
        let name = match file_name.rsplit_once('.') {
            Some((stem, ext)) if ext.eq_ignore_ascii_case("zip") => stem.to_string(),
            _ => file_name,
        };
        Ok(Self::from_vfs(name, Box::new(vfs)))
    }

    /// Wrap an existing file system.
    pub fn from_vfs(name: impl Into<String>, vfs: Box<dyn Vfs>) -> ShaderPack {
        ShaderPack {
            name: name.into(),
            vfs,
            dimension_map: None,
        }
    }

    /// An in-memory pack built from `(path, contents)` pairs (tests, tools).
    pub fn from_files<P: AsRef<str>, C: Into<Vec<u8>>>(
        name: impl Into<String>,
        files: impl IntoIterator<Item = (P, C)>,
    ) -> ShaderPack {
        let mut vfs = MemVfs::new();
        for (p, c) in files {
            vfs.insert(p.as_ref(), c);
        }
        Self::from_vfs(name, Box::new(vfs))
    }

    /// Pack name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The underlying file system.
    pub fn vfs(&self) -> &dyn Vfs {
        self.vfs.as_ref()
    }

    /// Read a file's bytes.
    pub fn read_bytes(&self, p: &str) -> Option<Vec<u8>> {
        self.vfs.read(p)
    }

    /// Read a UTF-8 text file (lossy, BOM stripped). Use for GLSL and `.lang` files.
    pub fn read_text(&self, p: &str) -> Option<String> {
        self.vfs.read(p).map(|b| decode_utf8(&b))
    }

    /// Read an ISO-8859-1 text file (BOM stripped). Use for `.properties` files.
    pub fn read_latin1(&self, p: &str) -> Option<String> {
        self.vfs.read(p).map(|b| decode_latin1(&b))
    }

    /// Read and parse a `.properties` file without preprocessing.
    pub fn read_properties(&self, p: &str) -> Option<Vec<properties::PropEntry>> {
        self.read_latin1(p).map(|t| properties::parse(&t))
    }

    /// Whether a file exists.
    pub fn exists(&self, p: &str) -> bool {
        self.vfs.exists(p)
    }

    /// Whether a directory exists (and is non-empty).
    pub fn is_dir(&self, p: &str) -> bool {
        self.vfs.is_dir(p)
    }

    /// Direct children of a directory (dirs end with `/`).
    pub fn list_dir(&self, dir: &str) -> Vec<String> {
        self.vfs.list_dir(dir)
    }

    /// Every file of the pack, sorted.
    pub fn all_files(&self) -> Vec<String> {
        self.vfs.all_files()
    }

    /// Set the parsed `dimension.properties` map (folder -> dimension ids); see
    /// [`crate::idmap::parse_dimension_properties`]. The caller preprocesses the file
    /// with environment macros first.
    pub fn set_dimension_map(&mut self, map: IndexMap<String, Vec<String>>) {
        self.dimension_map = Some(map);
    }

    /// Builder-style [`ShaderPack::set_dimension_map`].
    pub fn with_dimension_map(mut self, map: IndexMap<String, Vec<String>>) -> Self {
        self.set_dimension_map(map);
        self
    }

    /// The dimension map set with [`ShaderPack::set_dimension_map`].
    pub fn dimension_map(&self) -> Option<&IndexMap<String, Vec<String>>> {
        self.dimension_map.as_ref()
    }

    /// World folders that exist and contain at least one runnable program (see
    /// [`ProgramSet::has_runnable_programs`]): the standard `world0`, `world-1`,
    /// `world1` (in that order), then the folders named in the dimension map (in map
    /// order).
    ///
    /// Note that with a non-empty dimension map Iris only *uses* the folders the map
    /// names; [`ShaderPack::dimension_assignments`] applies that rule.
    pub fn world_folders(&self) -> Vec<String> {
        let mut candidates: Vec<String> = STANDARD_WORLD_FOLDERS
            .iter()
            .map(|(f, _)| f.to_string())
            .collect();
        if let Some(map) = &self.dimension_map {
            for folder in map.keys() {
                if !candidates.contains(folder) {
                    candidates.push(folder.clone());
                }
            }
        }
        candidates
            .into_iter()
            .filter(|f| {
                clean_path(f).is_some_and(|c| !c.is_empty())
                    && self.program_set(f).has_runnable_programs()
            })
            .collect()
    }

    /// Which dimensions each world folder serves (Iris rules):
    ///
    /// * with a non-empty dimension map: its entries whose folder exists;
    /// * otherwise `world0` -> `minecraft:overworld` + `*` (wildcard), `world-1` ->
    ///   `minecraft:the_nether`, `world1` -> `minecraft:the_end` (existing ones only).
    ///
    /// Dimensions without a folder use the root program set (see
    /// [`ShaderPack::base_folder`]).
    pub fn dimension_assignments(&self) -> IndexMap<String, Vec<String>> {
        let existing = self.world_folders();
        match &self.dimension_map {
            Some(map) if !map.is_empty() => map
                .iter()
                .filter(|(f, _)| existing.contains(f))
                .map(|(f, ids)| (f.clone(), ids.clone()))
                .collect(),
            _ => STANDARD_WORLD_FOLDERS
                .iter()
                .filter(|(f, _)| existing.iter().any(|e| e == f))
                .map(|(f, ids)| (f.to_string(), ids.iter().map(|s| s.to_string()).collect()))
                .collect(),
        }
    }

    /// The folder whose programs serve dimensions without their own folder: the folder
    /// assigned the `*` wildcard (Iris uses it as the base program set), else `""`.
    pub fn base_folder(&self) -> String {
        self.dimension_assignments()
            .into_iter()
            .find(|(_, ids)| ids.iter().any(|i| i == "*" || i == "*:*"))
            .map(|(f, _)| f)
            .unwrap_or_default()
    }

    /// Discover the programs of `folder` (`""` = pack root).
    pub fn program_set(&self, folder: &str) -> ProgramSet {
        let Some(folder) = clean_path(folder) else {
            return ProgramSet::default();
        };
        ProgramSet::discover(&folder, &self.vfs.list_dir(&folder))
    }

    /// Program sets of the root (if it has runnable programs) and of every world folder
    /// ([`ShaderPack::world_folders`]).
    pub fn program_sets(&self) -> Vec<ProgramSet> {
        let mut out = Vec::new();
        let root = self.program_set("");
        if root.has_runnable_programs() {
            out.push(root);
        }
        for f in self.world_folders() {
            out.push(self.program_set(&f));
        }
        out
    }

    /// The folders Iris loads program sets from: the pack root (`""`) followed by the
    /// dimension folders of [`ShaderPack::dimension_assignments`]. With a non-empty
    /// dimension map, existing world folders the map does not name are *not* included
    /// (Iris ignores them, so their files take no part in option discovery).
    pub fn program_folders(&self) -> Vec<String> {
        let mut out = vec![String::new()];
        out.extend(self.dimension_assignments().into_keys());
        out
    }

    /// The start files for option discovery (Iris `ShaderPackSourceNames`): in every
    /// folder of [`ShaderPack::program_folders`], every stage file of a recognized
    /// program, and every compute file of composite-style programs and of `shadow`
    /// (including lettered computes after a gap). Sorted.
    pub fn option_start_files(&self) -> Vec<String> {
        use sb_core::program::GeometryProgram;
        let mut files: Vec<String> = Vec::new();
        for set in self.program_folders().iter().map(|f| self.program_set(f)) {
            for p in set.programs.values() {
                files.extend(p.stages.values().cloned());
                let computes_listed = !matches!(p.name, sb_core::ProgramName::Geometry { program } if program != GeometryProgram::Shadow);
                if computes_listed {
                    files.extend(p.computes.values().cloned());
                    files.extend(p.ignored_computes.values().cloned());
                }
            }
        }
        files.sort();
        files.dedup();
        files
    }

    /// blake3 hash (hex) over every file: sorted `(path, bytes)` pairs, length-prefixed.
    pub fn content_hash(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        for path in self.vfs.all_files() {
            let bytes = self.vfs.read(&path).unwrap_or_default();
            hasher.update(&(path.len() as u64).to_le_bytes());
            hasher.update(path.as_bytes());
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(&bytes);
        }
        hasher.finalize().to_hex().to_string()
    }

    /// Language codes with a `lang/<code>.lang` file (lower-case), sorted.
    pub fn languages(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .vfs
            .list_dir("lang")
            .into_iter()
            .filter(|n| !n.ends_with('/'))
            .filter_map(|n| {
                let (stem, ext) = n.rsplit_once('.')?;
                ext.eq_ignore_ascii_case("lang")
                    .then(|| stem.to_lowercase())
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Lang strings for `code` (e.g. `de_de`, case-insensitive), falling back to
    /// `en_us` for missing keys. Empty if neither file exists.
    pub fn lang(&self, code: &str) -> IndexMap<String, String> {
        let mut map = self.lang_file("en_us").unwrap_or_default();
        if !code.eq_ignore_ascii_case("en_us")
            && let Some(over) = self.lang_file(code)
        {
            for (k, v) in over {
                map.insert(k, v);
            }
        }
        map
    }

    /// Lang strings of exactly one language file (no fallback).
    pub fn lang_file(&self, code: &str) -> Option<IndexMap<String, String>> {
        let wanted = code.to_lowercase();
        let file = self.vfs.list_dir("lang").into_iter().find(|n| {
            n.rsplit_once('.').is_some_and(|(stem, ext)| {
                ext.eq_ignore_ascii_case("lang") && stem.to_lowercase() == wanted
            })
        })?;
        Some(lang::parse_lang(&self.read_text(&format!("lang/{file}"))?))
    }

    /// Case-insensitive path fallbacks used so far.
    pub fn case_fallbacks(&self) -> Vec<CaseFallback> {
        self.vfs.case_fallbacks()
    }

    /// File-system level diagnostics: case-insensitive fallbacks (warnings, since they
    /// break on case-sensitive systems in other loaders) and I/O problems.
    pub fn vfs_diagnostics(&self) -> Diagnostics {
        let mut d = Diagnostics::new();
        for f in self.case_fallbacks() {
            d.push(
                Diagnostic::warning(
                    "pack.case-mismatch",
                    format!(
                        "`{}` does not exist; using `{}` (case-insensitive match)",
                        f.requested, f.actual
                    ),
                )
                .at(SourceLocation::new(f.actual.clone(), 1)),
            );
        }
        for w in self.vfs.warnings() {
            d.push(Diagnostic::warning("pack.io", w));
        }
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ShaderPack {
        ShaderPack::from_files(
            "sample",
            [
                ("shaders.properties", "sun=false\n"),
                ("composite.fsh", "void main(){}"),
                ("composite.vsh", "void main(){}"),
                ("world0/gbuffers_terrain.fsh", "x"),
                ("world-1/final.fsh", "x"),
                ("world1/readme.txt", "no programs"),
                ("custom_dim/composite.fsh", "x"),
                ("lang/en_US.lang", "option.A=Alpha\noption.B=Beta\n"),
                ("lang/de_de.lang", "option.A=Alfa\n"),
                ("lib/common.glsl", "\u{feff}// bom"),
            ],
        )
    }

    #[test]
    fn world_folders_and_assignments() {
        let pack = sample();
        assert_eq!(pack.world_folders(), vec!["world0", "world-1"]);
        let a = pack.dimension_assignments();
        assert_eq!(a.keys().collect::<Vec<_>>(), vec!["world0", "world-1"]);
        assert_eq!(a["world0"], vec!["minecraft:overworld", "*"]);
        assert_eq!(pack.base_folder(), "world0");

        let mut map = IndexMap::new();
        map.insert("custom_dim".to_string(), vec!["mod:dim".to_string()]);
        map.insert("missing".to_string(), vec!["*".to_string()]);
        let pack = pack.with_dimension_map(map);
        assert_eq!(
            pack.world_folders(),
            vec!["world0", "world-1", "custom_dim"]
        );
        let a = pack.dimension_assignments();
        assert_eq!(a.keys().collect::<Vec<_>>(), vec!["custom_dim"]);
        assert_eq!(pack.base_folder(), "");
    }

    #[test]
    fn program_sets_and_start_files() {
        let pack = sample();
        let sets = pack.program_sets();
        assert_eq!(
            sets.iter().map(|s| s.folder.as_str()).collect::<Vec<_>>(),
            vec!["", "world0", "world-1"]
        );
        assert_eq!(
            pack.option_start_files(),
            vec![
                "composite.fsh",
                "composite.vsh",
                "world-1/final.fsh",
                "world0/gbuffers_terrain.fsh"
            ]
        );
    }

    #[test]
    fn start_files_follow_the_dimension_map() {
        // Regression: with a non-empty dimension map, Iris only reads the root and the
        // folders the map names; `world-1` must not contribute start files (its option
        // declarations could otherwise make options ambiguous).
        let mut map = IndexMap::new();
        map.insert("world0".to_string(), vec!["*".to_string()]);
        let pack = sample().with_dimension_map(map);
        assert_eq!(pack.program_folders(), vec!["", "world0"]);
        assert_eq!(
            pack.option_start_files(),
            vec![
                "composite.fsh",
                "composite.vsh",
                "world0/gbuffers_terrain.fsh"
            ]
        );
        // `world_folders` still reports every existing folder.
        assert_eq!(pack.world_folders(), vec!["world0", "world-1"]);
        // An empty map behaves like no map at all.
        let pack = sample().with_dimension_map(IndexMap::new());
        assert_eq!(pack.program_folders(), vec!["", "world0", "world-1"]);
    }

    #[test]
    fn folders_without_runnable_programs_are_not_world_folders() {
        let pack = ShaderPack::from_files(
            "p",
            [
                ("world0/final.fsh", "x"),
                ("world1/gbuffers_terrain.csh", "not dispatched"),
            ],
        );
        assert_eq!(pack.world_folders(), vec!["world0"]);
    }

    #[test]
    fn text_reading() {
        let pack = sample();
        assert_eq!(pack.read_text("lib/common.glsl").unwrap(), "// bom");
        assert_eq!(
            pack.read_properties("shaders.properties").unwrap()[0].key,
            "sun"
        );
        assert!(pack.exists("/composite.fsh"));
        assert!(!pack.exists("nope.fsh"));
    }

    #[test]
    fn lang_with_fallback() {
        let pack = sample();
        assert_eq!(pack.languages(), vec!["de_de", "en_us"]);
        let de = pack.lang("de_DE");
        assert_eq!(de["option.A"], "Alfa");
        assert_eq!(de["option.B"], "Beta");
        let en = pack.lang("en_us");
        assert_eq!(en["option.A"], "Alpha");
        assert_eq!(pack.lang("fr_fr")["option.A"], "Alpha");
        assert!(pack.lang_file("fr_fr").is_none());
    }

    #[test]
    fn content_hash_is_stable_and_sensitive() {
        let a = sample().content_hash();
        assert_eq!(a, sample().content_hash());
        assert_eq!(a.len(), 64);
        let other = ShaderPack::from_files("x", [("composite.fsh", "changed")]);
        assert_ne!(a, other.content_hash());
    }

    #[test]
    fn case_fallback_diagnostics() {
        let pack = sample();
        assert!(pack.read_text("LIB/Common.glsl").is_some());
        let d = pack.vfs_diagnostics();
        assert_eq!(d.len(), 1);
        assert_eq!(d.iter().next().unwrap().code, "pack.case-mismatch");
    }

    #[test]
    fn pack_names_of_unusual_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let shaders = tmp.path().join("My Pack").join("shaders");
        std::fs::create_dir_all(&shaders).unwrap();
        std::fs::write(shaders.join("final.fsh"), "void main(){}").unwrap();
        // Regression: `x/..`-style paths used to produce an empty name.
        let dotted = shaders.join("..");
        assert_eq!(ShaderPack::open(&dotted).unwrap().name(), "My Pack");
        assert_eq!(ShaderPack::open(&shaders).unwrap().name(), "My Pack");
        assert_eq!(
            ShaderPack::open(&shaders.join(".")).unwrap().name(),
            "My Pack",
            "`shaders/.` is still the shaders directory"
        );
        assert_eq!(path_name(Path::new("a/b")).as_deref(), Some("b"));
        assert_eq!(parent_name(Path::new("a/b")).as_deref(), Some("a"));
    }

    #[test]
    fn open_errors() {
        assert!(matches!(
            ShaderPack::open(Path::new("/definitely/not/here")),
            Err(PackError::Io { .. })
        ));
        let tmp = tempfile::tempdir().unwrap();
        let bogus = tmp.path().join("bogus.zip");
        std::fs::write(&bogus, b"not a zip").unwrap();
        assert!(matches!(
            ShaderPack::open(&bogus),
            Err(PackError::Zip { .. })
        ));
    }
}
