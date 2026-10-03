//! Building one [`DimensionPipeline`] (spec steps 7–14 for one program folder).

use crate::analysis::{self, FolderAnalysis, FolderDirectives};
use crate::bindings::{self, LayoutBuild};
use crate::compile::{Caches, CompiledVariant, JobEnv, VariantJob, compile_job};
use crate::flips::{self, PassInput, Schedule, ScheduleInput};
use crate::load::{FolderInfo, PackLoad};
use crate::resolve::{FolderPlan, Unit};
use crate::{BlobStore, CompileSettings};
use indexmap::IndexMap;
use rayon::prelude::*;
use sb_core::model::{
    AlphaTest, BindingTable, BindingUse, ColorTarget, ComputeInfo, DhPipeline, DhStrategy, DimensionPipeline, GeometrySlot,
    PackSettings, Program, ProgramKind, RenderTargets, ResourceRef, ShadowSettings, StageModule, TargetSize, UniformLayout,
    WorkGroups,
};
use sb_core::program::{GeometryGroup, GeometryProgram, PassGroup};
use sb_core::{Diagnostic, Diagnostics, TextureFormat};
use sb_pack::ShaderPack;
use sb_pack::shaders_properties::program_path;
use sb_transform::{DrawProfile, FULLSCREEN_PROFILE, PackContext};
use sb_uniforms::{MemberIndex, ProgramClass, ResourceContext, UniformDecl};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Draw profiles: host-registered ones first, then the built-in ones.
pub struct Profiles<'a> {
    pub extra: &'a [DrawProfile],
}

impl Profiles<'_> {
    pub fn get(&self, name: &str) -> Option<&DrawProfile> {
        self.extra.iter().find(|p| p.name == name).or_else(|| sb_transform::profile(name))
    }
}

/// Inputs shared by every folder of a compile.
pub struct FolderInputs<'a> {
    pub pack: &'a ShaderPack,
    pub sources: &'a dyn sb_core::SourceProvider,
    pub load: &'a PackLoad,
    pub settings: &'a CompileSettings,
    pub caches: &'a Caches,
    pub profiles: Profiles<'a>,
}

/// Identity of a compiled (program, profile) variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VariantKey {
    pub unit: usize,
    pub profile: String,
    pub class: ProgramClass,
    pub shadow_pass: bool,
}

/// How a variant appears in the model.
#[derive(Debug, Clone)]
pub struct VariantSpec {
    pub key: VariantKey,
    pub name: String,
    pub kind: ProgramKind,
    pub synthesized_from: Option<String>,
    /// Name for per-program `shaders.properties` keys.
    pub props_name: String,
    /// Geometry program whose defaults (blend, alpha test, cull) apply.
    pub identity: Option<GeometryProgram>,
    pub draw_profile: Option<String>,
    pub draw_buffers: Vec<u32>,
    pub output_slots: Vec<u32>,
    pub alpha_test: Option<AlphaTest>,
    pub profile_constants: IndexMap<String, i64>,
    /// Sort key for the model's program order.
    order: (u8, u32, u32),
}

/// A geometry slot to resolve (fallback chain).
#[derive(Debug, Clone)]
struct SlotRequest {
    slot: Option<GeometryProgram>,
    candidates: Vec<usize>,
    profile: String,
    class: ProgramClass,
    shadow_pass: bool,
    /// Synthesized DH program identity.
    synth: Option<GeometryProgram>,
    order: (u8, u32),
}

/// State of a compiled folder kept by a session (for [`crate::compile_variant`]).
#[derive(Debug, Clone)]
pub struct FolderState {
    pub info: FolderInfo,
    pub plan: FolderPlan,
    pub analysis: FolderAnalysis,
    pub directives: FolderDirectives,
    pub layout: UniformLayout,
    pub members: MemberIndex,
    pub bindings: BindingTable,
    pub contexts: Vec<ResourceContext>,
    pub image_formats: IndexMap<String, String>,
    pub dh_constants: IndexMap<String, i64>,
    pub gbuffer_attachments: Vec<u32>,
    pub shadow_attachments: Vec<u32>,
    pub schedule: Schedule,
}

/// Per-folder results.
#[derive(Debug, Clone, Default)]
pub struct FolderStats {
    pub programs_ok: usize,
    pub programs_failed: Vec<String>,
    pub modules: usize,
    pub modules_validated: usize,
}

/// The result of one folder.
pub struct FolderResult {
    pub pipeline: DimensionPipeline,
    pub diagnostics: Diagnostics,
    pub stats: FolderStats,
    pub state: FolderState,
}

/// The geometry pass a slot draws in.
pub(crate) fn phase_of_slot(g: GeometryProgram) -> PassGroup {
    if g.group() == GeometryGroup::Shadow || g == GeometryProgram::DhShadow {
        PassGroup::Shadow
    } else if g.is_translucent() {
        PassGroup::GbuffersTranslucent
    } else {
        PassGroup::GbuffersOpaque
    }
}

pub(crate) fn is_shadow_group(g: GeometryProgram) -> bool {
    g.group() == GeometryGroup::Shadow || g == GeometryProgram::DhShadow
}

/// `backFace.*` → cull override of programs dedicated to one render layer.
fn cull_of(identity: Option<GeometryProgram>, settings: &PackSettings) -> Option<bool> {
    let key = match identity? {
        GeometryProgram::TerrainSolid => "solid",
        GeometryProgram::TerrainCutout => "cutout",
        GeometryProgram::Water | GeometryProgram::BlockTranslucent => "translucent",
        _ => return None,
    };
    settings.back_face.get(key).map(|render_back_faces| !render_back_faces)
}

/// Group rank of a composite-style program kind (for ordering).
fn group_rank(g: PassGroup) -> u32 {
    flips::GROUP_ORDER.iter().position(|x| *x == g).unwrap_or(0) as u32
}

