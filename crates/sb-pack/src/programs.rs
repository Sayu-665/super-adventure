//! Program discovery: which programs (and which stages / computes of each) a program
//! folder (`""` = pack root, `world0`, ...) provides.
//!
//! Rules (Iris `ProgramSet`):
//!
//! * Only direct children of the folder are considered.
//! * Stage files are `<program>.<vsh|tcs|tes|gsh|fsh>`; `<program>` must parse with
//!   [`ProgramName::parse`]. Other names (include fragments, mod-specific programs such
//!   as `clrwl_*`) are ignored and listed in [`ProgramSet::unrecognized`].
//! * Computes are `<program>.csh` plus `<program>_a.csh` ..`_z.csh`. Lettered computes
//!   are loaded in order and loading **stops at the first missing letter**; later
//!   letters are kept in [`ProgramSources::ignored_computes`] with a warning.
//! * Iris only runs computes of composite-style programs (`setup*`, `begin*`,
//!   `shadowcomp*`, `prepare*`, `deferred*`, `composite*`, `final`) and of `shadow`;
//!   `setup*` takes no letters. Computes elsewhere are ignored with a warning.
//! * `.tcs`/`.tes` files are discovered unconditionally; Iris only uses them when the
//!   pack declares the `TESSELLATION_SHADERS` feature flag.

use sb_core::program::{GeometryProgram, PassGroup};
use sb_core::{Diagnostic, Diagnostics, ProgramName, ShaderStage, SourceLocation};
use std::collections::BTreeMap;

/// The programs found in one folder.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProgramSet {
    /// Folder relative to the shaders root (`""` = root, `world0`, ...).
    pub folder: String,
    /// Program base name (`gbuffers_terrain`, `composite3`, ...) -> sources.
    pub programs: BTreeMap<String, ProgramSources>,
    /// Files with a program extension whose name is not a known program.
    pub unrecognized: Vec<String>,
    /// Warnings found during discovery (ignored computes, ...).
    pub diagnostics: Diagnostics,
}

/// The source files of one program.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramSources {
    pub name: ProgramName,
    /// Graphics stages: stage -> file path (relative to the shaders root).
    pub stages: BTreeMap<ShaderStage, String>,
    /// Computes that Iris runs, in dispatch order: `None` = `NAME.csh`, `Some('a')` =
    /// `NAME_a.csh`, ...
    pub computes: BTreeMap<Option<char>, String>,
    /// Compute files present but not run (after a missing letter, or in a program
    /// that does not support computes).
    pub ignored_computes: BTreeMap<Option<char>, String>,
}

impl ProgramSources {
    fn new(name: ProgramName) -> Self {
        Self {
            name,
            stages: BTreeMap::new(),
            computes: BTreeMap::new(),
            ignored_computes: BTreeMap::new(),
        }
    }

    /// Program base file name (`composite3`).
    pub fn file_name(&self) -> String {
        self.name.file_name()
    }

    /// Path of a stage, if present.
    pub fn stage(&self, stage: ShaderStage) -> Option<&str> {
        self.stages.get(&stage).map(String::as_str)
    }

    /// True if the program has any graphics stage.
    pub fn has_graphics(&self) -> bool {
        !self.stages.is_empty()
    }

    /// True if the program has anything to run: a graphics stage or a dispatched
    /// compute. (A program consisting only of [`ProgramSources::ignored_computes`], e.g.
    /// a lone `gbuffers_terrain.csh`, is not runnable.)
    pub fn is_runnable(&self) -> bool {
        self.has_graphics() || !self.computes.is_empty()
    }

    /// True if the program has a fragment shader but no vertex shader (Iris then
    /// synthesizes a `#version 120` `ftransform()` vertex shader).
    pub fn needs_synthesized_vertex(&self) -> bool {
        self.stages.contains_key(&ShaderStage::Fragment)
            && !self.stages.contains_key(&ShaderStage::Vertex)
    }

    /// Every file (stages, then computes that run).
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.stages
            .values()
            .chain(self.computes.values())
            .map(String::as_str)
    }
}

