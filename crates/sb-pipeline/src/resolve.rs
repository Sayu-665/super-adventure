//! Program resolution of one folder (spec step 6): which programs and computes exist and
//! are enabled (`program.<path>.enabled`, profile `!program.<path>` toggles), with Iris's
//! loading rules (a program needs a `.fsh`; a missing `.vsh` is synthesized; lettered
//! computes stop at the first missing or disabled letter; tessellation only with the
//! `TESSELLATION_SHADERS` feature).

use crate::load::PackLoad;
use sb_core::model::{CompileEnvironment, ProgramKind};
use sb_core::program::{GeometryGroup, GeometryProgram, PassGroup};
use sb_core::{Diagnostic, Diagnostics, ProgramName, ShaderStage, SourceLocation};
use sb_pack::ShaderPack;
use sb_pack::shaders_properties::program_path;
use std::collections::BTreeMap;

/// Where a stage's source comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageSource {
    /// A pack file (path relative to `shaders/`).
    File(String),
    /// Iris's default vertex shader for a program without `.vsh`; `virtual_path` is the
    /// `.vsh` path it stands in for.
    SynthesizedVertex { virtual_path: String },
}

impl StageSource {
    /// The (possibly virtual) file path.
    pub fn path(&self) -> &str {
        match self {
            StageSource::File(p) => p,
            StageSource::SynthesizedVertex { virtual_path } => virtual_path,
        }
    }
}

/// One program (graphics or compute) of a folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    /// Name within the folder: `gbuffers_terrain`, `composite3`, `composite3_a`, and
    /// `composite3.csh` for an unlettered compute.
    pub name: String,
    /// Name used for per-program `shaders.properties` keys (`blend.<x>`, `flip.<x>`,
    /// `indirect.<x>`, ...): the base name for graphics programs and unlettered computes,
    /// `<base>_<letter>` for lettered computes.
    pub props_name: String,
    /// Model program name (`world0/gbuffers_terrain`).
    pub path: String,
    pub kind: ProgramKind,
    pub stages: Vec<(ShaderStage, StageSource)>,
}

impl Unit {
    /// Whether this is a compute program.
    pub fn is_compute(&self) -> bool {
        matches!(self.kind, ProgramKind::Compute { .. } | ProgramKind::GeometryCompute { .. })
    }

    /// The geometry program of a graphics geometry unit.
    pub fn geometry(&self) -> Option<GeometryProgram> {
        match self.kind {
            ProgramKind::Geometry { program } => Some(program),
            _ => None,
        }
    }
}

/// One composite-style slot (`composite3`): its fullscreen program and computes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SlotPlan {
    pub index: u8,
    pub program: Option<usize>,
    pub computes: Vec<usize>,
}

/// The programs of one folder.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FolderPlan {
    pub folder: String,
    pub units: Vec<Unit>,
    /// Present and enabled geometry programs → unit index.
    pub geometry: BTreeMap<GeometryProgram, usize>,
    /// Composite-style groups (setup, begin, shadowcomp, prepare, deferred, composite,
    /// final) → slots in index order.
    pub groups: BTreeMap<PassGroup, Vec<SlotPlan>>,
    /// `shadow.csh`, `shadow_a.csh`, ... (unit indices, dispatch order).
    pub shadow_computes: Vec<usize>,
    /// Programs disabled by `program.*.enabled` or the current profile (paths).
    pub disabled: Vec<String>,
    pub diagnostics: Diagnostics,
}

/// Decides whether programs are enabled.
pub struct Enabler<'a> {
    load: &'a PackLoad,
    folder: &'a str,
}

impl<'a> Enabler<'a> {
    pub fn new(load: &'a PackLoad, folder: &'a str) -> Self {
        Self { load, folder }
    }