/// Build one folder.
pub fn build_folder(input: &FolderInputs<'_>, info: &FolderInfo, blobs: &mut BlobStore) -> FolderResult {
    let load = input.load;
    let props = &load.props;
    let env = &input.settings.env;
    let mut diags = Diagnostics::new();
    let mut stats = FolderStats::default();

    // ---- Resolution, preprocessing, analysis, directives -------------------------------
    let plan = crate::resolve::resolve_folder(input.pack, &info.folder, load, env);
    diags.extend(plan.diagnostics.iter().cloned());
    let pp_opts = sb_preprocess::PreprocessOptions { defines: load.glsl_macros.clone(), ..Default::default() };
    let analysis = analysis::analyze_folder(&plan, input.sources, &pp_opts, &input.caches.analysis, &mut diags);
    let directives = analysis::scan_directives(&plan, &analysis, env.device.max_color_attachments, &mut diags);
    let ok = |u: usize| analysis.ok(u);

    // ---- DH strategy -----------------------------------------------------------------------
    let has_native_dh = [GeometryProgram::DhTerrain, GeometryProgram::DhWater]
        .iter()
        .any(|g| plan.geometry.get(g).is_some_and(|&u| ok(u)));
    let strategy = if !env.distant_horizons {
        DhStrategy::Disabled
    } else if has_native_dh {
        DhStrategy::Native
    } else {
        DhStrategy::Synthesized
    };

    // ---- Shared attachments (13b) ----------------------------------------------------------
    let max_attachments = env.device.max_color_attachments.clamp(1, 8) as usize;
    let mut gb_union = BTreeSet::new();
    let mut sh_union = BTreeSet::new();
    for (&g, &u) in &plan.geometry {
        if !ok(u) {
            continue;
        }
        if is_shadow_group(g) {
            sh_union.extend(directives.draw_buffers[u].iter().copied());
        } else {
            gb_union.extend(directives.draw_buffers[u].iter().copied());
        }
    }
    // One shared gbuffer pass binds every attachment at once, so they must have one size:
    // Iris binds a framebuffer of each program's own draw buffers, and a `size.buffer`
    // target only clips the programs that write it (bloop draws the sky into a 0.75-scaled
    // colortex8 with the full-screen viewport).
    let size_of = |i: u32| props.buffer_size_of(i).and_then(|s| s.to_target_size()).unwrap_or_default();
    let gb_sizes: Vec<TargetSize> = gb_union.iter().map(|&i| size_of(i)).collect();
    let gb_one_size = gb_sizes.windows(2).all(|w| w[0] == w[1]);
    let gb_shared = gb_union.len() <= max_attachments && gb_one_size;
    let gbuffer_attachments: Vec<u32> = if gb_shared { gb_union.into_iter().collect() } else { Vec::new() };
    let shadow_attachments: Vec<u32> = if sh_union.len() <= max_attachments { sh_union.into_iter().collect() } else { Vec::new() };
    if gbuffer_attachments.is_empty() && plan.geometry.keys().any(|g| !is_shadow_group(*g)) {
        let why = if gb_one_size {
            format!("gbuffers programs write more than {max_attachments} distinct buffers")
        } else {
            "gbuffers programs write buffers of different sizes (`size.buffer`)".to_string()
        };
        diags.push(Diagnostic::info("pipeline.gbuffer-attachments", format!("{why}; no shared gbuffer pass (per-program attachments)")));
    }

    // ---- Pre-compile schedule (flippedAtLeastOnce for custom textures) ----------------------
    let cleared = |i: u32| directives.pack.colortex.get(&i).and_then(|b| b.clear).unwrap_or(true);
    let pre_schedule = {
        let mut si = schedule_input(&plan, &directives, props, &|u| ok(u).then_some(u as u32));
        si.cleared = (0..sb_uniforms::MAX_COLOR_TEX).filter(|i| cleared(*i)).collect();
        flips::schedule(&si)
    };
    let flipped_once_of = |u: usize| -> BTreeSet<u32> {
        pre_schedule
            .passes
            .iter()
            .position(|p| p.program == Some(u as u32) || p.computes.contains(&(u as u32)))
            .map(|i| pre_schedule.flipped_at_least_once[i].clone())
            .unwrap_or_default()
    };

    // ---- Resource contexts, layout and binding table ---------------------------------------
    let watershadow = analysis.units.iter().flatten().any(|s| {
        s.analyzed.as_ref().is_ok_and(|a| a.info.opaque_uniforms.iter().any(|o| o.name == "watershadow"))
    });
    let contexts: Vec<ResourceContext> = plan
        .units
        .iter()
        .enumerate()
        .map(|(u, unit)| {
            bindings::resource_context(bindings::class_of(unit), bindings::texture_stage_of(unit), props, watershadow, &flipped_once_of(u))
        })
        .collect();
    let mut lb = LayoutBuild::new();
    for (u, unit) in plan.units.iter().enumerate() {
        lb.add_uniforms(unit, &analysis, u);
        lb.add_resources(&analysis, u, &contexts[u]);
        // Geometry units may also be compiled as synthesized DH programs.
        if strategy == DhStrategy::Synthesized && unit.geometry().is_some() {
            lb.add_resources(&analysis, u, &contexts[u].for_class(ProgramClass::Dh));
        }
    }

    // Slot requests.
    let mut requests: Vec<SlotRequest> = Vec::new();
    let chain_units = |g: GeometryProgram| -> Vec<usize> {
        g.chain().filter_map(|p| plan.geometry.get(&p).copied()).filter(|&u| ok(u)).collect()
    };
    for (i, &g) in GeometryProgram::ALL.iter().enumerate() {
        if g.group() == GeometryGroup::DistantHorizons {
            continue;
        }
        let candidates = chain_units(g);
        if candidates.is_empty() {
            continue;
        }
        requests.push(SlotRequest {
            slot: Some(g),
            candidates,
            profile: sb_transform::default_profile_for(g).to_string(),
            class: ProgramClass::from_geometry(g),
            shadow_pass: is_shadow_group(g),
            synth: None,
            order: (0, i as u32),
        });
    }
    let shadow_on_for_dh = props.shadow_enabled != Some(false)
        && props.dh_shadow_enabled != Some(false)
        && plan.geometry.get(&GeometryProgram::Shadow).is_some_and(|&u| ok(u));
    match strategy {
        DhStrategy::Native => {
            for (i, g) in [GeometryProgram::DhTerrain, GeometryProgram::DhWater, GeometryProgram::DhGeneric, GeometryProgram::DhShadow]
                .into_iter()
                .enumerate()
            {
                let candidates = chain_units(g);
                if candidates.is_empty() || (g == GeometryProgram::DhShadow && props.dh_shadow_enabled == Some(false)) {
                    continue;
                }
                requests.push(SlotRequest {
                    slot: Some(g),
                    candidates,
                    profile: sb_transform::default_profile_for(g).to_string(),
                    class: ProgramClass::Dh,
                    shadow_pass: g == GeometryProgram::DhShadow,
                    synth: None,
                    order: (1, i as u32),
                });
            }
        }
        DhStrategy::Synthesized => {
            // Programs synthesized from gbuffers_terrain/gbuffers_water/shadow get the
            // `dh_terrain` variant whose lightmap follows the vanilla terrain convention
            // those sources were written for (DH_SYNTH_PROFILE); dh_generic keeps its
            // own profile.
            let specs = [
                (GeometryProgram::DhTerrain, GeometryProgram::Terrain, sb_transform::DH_SYNTH_PROFILE, false),
                (GeometryProgram::DhWater, GeometryProgram::Water, sb_transform::DH_SYNTH_PROFILE, false),
                (GeometryProgram::DhGeneric, GeometryProgram::Terrain, "dh_generic", false),
                (GeometryProgram::DhShadow, GeometryProgram::Shadow, sb_transform::DH_SYNTH_PROFILE, true),
            ];
            for (i, (dh, source, synth_profile, shadow)) in specs.into_iter().enumerate() {
                if shadow && !shadow_on_for_dh {
                    continue;
                }
                // A pack shipping only dh_shadow keeps it (with the native DH profile).
                let native = plan.geometry.get(&dh).copied().filter(|&u| ok(u));
                let (candidates, synth, profile) = match native {
                    Some(u) => (vec![u], None, sb_transform::default_profile_for(dh)),
                    None => (chain_units(source), Some(dh), synth_profile),
                };
                if candidates.is_empty() {
                    continue;
                }
                requests.push(SlotRequest {
                    slot: Some(dh),
                    candidates,
                    profile: profile.to_string(),
                    class: ProgramClass::Dh,
                    shadow_pass: shadow,
                    synth,
                    order: (1, i as u32),
                });
            }
        }
        DhStrategy::Disabled => {}
    }
    for (i, (g, profiles)) in input.settings.profile_overrides.iter().enumerate() {
        let candidates = chain_units(*g);
        if candidates.is_empty() {
            continue;
        }
        for (j, p) in profiles.iter().enumerate() {
            requests.push(SlotRequest {
                slot: None,
                candidates: candidates.clone(),
                profile: p.clone(),
                class: if g.group() == GeometryGroup::DistantHorizons { ProgramClass::Dh } else { ProgramClass::from_geometry(*g) },
                shadow_pass: is_shadow_group(*g),
                synth: None,
                order: (3, (i * 64 + j) as u32),
            });
        }
    }

    // Profiles used → layout and bindings.
    let mut profiles_used: BTreeMap<(String, ProgramClass), ()> = BTreeMap::new();
    for r in &requests {
        profiles_used.insert((r.profile.clone(), r.class), ());
    }
    if plan.units.iter().any(|u| matches!(u.kind, ProgramKind::Composite { .. })) {
        profiles_used.insert((FULLSCREEN_PROFILE.to_string(), ProgramClass::Fullscreen), ());
    }
    for p in input.settings.extra_profiles.iter() {
        let class = if p.name.starts_with("dh_") { ProgramClass::Dh } else { ProgramClass::Gbuffers };
        profiles_used.insert((p.name.clone(), class), ());
    }
    let base_ctx = bindings::resource_context(ProgramClass::Gbuffers, "gbuffers", props, watershadow, &BTreeSet::new());
    for (name, class) in profiles_used.keys() {
        match input.profiles.get(name) {
            Some(p) => lb.add_profile(p, &base_ctx.for_class(*class)),
            None => diags.push(Diagnostic::error("pipeline.unknown-profile", format!("draw profile `{name}` does not exist"))),
        }
    }

    // Custom uniforms.
    let constants = crate::macros::expression_constants();
    let (custom, cd) = sb_expr::CustomUniforms::compile(&props.custom_uniforms, &|n: &str| sb_uniforms::custom_uniform_input_type(n), &constants);
    diags.extend(cd.into_iter().map(|d| if d.location.is_none() { d.at(sb_core::SourceLocation::new(sb_pack::shaders_properties::FILE, 1)) } else { d }));
    for name in custom.referenced_inputs() {
        if let Some(b) = sb_uniforms::get(&name) {
            lb.add_uniform(UniformDecl::from_registry(name, b.ty));
        }
    }
    for (name, ty) in custom.outputs() {
        lb.add_uniform(UniformDecl::custom(name, ty));
    }
    let (layout, members, mut binding_table, ld) = lb.finish();
    diags.extend(ld);

    // ---- Image formats and DH constants ----------------------------------------------------
    let colortex_format = |i: u32| directives.pack.colortex.get(&i).and_then(|b| b.format).unwrap_or(TextureFormat::RGBA);
    let shadowcolor_format = |i: u32| directives.pack.shadowcolor.get(&i).and_then(|b| b.format).unwrap_or(TextureFormat::RGBA);
    let image_formats = bindings::image_formats(&colortex_format, &shadowcolor_format, &props.images);
    let dh_constants = crate::dh::dh_block_constants(&load.id_maps);

    // ---- Variant specs ---------------------------------------------------------------------
    let spec_inputs = SpecInputs {
        folder: &info.folder,
        plan: &plan,
        directives: &directives,
        gbuffer_attachments: &gbuffer_attachments,
        shadow_attachments: &shadow_attachments,
        dh_constants: &dh_constants,
        props,
    };
    let make_spec = |key: &VariantKey, synth: Option<GeometryProgram>, order: (u8, u32, u32)| spec_inputs.spec(key, synth, order);
    let mut specs: IndexMap<VariantKey, VariantSpec> = IndexMap::new();
    // Composite-style programs and computes.
    for (group, slots) in &plan.groups {
        for slot in slots {
            for (k, u) in slot.program.iter().chain(slot.computes.iter()).enumerate() {
                if !ok(*u) {
                    continue;
                }
                let unit = &plan.units[*u];
                let key = VariantKey {
                    unit: *u,
                    profile: if unit.is_compute() { String::new() } else { FULLSCREEN_PROFILE.to_string() },
                    class: bindings::class_of(unit),
                    shadow_pass: false,
                };
                let spec = make_spec(&key, None, (2, group_rank(*group) * 1000 + u32::from(slot.index), k as u32));
                specs.insert(key, spec);
            }
        }
    }
    for (k, &u) in plan.shadow_computes.iter().enumerate() {
        if ok(u) {
            let key = VariantKey { unit: u, profile: String::new(), class: ProgramClass::Shadow, shadow_pass: true };
            let spec = make_spec(&key, None, (2, group_rank(PassGroup::Shadow) * 1000, k as u32));
            specs.insert(key, spec);
        }
    }

    // ---- Compile rounds with fallback ------------------------------------------------------
    let mut compiled: HashMap<VariantKey, Result<CompiledVariant, Diagnostics>> = HashMap::new();
    let fullscreen = input.profiles.get(FULLSCREEN_PROFILE);
    let mut pending: Vec<VariantKey> = specs.keys().cloned().collect();
    for round in 0..8 {
        if round > 0 {
            pending.clear();
        }
        for r in &requests {
            for &u in &r.candidates {
                let key = VariantKey { unit: u, profile: r.profile.clone(), class: r.class, shadow_pass: r.shadow_pass };
                match compiled.get(&key) {
                    Some(Ok(_)) => break,
                    Some(Err(_)) => continue,
                    None => {
                        if !specs.contains_key(&key) {
                            let spec = make_spec(&key, r.synth, (if r.slot.is_some() { r.order.0 } else { 3 }, r.order.1, u as u32));
                            specs.insert(key.clone(), spec);
                        }
                        if !pending.contains(&key) {
                            pending.push(key);
                        }
                        break;
                    }
                }
            }
        }
        if pending.is_empty() {
            break;
        }
        let results: Vec<(VariantKey, Result<CompiledVariant, Diagnostics>)> = pending
            .par_iter()
            .map(|key| {
                let spec = &specs[key];
                let unit = &plan.units[key.unit];
                let profile = if key.profile.is_empty() { fullscreen } else { input.profiles.get(&key.profile) };
                let Some(profile) = profile else {
                    return (
                        key.clone(),
                        Err(Diagnostics(vec![Diagnostic::error("pipeline.unknown-profile", format!("draw profile `{}` does not exist", key.profile)).in_program(spec.name.clone())])),
                    );
                };
                let Some(stages) = analysis.stages_of(key.unit) else {
                    return (key.clone(), Err(Diagnostics::new()));
                };
                let ctx = contexts[key.unit].for_class(key.class);
                let jenv = JobEnv {
                    env,
                    validate: input.settings.validate_spirv,
                    caches: input.caches,
                    pack: PackContext { layout: &layout, members: &members, bindings: &binding_table, resources: &ctx },
                    image_formats: &image_formats,
                };
                let job = VariantJob {
                    name: spec.name.clone(),
                    stages,
                    files: unit.stages.iter().map(|(s, src)| (*s, src.path().to_string())).collect(),
                    profile,
                    class: key.class,
                    shadow_pass: key.shadow_pass,
                    alpha_test: spec.alpha_test,
                    output_locations: spec.output_slots.clone(),
                    profile_constants: spec.profile_constants.clone(),
                    kind: spec.kind.clone(),
                };
                (key.clone(), compile_job(&job, &jenv))
            })
            .collect();
        for (k, r) in results {
            compiled.insert(k, r);
        }
    }

    // ---- Diagnostics of variants -----------------------------------------------------------
    for (key, spec) in &specs {
        match compiled.get(key) {
            Some(Ok(c)) => diags.extend(c.diagnostics.iter().cloned()),
            Some(Err(d)) => {
                diags.extend(analysis::tag(d.iter().cloned(), &spec.name, None));
                stats.programs_failed.push(format!("{} [{}]", spec.name, spec.draw_profile.as_deref().unwrap_or("compute")));
                if !d.has_errors() {
                    diags.push(Diagnostic::error("pipeline.program-failed", "the program failed without an error message").in_program(spec.name.clone()));
                }
            }
            None => {}
        }
    }
    // Units that failed analysis count as failed programs.
    for (u, unit) in plan.units.iter().enumerate() {
        if !ok(u) {
            stats.programs_failed.push(format!("{} [analysis]", unit.path));
        }
    }

    // ---- Model programs --------------------------------------------------------------------
    let mut ok_keys: Vec<&VariantKey> = specs.keys().filter(|k| matches!(compiled.get(*k), Some(Ok(_)))).collect();
    ok_keys.sort_by_key(|k| specs[*k].order);
    let mut index_of: HashMap<VariantKey, u32> = HashMap::new();
    let mut programs: Vec<Program> = Vec::new();
    let back_settings = props.settings();
    for key in ok_keys {
        let spec = &specs[key];
        let Some(Ok(c)) = compiled.get(key) else { continue };
        stats.programs_ok += 1;
        stats.modules += c.stages.len();
        stats.modules_validated += c.validated_modules as usize;
        let program = model_program(spec, c, &plan.units[key.unit], &directives, props, &binding_table, &back_settings, env, &mut diags, blobs);
        index_of.insert(key.clone(), programs.len() as u32);
        programs.push(program);
    }

    // Geometry slots.
    let mut geometry: IndexMap<GeometryProgram, GeometrySlot> = IndexMap::new();
    let mut slot_phase: Vec<(GeometryProgram, u32)> = Vec::new();
    for r in &requests {
        let Some(g) = r.slot else { continue };
        let found = r.candidates.iter().enumerate().find_map(|(i, &u)| {
            let key = VariantKey { unit: u, profile: r.profile.clone(), class: r.class, shadow_pass: r.shadow_pass };
            index_of.get(&key).map(|idx| (i, u, *idx))
        });
        let Some((i, u, idx)) = found else {
            diags.push(
                Diagnostic::warning("pipeline.slot-unresolved", format!("no program of the fallback chain of {} compiled; that geometry renders unshaded", g.file_name()))
                    .in_program(program_path(&info.folder, g.file_name())),
            );
            continue;
        };
        if i > 0 {
            diags.push(
                Diagnostic::warning(
                    "pipeline.fallback",
                    format!("{} uses {} because {} failed to compile", g.file_name(), plan.units[u].name, plan.units[r.candidates[0]].name),
                )
                .in_program(program_path(&info.folder, g.file_name())),
            );
        }
        // A synthesized DH program may be shared (dh_water → dh_terrain): report the
        // program actually used.
        let resolved_from = match (&programs[idx as usize].kind, r.synth) {
            (ProgramKind::Geometry { program }, Some(_)) => *program,
            _ => plan.units[u].geometry().unwrap_or(g),
        };
        geometry.insert(g, GeometrySlot { program: idx, resolved_from });
        slot_phase.push((g, idx));
    }

    // ---- Final schedule --------------------------------------------------------------------
    let model_index = |u: usize| -> Option<u32> {
        let unit = &plan.units[u];
        let key = VariantKey {
            unit: u,
            profile: if unit.is_compute() { String::new() } else { FULLSCREEN_PROFILE.to_string() },
            class: bindings::class_of(unit),
            shadow_pass: matches!(unit.kind, ProgramKind::GeometryCompute { .. }),
        };
        index_of.get(&key).copied()
    };
    let colortex_used = colortex_used(&programs, &binding_table, props);
    let mut si = schedule_input(&plan, &directives, props, &model_index);
    si.colortex_count = colortex_used.iter().max().map_or(1, |m| m + 1);
    si.cleared = (0..sb_uniforms::MAX_COLOR_TEX).filter(|i| cleared(*i)).collect();
    let schedule = flips::schedule(&si);

    // use_alt of colortex/colorimg bindings, per pass state (programs used in passes with
    // different states are duplicated).
    assign_use_alt(&mut programs, &mut geometry, &slot_phase, &schedule, &binding_table);

    // ---- Targets and settings --------------------------------------------------------------
    let shadow_programs = geometry.iter().any(|(g, _)| g.group() == GeometryGroup::Shadow);
    let samples_shadow = programs.iter().any(|p| {
        p.bindings_used.iter().any(|b| binding_table.get(&b.name).is_some_and(|e| bindings::is_shadow_resource(&e.resource)))
    });
    let mut shadow = directives.pack.shadow.clone();
    shadow.enabled = shadow_programs || samples_shadow;
    props.apply_shadow_settings(&mut shadow);
    shadow.color_mipmap.resize(8, false);
    shadow.color_nearest.resize(8, false);
    let targets = render_targets(&programs, &binding_table, &directives, props, input.pack, &colortex_used, shadow, &mut diags, load);
    let mut settings = props.settings();
    settings.sun_path_rotation = directives.pack.sun_path_rotation;
    settings.ambient_occlusion_level = directives.pack.ambient_occlusion_level;
    settings.wetness_half_life = directives.pack.wetness_half_life;
    settings.dryness_half_life = directives.pack.dryness_half_life;
    settings.eye_brightness_half_life = directives.pack.eye_brightness_half_life;
    settings.center_depth_half_life = directives.pack.center_depth_half_life;

    // Item 5: refine storage-image entries from reflection.
    let reflections: Vec<&sb_compile::Reflection> =
        specs.keys().filter_map(|k| compiled.get(k).and_then(|r| r.as_ref().ok())).flat_map(|c| c.reflections.iter()).collect();
    bindings::refine_storage_images(&mut binding_table, &reflections);

    let distant_horizons = DhPipeline {
        strategy,
        unified_projection: strategy == DhStrategy::Synthesized,
        shadow_enabled: geometry.contains_key(&GeometryProgram::DhShadow),
    };
    if strategy == DhStrategy::Synthesized && !geometry.contains_key(&GeometryProgram::DhTerrain) {
        diags.push(Diagnostic::warning("pipeline.dh-synthesis", "no DH terrain program could be synthesized (gbuffers_terrain chain missing or failed); LODs render unshaded"));
    }

    let pipeline = DimensionPipeline {
        folder: info.folder.clone(),
        dimension_ids: info.dimension_ids.clone(),
        targets,
        settings,
        uniforms: layout.clone(),
        custom_uniforms: props.custom_uniforms.clone(),
        bindings: binding_table.clone(),
        programs,
        geometry,
        passes: schedule.passes.clone(),
        gbuffer_attachments: gbuffer_attachments.clone(),
        shadow_attachments: shadow_attachments.clone(),
        end_of_frame_copies: schedule.end_of_frame_copies.clone(),
        distant_horizons,
    };
    let state = FolderState {
        info: info.clone(),
        plan,
        analysis,
        directives,
        layout,
        members,
        bindings: binding_table,
        contexts,
        image_formats,
        dh_constants,
        gbuffer_attachments,
        shadow_attachments,
        schedule,
    };
    FolderResult { pipeline, diagnostics: diags, stats, state }
}