impl ProgramSet {
    /// Discover programs among `entries` (the [`crate::Vfs::list_dir`] listing of
    /// `folder`).
    pub fn discover(folder: &str, entries: &[String]) -> ProgramSet {
        let folder = folder.trim_matches('/').to_string();
        let mut set = ProgramSet {
            folder: folder.clone(),
            ..Default::default()
        };
        // Lettered computes per program, collected before gap analysis.
        let mut computes: BTreeMap<String, BTreeMap<Option<char>, String>> = BTreeMap::new();

        for entry in entries {
            if entry.ends_with('/') {
                continue;
            }
            let Some((stem, ext)) = entry.rsplit_once('.') else {
                continue;
            };
            let Some(stage) = ShaderStage::from_pack_extension(ext) else {
                continue;
            };
            let path = if folder.is_empty() {
                entry.clone()
            } else {
                format!("{folder}/{entry}")
            };
            if stage == ShaderStage::Compute {
                let (base, letter) = ProgramName::split_compute_letter(stem);
                match ProgramName::parse(base) {
                    Some(name) => {
                        set.programs
                            .entry(base.to_string())
                            .or_insert_with(|| ProgramSources::new(name));
                        computes
                            .entry(base.to_string())
                            .or_default()
                            .insert(letter, path);
                    }
                    None => set.unrecognized.push(path),
                }
            } else {
                match ProgramName::parse(stem) {
                    Some(name) => {
                        set.programs
                            .entry(stem.to_string())
                            .or_insert_with(|| ProgramSources::new(name))
                            .stages
                            .insert(stage, path);
                    }
                    None => set.unrecognized.push(path),
                }
            }
        }

        for (base, found) in computes {
            let Some(program) = set.programs.get_mut(&base) else {
                continue;
            };
            let support = compute_support(&program.name);
            let mut next_letter = Some('a');
            for (letter, path) in found {
                let reason = match (support, letter) {
                    (ComputeSupport::None, _) => {
                        Some("computes of this program are never dispatched".to_string())
                    }
                    (ComputeSupport::Unlettered, Some(_)) => Some(
                        "lettered computes of `setup` programs are never dispatched".to_string(),
                    ),
                    (_, None) => None,
                    (ComputeSupport::Lettered, Some(c)) => {
                        if Some(c) == next_letter {
                            next_letter = char::from_u32(c as u32 + 1).filter(|n| *n <= 'z');
                            None
                        } else {
                            let missing = next_letter
                                .map(|m| format!("`{base}_{m}.csh`"))
                                .unwrap_or_else(|| "an earlier letter".into());
                            next_letter = None;
                            Some(format!(
                                "{missing} is missing and lettered computes stop at the first gap"
                            ))
                        }
                    }
                };
                match reason {
                    None => {
                        program.computes.insert(letter, path);
                    }
                    Some(why) => {
                        set.diagnostics.push(
                            Diagnostic::warning(
                                "pack.compute-ignored",
                                format!("compute shader `{path}` ignored: {why}"),
                            )
                            .at(SourceLocation::new(path.clone(), 1)),
                        );
                        program.ignored_computes.insert(letter, path);
                    }
                }
            }
        }
        set.unrecognized.sort();
        set
    }

    /// Look a program up by base name (`composite3`).
    pub fn get(&self, name: &str) -> Option<&ProgramSources> {
        self.programs.get(name)
    }

    /// Look a program up by [`ProgramName`].
    pub fn get_program(&self, name: &ProgramName) -> Option<&ProgramSources> {
        self.programs.get(&name.file_name())
    }

    /// Look a geometry program up.
    pub fn geometry(&self, program: GeometryProgram) -> Option<&ProgramSources> {
        self.programs.get(program.file_name())
    }