    /// Whether program `name` (`composite3`, `composite3_a`, `shadow`) of the folder is
    /// enabled. Mirrors Iris: profile toggles name the exact path; an expression that
    /// does not parse counts as enabled; identifiers that are not boolean options are
    /// `true` (value options compare numerically).
    pub fn enabled(&self, name: &str, diags: &mut Diagnostics) -> bool {
        let path = program_path(self.folder, name);
        if self.load.profile_disabled.contains(&path) {
            return false;
        }
        let Some(expr) = self.load.props.program_enabled_expr(self.folder, name) else { return true };
        let lookup = |id: &str| -> Option<sb_expr::Value> {
            let o = self.load.options.option(id)?;
            let v = sb_pack::options::is_boolean_option(o);
            let value = self.load.options.effective_value(id, &self.load.values).unwrap_or_default();
            Some(if v {
                sb_expr::Value::Bool(value == "true")
            } else if let Ok(i) = value.parse::<i32>() {
                sb_expr::Value::Int(i)
            } else if let Ok(f) = value.parse::<f32>() {
                sb_expr::Value::Float(f)
            } else {
                sb_expr::Value::Bool(true)
            })
        };
        let mut unknown = Vec::new();
        let full = |id: &str| -> Option<sb_expr::Value> {
            lookup(id).or_else(|| {
                if id != "true" && id != "false" {
                    // Recorded below; Iris treats unknown names as true.
                    Some(sb_expr::Value::Bool(true))
                } else {
                    None
                }
            })
        };
        if let Ok(parsed) = sb_expr::parse(expr) {
            collect_identifiers(&parsed, &mut |id| {
                if lookup(id).is_none() && !matches!(id, "true" | "false" | "pi") {
                    unknown.push(id.to_string());
                }
            });
        }
        let loc = SourceLocation::new(sb_pack::shaders_properties::FILE, 1);
        for u in unknown {
            diags.push(
                Diagnostic::warning(
                    "props.enabled-unknown-option",
                    format!("program.{path}.enabled: `{u}` is not an option of this pack; treated as true (Iris)"),
                )
                .at(loc.clone()),
            );
        }
        match sb_expr::eval_bool_with(expr, &full) {
            Ok(v) => v,
            Err(e) => {
                diags.push(
                    Diagnostic::warning(
                        "props.enabled-expression",
                        format!("program.{path}.enabled = `{expr}` cannot be evaluated ({}); the program stays enabled", e.message),
                    )
                    .at(loc),
                );
                true
            }
        }
    }
}

/// Visit every identifier of an expression.
fn collect_identifiers(e: &sb_expr::Expr, f: &mut dyn FnMut(&str)) {
    use sb_expr::ExprKind as K;
    match &e.kind {
        K::Ident(name) => f(name),
        K::Unary { operand, .. } => collect_identifiers(operand, f),
        K::Binary { lhs, rhs, .. } => {
            collect_identifiers(lhs, f);
            collect_identifiers(rhs, f);
        }
        K::Call { args, .. } => args.iter().for_each(|a| collect_identifiers(a, f)),
        K::Member { base, .. } => collect_identifiers(base, f),
        _ => {}
    }
}