/// What [`SpecInputs::spec`] needs to describe a variant.
pub struct SpecInputs<'a> {
    pub folder: &'a str,
    pub plan: &'a FolderPlan,
    pub directives: &'a FolderDirectives,
    pub gbuffer_attachments: &'a [u32],
    pub shadow_attachments: &'a [u32],
    pub dh_constants: &'a IndexMap<String, i64>,
    pub props: &'a sb_pack::ShadersProperties,
}

impl SpecInputs<'_> {
    /// The model identity, draw buffers, output slots, alpha test and profile constants of
    /// a variant. `synth` names the DH program a geometry unit is synthesized as.
    pub fn spec(&self, key: &VariantKey, synth: Option<GeometryProgram>, order: (u8, u32, u32)) -> VariantSpec {
        let unit = &self.plan.units[key.unit];
        let (name, kind, synthesized_from, props_name, identity) = match synth {
            Some(dh) => (
                program_path(self.folder, dh.file_name()),
                ProgramKind::Geometry { program: dh },
                Some(unit.path.clone()),
                dh.file_name().to_string(),
                Some(dh),
            ),
            None => (
                unit.path.clone(),
                unit.kind.clone(),
                unit.is_fallback().then(|| crate::resolve::FALLBACK_SOURCE.to_string()),
                unit.props_name.clone(),
                unit.geometry(),
            ),
        };
        let draw_buffers = self.directives.draw_buffers[key.unit].clone();
        let shared: &[u32] = match key.class {
            ProgramClass::Gbuffers | ProgramClass::Dh if !key.shadow_pass => self.gbuffer_attachments,
            ProgramClass::Shadow | ProgramClass::Dh => self.shadow_attachments,
            _ => &[],
        };
        let output_slots: Vec<u32> = if shared.is_empty() || unit.is_compute() {
            (0..draw_buffers.len() as u32).collect()
        } else {
            draw_buffers.iter().map(|b| shared.iter().position(|x| x == b).unwrap_or(0) as u32).collect()
        };
        let alpha_test = match identity {
            Some(g) if !unit.is_compute() => self
                .props
                .alpha_test_of(&props_name)
                .or_else(|| g.default_alpha_test().map(|(func, reference)| AlphaTest { func, reference })),
            _ => None,
        };
        let profile_constants = if key.class == ProgramClass::Dh { self.dh_constants.clone() } else { IndexMap::new() };
        VariantSpec {
            key: key.clone(),
            name,
            kind,
            synthesized_from,
            props_name,
            identity,
            draw_profile: (!unit.is_compute()).then(|| key.profile.clone()),
            draw_buffers: if unit.is_compute() { Vec::new() } else { draw_buffers },
            output_slots,
            alpha_test,
            profile_constants,
            order,
        }
    }
}