    /// True if no program was found.
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }

    /// True if at least one program is runnable (see [`ProgramSources::is_runnable`]).
    pub fn has_runnable_programs(&self) -> bool {
        self.programs.values().any(ProgramSources::is_runnable)
    }

    /// Every program source file (stages and dispatched computes), sorted.
    pub fn files(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .programs
            .values()
            .flat_map(|p| p.files().map(str::to_string))
            .collect();
        out.sort();
        out
    }

    /// Composite-style programs of `group` in index order (`composite`, `composite1`, ...).
    pub fn group(&self, group: PassGroup) -> Vec<(u8, &ProgramSources)> {
        let mut out: Vec<(u8, &ProgramSources)> = self
            .programs
            .values()
            .filter_map(|p| match p.name {
                ProgramName::Composite { group: g, index } if g == group => Some((index, p)),
                _ => None,
            })
            .collect();
        out.sort_by_key(|(i, _)| *i);
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComputeSupport {
    None,
    Unlettered,
    Lettered,
}

fn compute_support(name: &ProgramName) -> ComputeSupport {
    match name {
        ProgramName::Composite {
            group: PassGroup::Setup,
            ..
        } => ComputeSupport::Unlettered,
        ProgramName::Composite { .. } => ComputeSupport::Lettered,
        ProgramName::Geometry {
            program: GeometryProgram::Shadow,
        } => ComputeSupport::Lettered,
        ProgramName::Geometry { .. } => ComputeSupport::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn discovers_stages_and_ignores_fragments() {
        let set = ProgramSet::discover(
            "world0",
            &entries(&[
                "composite.fsh",
                "composite.vsh",
                "gbuffers_terrain.vsh",
                "gbuffers_terrain.gsh",
                "gbuffers_terrain.fsh",
                "gbuffers_water.tcs",
                "gbuffers_water.tes",
                "final.fsh",
                "common.glsl",
                "clrwl_gbuffers.fsh",
                "lib/",
                "composite01.fsh",
                "shaders.properties",
            ]),
        );
        assert_eq!(set.folder, "world0");
        assert_eq!(
            set.programs.keys().collect::<Vec<_>>(),
            vec!["composite", "final", "gbuffers_terrain", "gbuffers_water"]
        );
        let terrain = set.get("gbuffers_terrain").unwrap();
        assert_eq!(
            terrain.stage(ShaderStage::Geometry),
            Some("world0/gbuffers_terrain.gsh")
        );
        assert_eq!(terrain.stages.len(), 3);
        assert!(set.get("final").unwrap().needs_synthesized_vertex());
        assert!(!set.get("composite").unwrap().needs_synthesized_vertex());
        assert_eq!(set.get("gbuffers_water").unwrap().stages.len(), 2);
        assert_eq!(
            set.unrecognized,
            vec!["world0/clrwl_gbuffers.fsh", "world0/composite01.fsh"]
        );
        assert!(set.diagnostics.is_empty());
        assert_eq!(set.files().len(), 8);
    }

    #[test]
    fn computes_stop_at_first_missing_letter() {
        let set = ProgramSet::discover(
            "",
            &entries(&[
                "composite3.csh",
                "composite3_a.csh",
                "composite3_b.csh",
                "composite3_d.csh",
                "composite3_e.csh",
                "composite3.fsh",
            ]),
        );
        let p = set.get("composite3").unwrap();
        assert_eq!(
            p.computes.keys().copied().collect::<Vec<_>>(),
            vec![None, Some('a'), Some('b')]
        );
        assert_eq!(
            p.ignored_computes.keys().copied().collect::<Vec<_>>(),
            vec![Some('d'), Some('e')]
        );
        assert_eq!(set.diagnostics.len(), 2);
        assert!(
            set.diagnostics
                .iter()
                .next()
                .unwrap()
                .message
                .contains("composite3_c.csh")
        );
    }

    #[test]
    fn lettered_compute_without_base() {
        // photon ships deferred4_a.csh without deferred4.csh: index 0 is independent.
        let set = ProgramSet::discover(
            "",
            &entries(&["deferred4_a.csh", "deferred4.fsh", "deferred4.vsh"]),
        );
        let p = set.get("deferred4").unwrap();
        assert_eq!(
            p.computes.keys().copied().collect::<Vec<_>>(),
            vec![Some('a')]
        );
        assert!(p.has_graphics());
    }

    #[test]
    fn compute_only_programs() {
        let set = ProgramSet::discover(
            "",
            &entries(&[
                "setup.csh",
                "setup1.csh",
                "setup_a.csh",
                "shadowcomp.csh",
                "shadow_a.csh",
                "final.csh",
            ]),
        );
        assert!(set.get("setup").unwrap().computes.contains_key(&None));
        assert!(
            set.get("setup")
                .unwrap()
                .ignored_computes
                .contains_key(&Some('a'))
        );
        assert!(set.get("setup1").unwrap().computes.contains_key(&None));
        assert!(!set.get("shadowcomp").unwrap().has_graphics());
        assert!(set.get("shadow").unwrap().computes.contains_key(&Some('a')));
        assert!(set.get("final").unwrap().computes.contains_key(&None));
        assert_eq!(set.diagnostics.len(), 1);
    }

    #[test]
    fn geometry_computes_are_ignored_except_shadow() {
        let set = ProgramSet::discover(
            "",
            &entries(&["gbuffers_terrain.csh", "gbuffers_terrain.fsh"]),
        );
        let p = set.get("gbuffers_terrain").unwrap();
        assert!(p.computes.is_empty());
        assert_eq!(p.ignored_computes.len(), 1);
        assert!(p.is_runnable(), "it still has a fragment shader");

        // A folder whose only "program" is an ignored compute has nothing to run.
        let set =
            ProgramSet::discover("world5", &entries(&["gbuffers_terrain.csh", "setup_b.csh"]));
        assert!(!set.is_empty());
        assert!(!set.has_runnable_programs());
        assert!(set.programs.values().all(|p| !p.is_runnable()));
    }

    #[test]
    fn group_listing_is_index_ordered() {
        let set = ProgramSet::discover(
            "",
            &entries(&[
                "composite10.fsh",
                "composite2.fsh",
                "composite.fsh",
                "deferred.fsh",
            ]),
        );
        let order: Vec<u8> = set
            .group(PassGroup::Composite)
            .iter()
            .map(|(i, _)| *i)
            .collect();
        assert_eq!(order, vec![0, 2, 10]);
        assert_eq!(set.group(PassGroup::Deferred).len(), 1);
        assert!(set.geometry(GeometryProgram::Terrain).is_none());
    }
}