/// Resolve the programs of `folder`.
pub fn resolve_folder(pack: &ShaderPack, folder: &str, load: &PackLoad, env: &CompileEnvironment) -> FolderPlan {
    let set = pack.program_set(folder);
    let mut plan = FolderPlan { folder: folder.to_string(), ..Default::default() };
    plan.diagnostics.extend(set.diagnostics.iter().cloned());
    let enabler = Enabler::new(load, folder);
    let tessellation = load.feature_active("TESSELLATION_SHADERS");
    let mut diags = Diagnostics::new();

    // Graphics program unit (None if absent, incomplete or disabled).
    let graphics_unit = |ps: &sb_pack::ProgramSources, kind: ProgramKind, plan: &mut FolderPlan, diags: &mut Diagnostics| {
        let name = ps.file_name();
        if !ps.has_graphics() {
            return None;
        }
        if !enabler.enabled(&name, diags) {
            plan.disabled.push(program_path(folder, &name));
            return None;
        }
        let Some(frag) = ps.stage(ShaderStage::Fragment) else {
            diags.push(
                Diagnostic::info("pipeline.program-incomplete", format!("{} has no fragment shader; it is ignored (Iris treats it as absent)", program_path(folder, &name)))
                    .in_program(program_path(folder, &name)),
            );
            return None;
        };
        let mut stages: Vec<(ShaderStage, StageSource)> = Vec::new();
        match ps.stage(ShaderStage::Vertex) {
            Some(v) => stages.push((ShaderStage::Vertex, StageSource::File(v.to_string()))),
            None => {
                let virtual_path = format!("{}.vsh", frag.strip_suffix(".fsh").unwrap_or(frag));
                diags.push(
                    Diagnostic::info("pipeline.synthesized-vertex", format!("{} has no vertex shader; Iris's default `ftransform()` vertex shader is used", program_path(folder, &name)))
                        .in_program(program_path(folder, &name)),
                );
                stages.push((ShaderStage::Vertex, StageSource::SynthesizedVertex { virtual_path }));
            }
        }
        for st in [ShaderStage::TessControl, ShaderStage::TessEval] {
            if let Some(f) = ps.stage(st) {
                if tessellation {
                    stages.push((st, StageSource::File(f.to_string())));
                } else {
                    diags.push(
                        Diagnostic::info("pipeline.tessellation-ignored", format!("`{f}` is ignored: the pack does not declare the TESSELLATION_SHADERS feature (Iris)"))
                            .in_program(program_path(folder, &name)),
                    );
                }
            }
        }
        if let Some(g) = ps.stage(ShaderStage::Geometry) {
            stages.push((ShaderStage::Geometry, StageSource::File(g.to_string())));
        }
        stages.push((ShaderStage::Fragment, StageSource::File(frag.to_string())));
        plan.units.push(Unit { name: name.clone(), props_name: name.clone(), path: program_path(folder, &name), kind, stages });
        Some(plan.units.len() - 1)
    };

    // Compute units of a program, in dispatch order.
    let compute_units = |ps: &sb_pack::ProgramSources,
                         kind_of: &dyn Fn(Option<char>) -> ProgramKind,
                         plan: &mut FolderPlan,
                         diags: &mut Diagnostics|
     -> Vec<usize> {
        let base = ps.file_name();
        let mut out = Vec::new();
        for (letter, file) in &ps.computes {
            let (name, props_name) = match letter {
                None => (format!("{base}.csh"), base.clone()),
                Some(c) => (format!("{base}_{c}"), format!("{base}_{c}")),
            };
            // `composite3.csh` is disabled with `composite3`; lettered computes have their
            // own `program.composite3_a.enabled`. A disabled lettered compute ends the
            // chain (Iris stops at the first missing letter).
            if !enabler.enabled(&props_name, diags) {
                plan.disabled.push(program_path(folder, &name));
                if letter.is_some() {
                    break;
                }
                continue;
            }
            plan.units.push(Unit {
                name: name.clone(),
                props_name,
                path: program_path(folder, &name),
                kind: kind_of(*letter),
                stages: vec![(ShaderStage::Compute, StageSource::File(file.clone()))],
            });
            out.push(plan.units.len() - 1);
        }
        out
    };

    for ps in set.programs.values() {
        match ps.name {
            ProgramName::Geometry { program } => {
                if program.group() == GeometryGroup::DistantHorizons && !env.distant_horizons {
                    continue;
                }
                if let Some(u) = graphics_unit(ps, ProgramKind::Geometry { program }, &mut plan, &mut diags) {
                    plan.geometry.insert(program, u);
                }
                if program == GeometryProgram::Shadow {
                    let kind_of = |letter: Option<char>| ProgramKind::GeometryCompute { program, letter };
                    plan.shadow_computes = compute_units(ps, &kind_of, &mut plan, &mut diags);
                }
            }
            ProgramName::Composite { group, index } => {
                let program = if group == PassGroup::Setup {
                    if ps.has_graphics() {
                        diags.push(Diagnostic::info(
                            "pipeline.setup-graphics",
                            format!("{}: setup programs are compute-only; its graphics stages are ignored", program_path(folder, &ps.file_name())),
                        ));
                    }
                    None
                } else {
                    graphics_unit(ps, ProgramKind::Composite { group, index }, &mut plan, &mut diags)
                };
                let kind_of = |letter: Option<char>| ProgramKind::Compute { group, index, letter };
                let computes = compute_units(ps, &kind_of, &mut plan, &mut diags);
                if program.is_some() || !computes.is_empty() {
                    plan.groups.entry(group).or_default().push(SlotPlan { index, program, computes });
                }
            }
        }
    }
    for slots in plan.groups.values_mut() {
        slots.sort_by_key(|s| s.index);
    }
    plan.diagnostics.extend(diags);
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::load;
    use sb_pack::OptionValues;

    fn plan_of(files: &[(&str, &str)], values: &[(&str, &str)], env: &CompileEnvironment, folder: &str) -> FolderPlan {
        let pack = ShaderPack::from_files("t", files.iter().map(|(a, b)| (*a, *b)));
        let l = load(&pack, env, &OptionValues::from_pairs(values.iter().map(|(a, b)| (*a, *b))), "en_us", None);
        resolve_folder(&pack, folder, &l, env)
    }

    fn names(p: &FolderPlan) -> Vec<String> {
        p.units.iter().map(|u| u.name.clone()).collect()
    }

    #[test]
    fn programs_computes_and_enabled() {
        let files = [
            ("shaders.properties", "program.composite1.enabled = BLOOM && !FOG\nprogram.composite2_b.enabled=false\nprogram.gbuffers_water.enabled = NOT_AN_OPTION\n"),
            ("lib.glsl", "#define BLOOM\n#ifdef BLOOM\n#endif\n//#define FOG\n#ifdef FOG\n#endif\n"),
            ("composite.fsh", "#include \"lib.glsl\"\nvoid main(){}"),
            ("composite.vsh", "void main(){}"),
            ("composite1.fsh", "void main(){}"),
            ("composite2.csh", "void main(){}"),
            ("composite2_a.csh", "void main(){}"),
            ("composite2_b.csh", "void main(){}"),
            ("composite2_c.csh", "void main(){}"),
            ("gbuffers_terrain.fsh", "void main(){}"),
            ("gbuffers_terrain.vsh", "void main(){}"),
            ("gbuffers_water.fsh", "void main(){}"),
            ("gbuffers_basic.vsh", "void main(){}"),
            ("shadow.fsh", "void main(){}"),
            ("shadow_a.csh", "void main(){}"),
            ("setup.csh", "void main(){}"),
            ("setup1.csh", "void main(){}"),
            ("final.fsh", "void main(){}"),
            ("dh_terrain.fsh", "void main(){}"),
        ];
        let env = CompileEnvironment::default();
        let p = plan_of(&files, &[], &env, "");
        assert_eq!(
            names(&p),
            vec![
                "composite", "composite1", "composite2.csh", "composite2_a", "dh_terrain", "final", "gbuffers_terrain",
                "gbuffers_water", "setup.csh", "setup1.csh", "shadow", "shadow_a"
            ]
        );
        // composite2_b disabled: the chain stops (composite2_c is not loaded either).
        assert!(p.disabled.contains(&"composite2_b".to_string()));
        // gbuffers_basic has no fragment shader.
        assert!(!p.geometry.contains_key(&GeometryProgram::Basic));
        assert!(p.diagnostics.iter().any(|d| d.code == "props.enabled-unknown-option"));
        let comp = &p.groups[&PassGroup::Composite];
        assert_eq!(comp.iter().map(|s| s.index).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(comp[2].program, None);
        assert_eq!(comp[2].computes.len(), 2);
        assert_eq!(p.groups[&PassGroup::Setup].len(), 2);
        assert_eq!(p.shadow_computes.len(), 1);
        // Missing vertex shader is synthesized.
        let water = &p.units[p.geometry[&GeometryProgram::Water]];
        assert!(matches!(water.stages[0].1, StageSource::SynthesizedVertex { .. }));
        assert_eq!(water.stages[0].1.path(), "gbuffers_water.vsh");
        // Value-driven disabling.
        let p = plan_of(&files, &[("FOG", "true")], &env, "");
        assert!(!names(&p).contains(&"composite1".to_string()));
        // DH programs are skipped without Distant Horizons.
        let no_dh = CompileEnvironment { distant_horizons: false, ..Default::default() };
        let p = plan_of(&files, &[], &no_dh, "");
        assert!(!p.geometry.contains_key(&GeometryProgram::DhTerrain));
    }

    #[test]
    fn profile_toggles_and_folder_paths() {
        let files = [
            ("shaders.properties", "profile.LOW=!program.world0/composite !BLOOM\nprofile.HIGH=BLOOM\nprogram.world0/deferred.enabled=false\n"),
            ("world0/composite.fsh", "#define BLOOM\n#ifdef BLOOM\n#endif\nvoid main(){}"),
            ("world0/deferred.fsh", "void main(){}"),
            ("world0/final.fsh", "void main(){}"),
        ];
        let env = CompileEnvironment::default();
        // Default values match HIGH: everything but deferred.
        let p = plan_of(&files, &[], &env, "world0");
        assert_eq!(names(&p), vec!["composite", "final"]);
        assert_eq!(p.units[0].path, "world0/composite");
        // BLOOM=false matches LOW, which disables world0/composite.
        let p = plan_of(&files, &[("BLOOM", "false")], &env, "world0");
        assert_eq!(names(&p), vec!["final"]);
    }

    #[test]
    fn tessellation_needs_feature() {
        let files = [("gbuffers_terrain.fsh", "x"), ("gbuffers_terrain.vsh", "x"), ("gbuffers_terrain.tcs", "x"), ("gbuffers_terrain.tes", "x")];
        let env = CompileEnvironment::default();
        let p = plan_of(&files, &[], &env, "");
        assert_eq!(p.units[0].stages.len(), 2);
        let mut with = files.to_vec();
        with.push(("shaders.properties", "iris.features.optional=TESSELATION_SHADERS"));
        let p = plan_of(&with, &[], &env, "");
        assert_eq!(p.units[0].stages.iter().map(|s| s.0).collect::<Vec<_>>(), vec![
            ShaderStage::Vertex,
            ShaderStage::TessControl,
            ShaderStage::TessEval,
            ShaderStage::Fragment
        ]);
    }
}