/// Schedule input from a plan; `index` maps a unit to the id used in passes (`None` =
/// absent).
fn schedule_input(
    plan: &FolderPlan,
    directives: &FolderDirectives,
    props: &sb_pack::ShadersProperties,
    index: &dyn Fn(usize) -> Option<u32>,
) -> ScheduleInput {
    let mut si = ScheduleInput::default();
    for (group, slots) in &plan.groups {
        let mut passes = Vec::new();
        for s in slots {
            let program = s.program.and_then(index);
            let computes: Vec<u32> = s.computes.iter().filter_map(|&u| index(u)).collect();
            let (draw_buffers, explicit_flips) = match s.program {
                Some(u) if program.is_some() => {
                    (directives.draw_buffers[u].clone(), props.flips_of(&plan.units[u].props_name).cloned().unwrap_or_default())
                }
                _ => (Vec::new(), IndexMap::new()),
            };
            passes.push(PassInput { index: s.index, program, computes, draw_buffers, explicit_flips });
        }
        si.passes.insert(*group, passes);
    }
    si.geometry_computes.insert(PassGroup::Shadow, plan.shadow_computes.iter().filter_map(|&u| index(u)).collect());
    for g in [PassGroup::Begin, PassGroup::Prepare, PassGroup::Deferred, PassGroup::Composite] {
        if let Some(name) = g.pre_flip_name()
            && let Some(f) = props.flips_of(name)
        {
            si.pre_flips.insert(g, f.clone());
        }
    }
    si
}

/// colortex indices the folder uses: written by non-shadow graphics programs, read through
/// bindings, plus colortex0 and `fallbackTex`.
fn colortex_used(programs: &[Program], table: &BindingTable, props: &sb_pack::ShadersProperties) -> BTreeSet<u32> {
    let mut used: BTreeSet<u32> = [0, props.fallback_tex.unwrap_or(0)].into();
    for p in programs {
        let writes_color = match &p.kind {
            ProgramKind::Geometry { program } => !is_shadow_group(*program),
            ProgramKind::Composite { group, .. } => !matches!(group, PassGroup::ShadowComp | PassGroup::Final),
            _ => false,
        };
        if writes_color {
            used.extend(p.draw_buffers.iter().copied());
        }
        for b in &p.bindings_used {
            if let Some(e) = table.get(&b.name)
                && let ResourceRef::ColorTex(i) | ResourceRef::ColorImage(i) = e.resource
            {
                used.insert(i);
            }
        }
    }
    used.retain(|i| *i < sb_uniforms::MAX_COLOR_TEX);
    used
}

#[allow(clippy::too_many_arguments)]
fn render_targets(
    programs: &[Program],
    table: &BindingTable,
    directives: &FolderDirectives,
    props: &sb_pack::ShadersProperties,
    pack: &ShaderPack,
    colortex_used: &BTreeSet<u32>,
    shadow: ShadowSettings,
    diags: &mut Diagnostics,
    load: &PackLoad,
) -> RenderTargets {
    let mipmap_programs = |i: u32| -> Vec<u32> {
        programs.iter().enumerate().filter(|(_, p)| p.mipmap_targets.contains(&i)).map(|(k, _)| k as u32).collect()
    };
    let colortex: Vec<ColorTarget> = colortex_used
        .iter()
        .map(|&i| {
            let d = directives.pack.colortex.get(&i).cloned().unwrap_or_default();
            ColorTarget {
                index: i,
                format: d.format.unwrap_or(TextureFormat::RGBA),
                clear: d.clear.unwrap_or(true),
                clear_color: d.clear_color,
                mipmap_programs: mipmap_programs(i),
                size: props.buffer_size_of(i).and_then(|s| s.to_target_size()).unwrap_or_default(),
                used: true,
            }
        })
        .collect();
    let mut sc_used: BTreeSet<u32> = BTreeSet::new();
    let mut uses_depthtex = [false; 3];
    for p in programs {
        let writes_shadow = match &p.kind {
            ProgramKind::Geometry { program } => is_shadow_group(*program),
            ProgramKind::Composite { group, .. } => *group == PassGroup::ShadowComp,
            _ => false,
        };
        if writes_shadow {
            sc_used.extend(p.draw_buffers.iter().copied());
        }
        for b in &p.bindings_used {
            match table.get(&b.name).map(|e| &e.resource) {
                Some(ResourceRef::ShadowColor(i) | ResourceRef::ShadowColorImage(i)) => {
                    sc_used.insert(*i);
                }
                Some(ResourceRef::DepthTex(i)) if (*i as usize) < 3 => uses_depthtex[*i as usize] = true,
                _ => {}
            }
        }
    }
    sc_used.retain(|i| *i < sb_uniforms::MAX_SHADOW_COLOR);
    if sc_used.iter().any(|i| *i >= 2) && !load.feature_active("HIGHER_SHADOWCOLOR") {
        diags.push(Diagnostic::warning(
            "pipeline.shadowcolor-flag",
            "shadowcolor2+ is used without the HIGHER_SHADOWCOLOR feature flag (Iris only provides shadowcolor0-1 then); provided anyway",
        ));
    }
    let shadowcolor: Vec<ColorTarget> = sc_used
        .iter()
        .map(|&i| {
            let d = directives.pack.shadowcolor.get(&i).cloned().unwrap_or_default();
            ColorTarget {
                index: i,
                format: d.format.unwrap_or(TextureFormat::RGBA),
                clear: d.clear.unwrap_or(true),
                clear_color: d.clear_color,
                mipmap_programs: Vec::new(),
                size: TargetSize::Absolute { width: shadow.resolution, height: shadow.resolution },
                used: true,
            }
        })
        .collect();
    let (custom_textures, d) = props.resolve_custom_textures(pack);
    diags.extend(d);
    let noise_texture = props.noise_texture.as_deref().and_then(|p| match sb_pack::shaders_properties::parse_texture_source(p) {
        Ok((src, _)) => Some(src),
        Err(e) => {
            diags.push(Diagnostic::warning("props.bad-value", format!("texture.noise `{p}`: {e}")));
            None
        }
    });
    RenderTargets {
        colortex,
        shadowcolor,
        shadow,
        uses_depthtex1: uses_depthtex[1],
        uses_depthtex2: uses_depthtex[2],
        noise_texture_resolution: directives.pack.noise_texture_resolution,
        noise_texture,
        custom_textures,
        images: props.images.clone(),
        buffers: props.storage_buffers(),
    }
}

/// Build the model program of a compiled variant (spec step 12).
#[allow(clippy::too_many_arguments)]
pub(crate) fn model_program(
    spec: &VariantSpec,
    c: &CompiledVariant,
    unit: &Unit,
    directives: &FolderDirectives,
    props: &sb_pack::ShadersProperties,
    table: &BindingTable,
    settings: &PackSettings,
    env: &sb_core::model::CompileEnvironment,
    diags: &mut Diagnostics,
    blobs: &mut BlobStore,
) -> Program {
    let u = spec.key.unit;
    let stages: Vec<StageModule> = c
        .stages
        .iter()
        .map(|s| StageModule {
            stage: s.stage,
            entry_point: "main".into(),
            spirv: s.spirv.as_ref().map(|w| blobs.spirv(w)),
            glsl_vulkan: s.glsl_vulkan.as_ref().map(|g| blobs.glsl(g)),
            glsl_renderpearl: s.glsl_renderpearl.as_ref().map(|g| blobs.glsl(g)),
            source_file: s.source_file.clone(),
        })
        .collect();
    let output_types: Vec<String> = spec
        .output_slots
        .iter()
        .map(|slot| c.fragment_outputs.iter().find(|(l, _)| l == slot).map(|(_, t)| t.clone()).unwrap_or_else(|| "float".into()))
        .collect();
    let blend = match (&spec.kind, spec.identity) {
        (ProgramKind::Compute { .. } | ProgramKind::GeometryCompute { .. }, _) => None,
        (_, Some(g)) => props.blend_of(&spec.props_name).unwrap_or_else(|| g.default_blend()),
        _ => props.blend_of(&spec.props_name).flatten(),
    };
    let bindings_used: Vec<BindingUse> = c
        .resources_used
        .iter()
        .filter_map(|(name, stages)| {
            let e = table.get(name)?;
            Some(BindingUse { name: e.name.clone(), set: e.set, binding: e.binding, use_alt: false, stages: stages.clone() })
        })
        .collect();
    let compute = unit.is_compute().then(|| ComputeInfo {
        local_size: c.local_size.unwrap_or([1, 1, 1]),
        work_groups: directives.work_groups[u].unwrap_or(WorkGroups::Relative { x: 1.0, y: 1.0 }),
        indirect: props.indirect_of(&spec.props_name),
    });
    let mut requires_raw_vulkan = c.requires_raw_vulkan;
    // Cross-crate item 4: host descriptor limit.
    if let (Some(limit), Some(count)) = (env.device.max_descriptors_per_program, c.descriptor_count)
        && count > limit
    {
        requires_raw_vulkan = true;
        diags.push(
            Diagnostic::error(
                "xf.too-many-descriptors",
                format!("the program uses {count} descriptors but the host allows {limit} per program; it needs the raw Vulkan path"),
            )
            .in_program(spec.name.clone()),
        );
    }
    let mipmap_targets = if matches!(spec.kind, ProgramKind::Composite { .. }) { directives.mipmaps[u].clone() } else { Vec::new() };
    Program {
        name: spec.name.clone(),
        kind: spec.kind.clone(),
        draw_profile: spec.draw_profile.clone(),
        requires_raw_vulkan,
        stages,
        draw_buffers: spec.draw_buffers.clone(),
        output_slots: if unit.is_compute() { Vec::new() } else { spec.output_slots.clone() },
        output_types: if unit.is_compute() { Vec::new() } else { output_types },
        blend,
        blend_per_buffer: if unit.is_compute() { IndexMap::new() } else { props.buffer_blend_of(&spec.props_name).cloned().unwrap_or_default() },
        alpha_test: spec.alpha_test,
        viewport: props.scale_of(&spec.props_name).unwrap_or_default(),
        mipmap_targets,
        bindings_used,
        vertex_inputs: c.vertex_inputs.clone(),
        push_constant_size: c.push_constant_size,
        compute,
        cull: cull_of(spec.identity, settings),
        synthesized_from: spec.synthesized_from.clone(),
    }
}

/// Set `use_alt` of colortex/colorimg bindings from the flip state of the pass each
/// program runs in. A program shared by geometry slots of passes with different states
/// (e.g. opaque and translucent around a flipping `deferred`) is duplicated.
fn assign_use_alt(
    programs: &mut Vec<Program>,
    geometry: &mut IndexMap<GeometryProgram, GeometrySlot>,
    slot_phase: &[(GeometryProgram, u32)],
    schedule: &Schedule,
    table: &BindingTable,
) {
    let state_vec = |p: &sb_core::model::Pass| -> BTreeSet<u32> {
        p.flip_state.iter().enumerate().filter(|(_, f)| **f).map(|(i, _)| i as u32).collect()
    };
    let apply = |prog: &mut Program, state: &BTreeSet<u32>| {
        for b in &mut prog.bindings_used {
            b.use_alt = match table.get(&b.name).map(|e| &e.resource) {
                Some(ResourceRef::ColorTex(i) | ResourceRef::ColorImage(i)) => state.contains(i),
                _ => false,
            };
        }
    };
    // Composite-style programs and computes: their own pass.
    for pass in &schedule.passes {
        let st = state_vec(pass);
        for idx in pass.program.iter().chain(pass.computes.iter()) {
            if let Some(p) = programs.get_mut(*idx as usize) {
                apply(p, &st);
            }
        }
    }
    // Geometry programs: the pass of the slot's phase.
    let mut assigned: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    let mut clones: HashMap<(u32, Vec<u32>), u32> = HashMap::new();
    for (g, idx) in slot_phase {
        let phase = phase_of_slot(*g);
        let st = schedule.group_state.get(&phase).cloned().unwrap_or_default();
        match assigned.get(idx) {
            None => {
                if let Some(p) = programs.get_mut(*idx as usize) {
                    apply(p, &st);
                }
                assigned.insert(*idx, st);
            }
            Some(prev) if *prev == st => {}
            Some(_) => {
                let sig: Vec<u32> = st.iter().copied().collect();
                let new_idx = match clones.get(&(*idx, sig.clone())) {
                    Some(n) => *n,
                    None => {
                        let mut p = programs[*idx as usize].clone();
                        apply(&mut p, &st);
                        programs.push(p);
                        let n = (programs.len() - 1) as u32;
                        clones.insert((*idx, sig), n);
                        n
                    }
                };
                if let Some(slot) = geometry.get_mut(g) {
                    slot.program = new_idx;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_and_cull() {
        assert_eq!(phase_of_slot(GeometryProgram::ShadowCutout), PassGroup::Shadow);
        assert_eq!(phase_of_slot(GeometryProgram::DhShadow), PassGroup::Shadow);
        assert_eq!(phase_of_slot(GeometryProgram::Water), PassGroup::GbuffersTranslucent);
        assert_eq!(phase_of_slot(GeometryProgram::DhWater), PassGroup::GbuffersTranslucent);
        assert_eq!(phase_of_slot(GeometryProgram::Terrain), PassGroup::GbuffersOpaque);
        let mut s = PackSettings::default();
        s.back_face.insert("solid".into(), true);
        s.back_face.insert("translucent".into(), false);
        assert_eq!(cull_of(Some(GeometryProgram::TerrainSolid), &s), Some(false));
        assert_eq!(cull_of(Some(GeometryProgram::Water), &s), Some(true));
        assert_eq!(cull_of(Some(GeometryProgram::TerrainCutout), &s), None);
        assert_eq!(cull_of(Some(GeometryProgram::Terrain), &s), None);
    }
}
