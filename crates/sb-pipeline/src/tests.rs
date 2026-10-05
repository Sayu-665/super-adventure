//! End-to-end unit tests on small in-memory packs.

use super::*;
use pretty_assertions::assert_eq;
use sb_core::PassGroup;
use sb_core::model::{DhStrategy, OutputTarget, ResourceKind, ResourceRef};

const TERRAIN_VSH: &str = "#version 120\nvarying vec2 uv;\nvarying vec4 color;\nvoid main() {\n    gl_Position = ftransform();\n    uv = (gl_TextureMatrix[0] * gl_MultiTexCoord0).xy;\n    color = gl_Color;\n}\n";
const TERRAIN_FSH: &str = "#version 120\nuniform sampler2D texture;\nuniform sampler2D gaux1;\nvarying vec2 uv;\nvarying vec4 color;\n/* RENDERTARGETS: 0,2 */\nvoid main() {\n    gl_FragData[0] = texture2D(texture, uv) * color;\n    gl_FragData[1] = texture2D(gaux1, uv);\n}\n";
const QUAD_VSH: &str = "#version 120\nvarying vec2 texcoord;\nvoid main() {\n    gl_Position = ftransform();\n    texcoord = gl_MultiTexCoord0.xy;\n}\n";

fn composite_fsh(read: &str, targets: &str) -> String {
    format!(
        "#version 120\nuniform sampler2D {read};\nuniform float frameTimeCounter;\nvarying vec2 texcoord;\n/* RENDERTARGETS: {targets} */\nvoid main() {{\n    gl_FragData[0] = texture2D({read}, texcoord) + vec4(frameTimeCounter * 0.0);\n}}\n"
    )
}

fn base_files() -> Vec<(String, String)> {
    vec![
        ("gbuffers_terrain.vsh".into(), TERRAIN_VSH.into()),
        ("gbuffers_terrain.fsh".into(), TERRAIN_FSH.into()),
        ("composite.vsh".into(), QUAD_VSH.into()),
        ("composite.fsh".into(), composite_fsh("colortex0", "0")),
        ("composite1.vsh".into(), QUAD_VSH.into()),
        ("composite1.fsh".into(), composite_fsh("colortex0", "0")),
        ("final.vsh".into(), QUAD_VSH.into()),
        ("final.fsh".into(), "#version 120\nuniform sampler2D colortex0;\nvarying vec2 texcoord;\nvoid main() { gl_FragColor = texture2D(colortex0, texcoord); }\n".into()),
    ]
}

fn pack_of(files: &[(String, String)]) -> ShaderPack {
    ShaderPack::from_files("test", files.iter().map(|(a, b)| (a.as_str(), b.as_str())))
}

fn vulkan_only() -> CompileSettings {
    let mut s = CompileSettings::default();
    s.env.targets = vec![OutputTarget::Vulkan];
    s
}

fn errors(out: &CompileOutput) -> Vec<String> {
    out.pack.diagnostics.errors().map(|d| d.to_string()).collect()
}

fn program<'a>(dim: &'a sb_core::model::DimensionPipeline, name: &str) -> &'a Program {
    dim.programs.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no program {name}"))
}

#[test]
fn minimal_pack_end_to_end() {
    let pack = pack_of(&base_files());
    let settings = CompileSettings { validate_spirv: true, ..Default::default() };
    let out = compile_pack(&pack, &settings);
    assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
    assert!(out.stats.programs_failed.is_empty(), "{:?}", out.stats.programs_failed);
    assert_eq!(out.pack.dimensions.len(), 1);
    let dim = &out.pack.dimensions[0];
    assert_eq!(dim.folder, "");
    assert_eq!(dim.dimension_ids, vec!["*".to_string()]);

    // Geometry: terrain serves its own slot and its fallbacks; nothing else exists.
    let terrain = dim.geometry[&GeometryProgram::Terrain].program;
    for g in [GeometryProgram::TerrainCutout, GeometryProgram::TerrainSolid, GeometryProgram::Water, GeometryProgram::Block] {
        let slot = &dim.geometry[&g];
        assert_eq!(slot.resolved_from, GeometryProgram::Terrain, "{g:?}");
    }
    assert_eq!(dim.geometry[&GeometryProgram::Water].program, terrain);
    // gbuffers_block wants the entity profile: a second variant of gbuffers_terrain.
    let block = &dim.programs[dim.geometry[&GeometryProgram::Block].program as usize];
    assert_eq!(block.name, "gbuffers_terrain");
    assert_eq!(block.draw_profile.as_deref(), Some("vanilla_entity"));
    // gbuffers_basic's chain is absent: Iris's fallback program draws that geometry.
    let basic = &dim.programs[dim.geometry[&GeometryProgram::Basic].program as usize];
    assert_eq!(basic.synthesized_from.as_deref(), Some(FALLBACK_SOURCE));

    // Shared gbuffer attachments and output slots.
    assert_eq!(dim.gbuffer_attachments, vec![0, 2]);
    let t = &dim.programs[terrain as usize];
    assert_eq!(t.draw_buffers, vec![0, 2]);
    assert_eq!(t.output_slots, vec![0, 1]);
    assert_eq!(t.alpha_test.map(|a| a.reference), Some(0.1));
    assert!(t.stages.iter().all(|s| s.spirv.is_some() && s.glsl_vulkan.is_some() && s.glsl_renderpearl.is_some()));

    // Passes and flips: composite and composite1 ping-pong colortex0.
    let groups: Vec<(PassGroup, u8)> = dim.passes.iter().map(|p| (p.group, p.index)).collect();
    assert_eq!(
        groups,
        vec![
            (PassGroup::Shadow, 0),
            (PassGroup::GbuffersOpaque, 0),
            (PassGroup::GbuffersTranslucent, 0),
            (PassGroup::Composite, 0),
            (PassGroup::Composite, 1),
            (PassGroup::Final, 0)
        ]
    );
    let c0 = program(dim, "composite");
    let c1 = program(dim, "composite1");
    let fin = program(dim, "final");
    assert_eq!(c0.bindings_used.iter().find(|b| b.name == "colortex0").map(|b| b.use_alt), Some(false));
    assert_eq!(c1.bindings_used.iter().find(|b| b.name == "colortex0").map(|b| b.use_alt), Some(false).map(|_| true));
    assert_eq!(fin.bindings_used.iter().find(|b| b.name == "colortex0").map(|b| b.use_alt), Some(false));
    assert_eq!(dim.passes[3].flips_after, vec![0]);
    assert_eq!(dim.passes[4].flips_after, vec![0]);
    // Flipped twice: no end-of-frame copy.
    assert!(dim.end_of_frame_copies.is_empty());
    // The gbuffers program reads gaux1 (colortex4) and the composites read colortex0.
    let used: Vec<u32> = dim.targets.colortex.iter().map(|c| c.index).collect();
    assert_eq!(used, vec![0, 2, 4]);
    assert!(dim.uniforms.frame.member("frameTimeCounter").is_some());
    assert!(!dim.targets.shadow.enabled);
    assert_eq!(dim.distant_horizons.strategy, DhStrategy::Synthesized);
    assert!(dim.distant_horizons.unified_projection);
    let dh = &dim.programs[dim.geometry[&GeometryProgram::DhTerrain].program as usize];
    assert_eq!(dh.name, "dh_terrain");
    assert_eq!(dh.synthesized_from.as_deref(), Some("gbuffers_terrain"));
    // Synthesized from gbuffers_terrain: the vanilla-lightmap variant of dh_terrain.
    assert_eq!(dh.draw_profile.as_deref(), Some(sb_transform::DH_SYNTH_PROFILE));
    // dh_water falls back to the same synthesized program (gbuffers_water → gbuffers_terrain).
    assert_eq!(dim.geometry[&GeometryProgram::DhWater].program, dim.geometry[&GeometryProgram::DhTerrain].program);
    assert!(out.stats.modules_validated > 0 || sb_compile::find_tool("spirv-val").is_none());

    // Blobs are valid SPIR-V and the JSON round-trips.
    for p in &dim.programs {
        for s in &p.stages {
            let words = out.blobs.get_spirv(s.spirv.unwrap()).unwrap();
            assert_eq!(words[0], 0x0723_0203);
        }
    }
    let json = out.pack.to_json();
    let back = CompiledPack::from_json(&json).unwrap();
    assert_eq!(back, out.pack);
    let (infos, buf) = out.blobs.concat();
    assert_eq!(infos, out.pack.blobs);
    assert!(BlobTable::from_concat(&out.pack.blobs, &buf).is_some());
}

#[test]
fn failing_program_falls_back() {
    let mut files = base_files();
    files.push(("gbuffers_water.vsh".into(), TERRAIN_VSH.into()));
    files.push(("gbuffers_water.fsh".into(), "#version 120\nvoid main() { gl_FragData[0] = undefined_function(1.0); }\n".into()));
    let pack = pack_of(&files);
    let out = compile_pack(&pack, &vulkan_only());
    let dim = &out.pack.dimensions[0];
    assert_eq!(dim.geometry[&GeometryProgram::Water].resolved_from, GeometryProgram::Terrain);
    assert!(out.stats.programs_failed.iter().any(|f| f.starts_with("gbuffers_water")), "{:?}", out.stats.programs_failed);
    let errs: Vec<_> = out.pack.diagnostics.errors().collect();
    assert!(errs.iter().any(|d| d.program.as_deref() == Some("gbuffers_water") && d.location.as_ref().is_some_and(|l| l.file == "gbuffers_water.fsh")), "{errs:#?}");
    assert!(out.pack.diagnostics.iter().any(|d| d.code == "pipeline.fallback"));
    // Synthesized dh_water also falls back past the broken gbuffers_water.
    let dh_water = &dim.programs[dim.geometry[&GeometryProgram::DhWater].program as usize];
    assert_eq!(dh_water.synthesized_from.as_deref(), Some("gbuffers_terrain"));
}

#[test]
fn failing_composite_is_absent_from_schedule() {
    let mut files = base_files();
    files.retain(|(n, _)| n != "composite1.fsh");
    files.push(("composite1.fsh".into(), "#version 120\nvoid main() { syntax error }\n".into()));
    let out = compile_pack(&pack_of(&files), &vulkan_only());
    let dim = &out.pack.dimensions[0];
    assert!(dim.programs.iter().all(|p| p.name != "composite1"));
    assert!(!dim.passes.iter().any(|p| p.group == PassGroup::Composite && p.index == 1));
    // composite flipped colortex0 once: final reads alt and the buffer is cleared (no copy).
    assert_eq!(program(dim, "final").bindings_used[0].use_alt, true);
    assert!(out.stats.programs_failed.iter().any(|f| f.contains("composite1")));
}

#[test]
fn native_and_disabled_dh() {
    let mut files = base_files();
    files.push(("dh_terrain.vsh".into(), "#version 120\nvarying vec4 c;\nvoid main() { gl_Position = ftransform(); c = gl_Color; }\n".into()));
    files.push(("dh_terrain.fsh".into(), "#version 120\nvarying vec4 c;\nuniform float dhFarPlane;\n/* RENDERTARGETS: 0 */\nvoid main() { gl_FragData[0] = c * dhFarPlane; }\n".into()));
    let pack = pack_of(&files);
    let out = compile_pack(&pack, &vulkan_only());
    assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
    let dim = &out.pack.dimensions[0];
    assert_eq!(dim.distant_horizons.strategy, DhStrategy::Native);
    assert!(!dim.distant_horizons.unified_projection);
    let dh = &dim.programs[dim.geometry[&GeometryProgram::DhTerrain].program as usize];
    assert_eq!(dh.synthesized_from, None);
    // Native DH programs keep the plain DH profile.
    assert_eq!(dh.draw_profile.as_deref(), Some("dh_terrain"));
    assert_eq!(dim.geometry[&GeometryProgram::DhWater].resolved_from, GeometryProgram::DhTerrain);

    let mut s = vulkan_only();
    s.env.distant_horizons = false;
    let out = compile_pack(&pack, &s);
    let dim = &out.pack.dimensions[0];
    assert_eq!(dim.distant_horizons.strategy, DhStrategy::Disabled);
    assert!(dim.programs.iter().all(|p| !p.name.starts_with("dh_")));
    assert!(!dim.geometry.keys().any(|g| g.group() == sb_core::program::GeometryGroup::DistantHorizons));
}

/// Iris draws a pack's `dh_shadow` into shadowcolor0 and shadowcolor1 whatever its
/// `RENDERTARGETS` says (`createDHFramebufferShadow` uses draw buffers `{0, 1}`), so output
/// 0 of a `RENDERTARGETS: 1` dh_shadow lands in shadowcolor0, not shadowcolor1.
#[test]
fn native_dh_shadow_ignores_its_draw_buffer_directive() {
    let mut files = base_files();
    files.push(("shadow.vsh".into(), TERRAIN_VSH.into()));
    files.push(("shadow.fsh".into(), "#version 120\nvarying vec2 uv;\nvarying vec4 color;\n/* RENDERTARGETS: 0,2 */\nvoid main() { gl_FragData[0] = color; gl_FragData[1] = color; }\n".into()));
    files.push(("dh_terrain.vsh".into(), "#version 120\nvarying vec4 c;\nvoid main() { gl_Position = ftransform(); c = gl_Color; }\n".into()));
    files.push(("dh_terrain.fsh".into(), "#version 120\nvarying vec4 c;\n/* RENDERTARGETS: 0 */\nvoid main() { gl_FragData[0] = c; }\n".into()));
    files.push(("dh_shadow.vsh".into(), "#version 120\nvarying vec4 c;\nvoid main() { gl_Position = ftransform(); c = gl_Color; }\n".into()));
    let dh_shadow_fsh = |targets: &str| format!("#version 120\nvarying vec4 c;\n/* RENDERTARGETS: {targets} */\nvoid main() {{ gl_FragData[0] = c; }}\n");
    files.push(("dh_shadow.fsh".into(), dh_shadow_fsh("1")));
    let out = compile_pack(&pack_of(&files), &vulkan_only());
    assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
    let dim = &out.pack.dimensions[0];
    assert_eq!(dim.distant_horizons.strategy, DhStrategy::Native);
    let dh = &dim.programs[dim.geometry[&GeometryProgram::DhShadow].program as usize];
    assert_eq!(dh.name, "dh_shadow");
    assert_eq!(dh.draw_buffers, vec![0, 1]);
    // The shared shadow pass binds the union of the shadow programs' buffers; output i of
    // dh_shadow goes to the slot holding shadowcolor i.
    assert_eq!(dim.shadow_attachments, vec![0, 1, 2]);
    assert_eq!(dh.output_slots, vec![0, 1]);
    let fs = dh.stages.iter().find(|s| s.stage == sb_core::ShaderStage::Fragment).unwrap();
    let glsl = out.blobs.get_str(fs.glsl_vulkan.unwrap()).unwrap();
    assert!(glsl.contains("location = 0"), "{glsl}");
    let ignored = |out: &CompileOutput| out.pack.diagnostics.iter().filter(|d| d.code == "dir.dh-shadow-draw-buffers").count();
    assert_eq!(ignored(&out), 1);
    // The regular shadow program keeps its directive.
    assert_eq!(program(dim, "shadow").draw_buffers, vec![0, 2]);

    // `0` and `0,1` route like Iris already: no diagnostic.
    for targets in ["0", "0,1"] {
        files.retain(|(n, _)| n != "dh_shadow.fsh");
        files.push(("dh_shadow.fsh".into(), dh_shadow_fsh(targets)));
        let out = compile_pack(&pack_of(&files), &vulkan_only());
        let dim = &out.pack.dimensions[0];
        assert_eq!(program(dim, "dh_shadow").draw_buffers, vec![0, 1], "{targets}");
        assert_eq!(ignored(&out), 0, "{targets}");
    }
}

#[test]
fn synthesized_dh_uses_block_ids() {
    let mut files = base_files();
    files.retain(|(n, _)| n != "gbuffers_terrain.vsh");
    files.push((
        "gbuffers_terrain.vsh".into(),
        "#version 120\nattribute vec4 mc_Entity;\nvarying vec2 uv;\nvarying vec4 color;\nvoid main() {\n    gl_Position = ftransform();\n    uv = gl_MultiTexCoord0.xy;\n    color = gl_Color * (mc_Entity.x == 10001.0 ? 0.5 : 1.0);\n}\n".into(),
    ));
    files.push(("block.properties".into(), "block.10001=minecraft:stone granite\nblock.10002=oak_leaves\n".into()));
    let out = compile_pack(&pack_of(&files), &vulkan_only());
    let dim = &out.pack.dimensions[0];
    let dh = &dim.programs[dim.geometry[&GeometryProgram::DhTerrain].program as usize];
    let vs = dh.stages.iter().find(|s| s.stage == sb_core::ShaderStage::Vertex).unwrap();
    let glsl = out.blobs.get_str(vs.glsl_vulkan.unwrap()).unwrap();
    assert!(glsl.contains("SB_DH_BLOCK_ID_2 = 10001"), "{glsl}");
    assert!(glsl.contains("SB_DH_BLOCK_ID_1 = 10002"), "{glsl}");
    assert!(glsl.contains("SB_DH_BLOCK_ID_3 = -1"), "{glsl}");
    assert_eq!(out.pack.id_maps.blocks.get(&10001).map(Vec::len), Some(2));
}

#[test]
fn shadows_and_flip_overrides() {
    let mut files = base_files();
    files.push(("shadow.vsh".into(), TERRAIN_VSH.into()));
    files.push(("shadow.fsh".into(), "#version 120\nvarying vec2 uv;\nvarying vec4 color;\nvoid main() { gl_FragData[0] = color; }\n".into()));
    files.push(("deferred.vsh".into(), QUAD_VSH.into()));
    files.push(("deferred.fsh".into(), composite_fsh("shadowtex0", "1")));
    files.push((
        "shaders.properties".into(),
        "flip.composite.colortex0=false\nflip.composite.colortex3=true\nflip.deferred_pre.colortex5=true\nshadow.enabled=true\n".into(),
    ));
    files.push(("composite2.vsh".into(), QUAD_VSH.into()));
    files.push(("composite2.fsh".into(), "#version 120\nconst bool colortex3Clear = false;\nuniform sampler2D colortex3;\nvarying vec2 texcoord;\n/* RENDERTARGETS: 3 */\nvoid main() { gl_FragData[0] = texture2D(colortex3, texcoord); }\n".into()));
    let out = compile_pack(&pack_of(&files), &vulkan_only());
    assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
    let dim = &out.pack.dimensions[0];
    assert!(dim.targets.shadow.enabled);
    assert_eq!(dim.shadow_attachments, vec![0, 1]);
    assert_eq!(dim.targets.shadowcolor.iter().map(|c| c.index).collect::<Vec<_>>(), vec![0, 1]);
    let flips: Vec<(PassGroup, u8, Vec<u32>)> =
        dim.passes.iter().filter(|p| p.program.is_some()).map(|p| (p.group, p.index, p.flips_after.clone())).collect();
    assert_eq!(
        flips,
        vec![
            (PassGroup::Deferred, 0, vec![1]),
            (PassGroup::Composite, 0, vec![3]),
            (PassGroup::Composite, 1, vec![0]),
            (PassGroup::Composite, 2, vec![3]),
            (PassGroup::Final, 0, vec![])
        ]
    );
    // deferred_pre flipped colortex5 before deferred.
    let deferred = dim.passes.iter().find(|p| p.group == PassGroup::Deferred).unwrap();
    assert!(deferred.flip_state[5]);
    // colortex3 is flipped by composite (forced) and composite2: even → main. colortex1 and
    // colortex5 end flipped; colortex1 is cleared, colortex5 is not used as a target but
    // is not cleared-configured either (default clear) → no copy.
    let c2 = program(dim, "composite2");
    assert_eq!(c2.bindings_used.iter().find(|b| b.name == "colortex3").map(|b| b.use_alt), Some(true));
    assert!(dim.targets.colortex.iter().any(|c| c.index == 3 && !c.clear));
    let st = |g| dim.passes.iter().find(|p| p.group == g).unwrap().flip_state.clone();
    assert!(st(PassGroup::GbuffersTranslucent)[1]);
    assert!(!st(PassGroup::GbuffersOpaque)[1]);
}

#[test]
fn use_alt_differs_between_opaque_and_translucent() {
    // deferred flips colortex4, which gbuffers_terrain reads: the water slot (translucent,
    // after deferred) needs a copy of the program reading the other image.
    let mut files = base_files();
    files.push(("deferred.vsh".into(), QUAD_VSH.into()));
    files.push(("deferred.fsh".into(), composite_fsh("colortex4", "4")));
    let out = compile_pack(&pack_of(&files), &vulkan_only());
    assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
    let dim = &out.pack.dimensions[0];
    let opaque = &dim.programs[dim.geometry[&GeometryProgram::Terrain].program as usize];
    let water = &dim.programs[dim.geometry[&GeometryProgram::Water].program as usize];
    assert_eq!(opaque.name, water.name);
    let alt = |p: &Program| p.bindings_used.iter().find(|b| dim.bindings.get(&b.name).is_some_and(|e| e.resource == ResourceRef::ColorTex(4))).map(|b| b.use_alt);
    assert_eq!(alt(opaque), Some(false));
    assert_eq!(alt(water), Some(true));
    assert_ne!(dim.geometry[&GeometryProgram::Terrain].program, dim.geometry[&GeometryProgram::Water].program);
}

#[test]
fn program_enabled_and_options() {
    let mut files = base_files();
    files.push(("shaders.properties".into(), "program.composite1.enabled=SECOND_PASS\n".into()));
    files.retain(|(n, _)| n != "composite1.fsh");
    files.push((
        "composite1.fsh".into(),
        "#version 120\n//#define SECOND_PASS\n#ifdef SECOND_PASS\n#endif\nuniform sampler2D colortex0;\nvarying vec2 texcoord;\nvoid main() { gl_FragData[0] = texture2D(colortex0, texcoord); }\n".into(),
    ));
    let pack = pack_of(&files);
    let out = compile_pack(&pack, &vulkan_only());
    let dim = &out.pack.dimensions[0];
    assert!(dim.programs.iter().all(|p| p.name != "composite1"));
    assert_eq!(out.pack.options.options.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), vec!["SECOND_PASS"]);

    let mut session = PackSession::new(&pack, vulkan_only());
    let a = session.compile();
    assert_eq!(a.pack.dimensions[0].programs.len(), dim.programs.len());
    session.set_option_values(OptionValues::from_pairs([("SECOND_PASS", "true")]));
    let b = session.compile();
    assert!(b.pack.dimensions[0].programs.iter().any(|p| p.name == "composite1"));
    assert_eq!(b.pack.options.options[0].value, "true");
}

#[test]
fn descriptor_limit_marks_raw_vulkan() {
    let mut s = vulkan_only();
    s.env.device.max_descriptors_per_program = Some(1);
    let out = compile_pack(&pack_of(&base_files()), &s);
    let dim = &out.pack.dimensions[0];
    let t = &dim.programs[dim.geometry[&GeometryProgram::Terrain].program as usize];
    assert!(t.requires_raw_vulkan);
    assert!(out.pack.diagnostics.iter().any(|d| d.code == "xf.too-many-descriptors" && d.program.as_deref() == Some("gbuffers_terrain")));
    let mut s = vulkan_only();
    s.env.device.max_descriptors_per_program = Some(64);
    let out = compile_pack(&pack_of(&base_files()), &s);
    assert!(!out.pack.diagnostics.iter().any(|d| d.code == "xf.too-many-descriptors"));
}

#[test]
fn targets_depth_modes_and_shadow_emulation() {
    let pack = pack_of(&base_files());
    let mut s = CompileSettings::default();
    s.env.targets = vec![OutputTarget::Renderpearl];
    s.env.depth_mode = sb_core::model::DepthMode::ReversedZeroToOne;
    s.env.device.comparison_samplers = false;
    let out = compile_pack(&pack, &s);
    assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
    let dim = &out.pack.dimensions[0];
    for p in &dim.programs {
        for st in &p.stages {
            assert!(st.spirv.is_none() && st.glsl_vulkan.is_none() && st.glsl_renderpearl.is_some(), "{}", p.name);
        }
    }
    let rp = out.blobs.get_str(dim.programs[0].stages[0].glsl_renderpearl.unwrap()).unwrap();
    assert!(!rp.contains("binding ="), "{rp}");
}

#[test]
fn compile_variant_and_cache() {
    let pack = pack_of(&base_files());
    // A variant with a profile the folder already uses.
    let mut session = PackSession::new(&pack, vulkan_only());
    let CompiledProgramVariant { program: p, blobs, .. } = compile_variant(&mut session, "", GeometryProgram::Block, "vanilla_entity").unwrap();
    assert_eq!(p.name, "gbuffers_terrain");
    assert_eq!(p.draw_profile.as_deref(), Some("vanilla_entity"));
    assert!(blobs.get_spirv(p.stages[0].spirv.unwrap()).is_some());
    assert!(compile_variant(&mut session, "world9", GeometryProgram::Terrain, "vanilla_terrain").is_err());
    assert!(compile_variant(&mut session, "", GeometryProgram::Terrain, "no_such_profile").is_err());
    assert!(compile_variant(&mut session, "", GeometryProgram::Shadow, "vanilla_terrain").is_err());
    // A profile registered up front through profile_overrides: the model lists the extra
    // variant (outside the geometry map) and compile_variant can produce it too.
    let mut s = vulkan_only();
    s.profile_overrides.insert(GeometryProgram::Terrain, vec!["vanilla_particle".into()]);
    let out = compile_pack(&pack, &s);
    let dim = &out.pack.dimensions[0];
    // (Fallback programs for the particle slots use that profile too.)
    let extra: Vec<&Program> =
        dim.programs.iter().filter(|p| p.draw_profile.as_deref() == Some("vanilla_particle") && p.synthesized_from.is_none()).collect();
    assert_eq!(extra.len(), 1);
    let idx = dim.programs.iter().position(|p| std::ptr::eq(p, extra[0])).unwrap() as u32;
    assert!(dim.geometry.values().all(|slot| slot.program != idx));
    // ... and is listed as a variant of the slots gbuffers_terrain draws.
    assert_eq!(dim.geometry[&GeometryProgram::Terrain].variants.get("vanilla_particle"), Some(&idx));
    assert_eq!(dim.geometry[&GeometryProgram::TerrainCutout].variants.get("vanilla_particle"), Some(&idx));
    let mut session = PackSession::new(&pack, s);
    assert!(compile_variant(&mut session, "", GeometryProgram::Terrain, "vanilla_particle").is_ok());

    let dir = tempfile::tempdir().unwrap();
    let mut s = vulkan_only();
    s.cache_dir = Some(dir.path().to_path_buf());
    let a = compile_pack(&pack, &s);
    assert!(!a.timings.cache_hit);
    let b = compile_pack(&pack, &s);
    assert!(b.timings.cache_hit);
    assert_eq!(a.pack, b.pack);
    assert_eq!(a.blobs.blobs, b.blobs.blobs);
    // A different option value is a different key.
    s.option_values.set("X", "1");
    assert_ne!(cache_key(&pack, &s), cache_key(&pack, &vulkan_only()));
}

#[test]
fn custom_uniforms_and_world_folders() {
    let mut files: Vec<(String, String)> = base_files().into_iter().map(|(n, t)| (format!("world0/{n}"), t)).collect();
    files.push(("world-1/final.fsh".into(), "#version 120\nuniform float nightVision;\nvoid main() { gl_FragColor = vec4(nightVision); }\n".into()));
    files.push(("shaders.properties".into(), "uniform.float.myFade = smooth(1, rainStrength, 2, 2)\nvariable.float.half = 0.5\n".into()));
    let pack = pack_of(&files);
    let out = compile_pack(&pack, &vulkan_only());
    assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
    let folders: Vec<&str> = out.pack.dimensions.iter().map(|d| d.folder.as_str()).collect();
    assert_eq!(folders, vec!["world0", "world-1"]);
    let w0 = &out.pack.dimensions[0];
    assert!(w0.programs.iter().all(|p| p.name.starts_with("world0/")));
    let fade = w0.uniforms.frame.member("myFade").unwrap();
    assert_eq!(fade.source, sb_core::model::UniformSource::Custom("myFade".into()));
    assert!(w0.uniforms.frame.member("rainStrength").is_some());
    assert_eq!(w0.custom_uniforms.len(), 2);
    // world-1 has no vertex shader for final: Iris's default one is synthesized.
    let w1 = &out.pack.dimensions[1];
    let fin = program(w1, "world-1/final");
    assert_eq!(fin.stages[0].source_file, "world-1/final.vsh");
    // Dimension filter.
    let mut s = vulkan_only();
    s.dimension_filter = Some(vec!["world-1".into()]);
    let out = compile_pack(&pack, &s);
    assert_eq!(out.pack.dimensions.len(), 1);
}

#[test]
fn garbage_input_does_not_panic() {
    let files = vec![
        ("shaders.properties".to_string(), "program.x.enabled=((\n\\\u{0}\nflip.composite.colortex99=true\nblend.gbuffers_terrain=ONE\n".to_string()),
        ("composite.fsh".to_string(), "\u{0}\u{1}#version 9999\n#include \"missing.glsl\"\nvoid main(".to_string()),
        ("gbuffers_terrain.fsh".to_string(), "/* RENDERTARGETS: 99,-1,x */\nvoid main(){}".to_string()),
        ("block.properties".to_string(), "block.abc=\\\nstone".to_string()),
    ];
    let out = compile_pack(&pack_of(&files), &CompileSettings::default());
    assert!(out.pack.diagnostics.has_errors());
    let empty = compile_pack(&ShaderPack::from_files("empty", Vec::<(&str, &str)>::new()), &CompileSettings::default());
    assert!(empty.pack.dimensions.is_empty());
    assert!(empty.pack.diagnostics.iter().any(|d| d.code == "pack.no-programs"));
}

/// A pack with only `final` (MinecraftShaderProgramming tutorial 1): every gbuffers slot
/// is drawn by Iris's fallback program, which compiles for every draw profile it needs
/// and writes colortex0.
#[test]
fn fallback_programs_compile_for_every_slot() {
    let files = vec![
        ("final.vsh".to_string(), QUAD_VSH.to_string()),
        ("final.fsh".to_string(), "#version 120\nuniform sampler2D colortex0;\nvarying vec2 texcoord;\nvoid main() { gl_FragColor = texture2D(colortex0, texcoord); }\n".to_string()),
    ];
    let settings = CompileSettings { validate_spirv: true, ..vulkan_only() };
    let out = compile_pack(&pack_of(&files), &settings);
    assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
    assert!(out.stats.programs_failed.is_empty(), "{:?}", out.stats.programs_failed);
    let dim = &out.pack.dimensions[0];
    for g in GeometryProgram::ALL.iter().filter(|g| g.group() == sb_core::program::GeometryGroup::Gbuffers) {
        let slot = dim.geometry.get(g).unwrap_or_else(|| panic!("{g:?} has no program"));
        let p = &dim.programs[slot.program as usize];
        assert_eq!(p.synthesized_from.as_deref(), Some(FALLBACK_SOURCE), "{g:?}");
        assert_eq!(p.draw_buffers, vec![0], "{g:?}");
        assert!(p.stages.iter().all(|s| s.spirv.is_some()), "{g:?}");
    }
    // Default alpha tests and blending of the slot apply (cutout terrain tests alpha,
    // water blends).
    let cutout = &dim.programs[dim.geometry[&GeometryProgram::TerrainCutout].program as usize];
    assert!(cutout.alpha_test.is_some());
    let water = &dim.programs[dim.geometry[&GeometryProgram::Water].program as usize];
    assert!(water.blend.is_some());
    // DH LODs are synthesized from the fallback terrain program; no shadow pass.
    assert_eq!(dim.distant_horizons.strategy, DhStrategy::Synthesized);
    assert!(!dim.geometry.contains_key(&GeometryProgram::Shadow));
    assert!(out.pack.diagnostics.iter().any(|d| d.code == "pipeline.fallback-program"));
}

/// A session recompile reuses every variant whose inputs are unchanged (no transform,
/// no glslang) and produces exactly what a fresh compile produces.
#[test]
fn session_reuses_unchanged_variants() {
    let mut files = base_files();
    files.retain(|(n, _)| n != "composite1.fsh");
    files.push((
        "composite1.fsh".into(),
        "#version 120\n#define BRIGHT\nuniform sampler2D colortex0;\nvarying vec2 texcoord;\nvoid main() {\n#ifdef BRIGHT\n    gl_FragData[0] = texture2D(colortex0, texcoord) * 2.0;\n#else\n    gl_FragData[0] = texture2D(colortex0, texcoord);\n#endif\n}\n".into(),
    ));
    let pack = pack_of(&files);
    let mut session = PackSession::new(&pack, vulkan_only());
    let cold = session.compile();
    assert!(errors(&cold).is_empty(), "{:#?}", errors(&cold));
    assert_eq!(cold.stats.variants_cached, 0);
    assert!(cold.pack.options.options.iter().any(|o| o.name == "BRIGHT"));

    // Unchanged settings: everything comes from the cache, the output is identical.
    session.set_settings(vulkan_only());
    let warm = session.compile();
    assert!(warm.stats.variants_cached >= warm.stats.programs_ok && warm.stats.programs_ok > 0, "{:?}", warm.stats);
    assert_eq!(warm.pack, cold.pack);
    assert_eq!(warm.blobs.blobs, cold.blobs.blobs);
    assert_eq!(warm.stats.programs_ok, cold.stats.programs_ok);

    // One option that only composite1 reads: only that program is translated again.
    let mut settings = vulkan_only();
    settings.option_values = OptionValues::from_pairs([("BRIGHT", "false")]);
    session.set_settings(settings.clone());
    let changed = session.compile();
    assert_eq!(changed.stats.variants_cached, warm.stats.variants_cached - 1, "{:?}", changed.stats);
    let fresh = compile_pack(&pack, &settings);
    assert_eq!(changed.pack, fresh.pack);
    assert_eq!(changed.blobs.blobs, fresh.blobs.blobs);
    assert_ne!(changed.blobs.blobs, warm.blobs.blobs);

    // The cache follows compile settings that change translations (depth mode).
    let mut reversed = settings.clone();
    reversed.env.depth_mode = sb_core::model::DepthMode::ReversedZeroToOne;
    session.set_settings(reversed.clone());
    let rev = session.compile();
    assert_eq!(rev.stats.variants_cached, 0, "{:?}", rev.stats);
    let fresh = compile_pack(&pack, &reversed);
    assert_eq!(rev.blobs.blobs, fresh.blobs.blobs);
}

/// A directory pack edited on disk between two compiles of a session: the recompile sees
/// the edit (the session caches are keyed by the pack contents).
#[test]
fn session_sees_pack_edits_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let shaders = dir.path().join("shaders");
    std::fs::create_dir_all(shaders.join("lib")).unwrap();
    for (name, text) in base_files() {
        std::fs::write(shaders.join(name), text).unwrap();
    }
    let final_fsh = "#version 120\n#include \"/lib/tint.glsl\"\nuniform sampler2D colortex0;\nvarying vec2 texcoord;\nvoid main() { gl_FragColor = texture2D(colortex0, texcoord) * TINT; }\n";
    std::fs::write(shaders.join("final.fsh"), final_fsh).unwrap();
    std::fs::write(shaders.join("lib/tint.glsl"), "#define TINT 0.5\n").unwrap();
    let pack = ShaderPack::open(dir.path()).unwrap();
    let mut session = PackSession::new(&pack, vulkan_only());
    let glsl_of_final = |out: &CompileOutput| {
        let fin = program(&out.pack.dimensions[0], "final");
        let fs = fin.stages.iter().find(|s| s.stage == sb_core::ShaderStage::Fragment).unwrap();
        out.blobs.get_str(fs.glsl_vulkan.unwrap()).unwrap().to_string()
    };
    let first = session.compile();
    assert!(glsl_of_final(&first).contains("0.5"), "{}", glsl_of_final(&first));
    // Only an included file changes.
    std::fs::write(shaders.join("lib/tint.glsl"), "#define TINT 0.25\n").unwrap();
    session.set_settings(vulkan_only());
    let second = session.compile();
    let glsl = glsl_of_final(&second);
    assert!(glsl.contains("0.25") && !glsl.contains("0.5"), "{glsl}");
    let fresh = compile_pack(&pack, &vulkan_only());
    assert_eq!(second.pack, fresh.pack);
    assert_eq!(second.blobs.blobs, fresh.blobs.blobs);
    // Every other variant was reused; only final was translated again.
    assert!(second.stats.programs_failed.is_empty(), "{:?}", second.stats);
    assert_eq!(second.stats.variants_cached, second.stats.programs_ok - 1, "{:?}", second.stats);
}

/// `compile_variant` works for every built-in draw profile (their builtins and host
/// resources are always part of the folder layout) and every slot, including DH slots
/// synthesized from gbuffers programs.
#[test]
fn compile_variant_for_every_builtin_profile() {
    let mut files = base_files();
    files.push(("shadow.vsh".into(), TERRAIN_VSH.into()));
    files.push(("shadow.fsh".into(), "#version 120\nvarying vec2 uv;\nvarying vec4 color;\nvoid main() { gl_FragData[0] = color; }\n".into()));
    let pack = pack_of(&files);
    let mut session = PackSession::new(&pack, vulkan_only());
    let out = session.compile();
    assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
    let slots = [GeometryProgram::TerrainSolid, GeometryProgram::Water, GeometryProgram::ShadowSolid, GeometryProgram::Entities, GeometryProgram::DhTerrain];
    for p in sb_transform::builtin_profiles() {
        for g in slots {
            let CompiledProgramVariant { program, blobs, .. } = compile_variant(&mut session, "", g, &p.name)
                .unwrap_or_else(|d| panic!("{} for {g:?}: {:?}", p.name, d.iter().map(|x| x.to_string()).collect::<Vec<_>>()));
            assert_eq!(program.draw_profile.as_deref(), Some(p.name.as_str()));
            assert!(program.stages.iter().all(|s| s.spirv.and_then(|b| blobs.get_spirv(b)).is_some()), "{} {g:?}", p.name);
            if g == GeometryProgram::DhTerrain {
                // Synthesized from gbuffers_terrain, as the pack's own dh_terrain.
                assert_eq!(program.name, "dh_terrain");
                assert_eq!(program.synthesized_from.as_deref(), Some("gbuffers_terrain"));
            }
        }
    }
    // Without Distant Horizons there is no DH program to make a variant of.
    let mut s = vulkan_only();
    s.env.distant_horizons = false;
    let mut session = PackSession::new(&pack, s);
    let e = compile_variant(&mut session, "", GeometryProgram::DhTerrain, "dh_terrain").unwrap_err();
    assert!(e.iter().any(|d| d.code == "pipeline.no-program"), "{e:?}");
}

/// Variants are listed per slot with the `use_alt` of the slot's pass: a program drawn
/// before and after a `deferred` pass that flips a buffer it reads gets one copy per state,
/// for its own profile and for every variant, and `compile_variant` agrees.
#[test]
fn slot_variants_follow_the_flip_state() {
    let mut files = base_files();
    files.push(("deferred.vsh".into(), QUAD_VSH.into()));
    files.push(("deferred.fsh".into(), composite_fsh("colortex4", "4")));
    let mut settings = vulkan_only();
    settings.profile_overrides.insert(GeometryProgram::Terrain, vec!["sodium_terrain".into()]);
    settings.profile_overrides.insert(GeometryProgram::Water, vec!["sodium_terrain".into()]);
    settings.profile_overrides.insert(GeometryProgram::DhTerrain, vec!["dh_terrain".into()]);
    let pack = pack_of(&files);
    let out = compile_pack(&pack, &settings);
    assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
    let dim = &out.pack.dimensions[0];
    let alt = |idx: u32| {
        let p = &dim.programs[idx as usize];
        p.bindings_used.iter().find(|b| dim.bindings.get(&b.name).is_some_and(|e| e.resource == ResourceRef::ColorTex(4))).map(|b| b.use_alt)
    };
    let terrain = &dim.geometry[&GeometryProgram::Terrain];
    let water = &dim.geometry[&GeometryProgram::Water];
    let (t, w) = (terrain.variants["sodium_terrain"], water.variants["sodium_terrain"]);
    assert_ne!(t, w);
    for idx in [t, w] {
        let p = &dim.programs[idx as usize];
        assert_eq!((p.name.as_str(), p.draw_profile.as_deref()), ("gbuffers_terrain", Some("sodium_terrain")));
    }
    assert_eq!((alt(terrain.program), alt(t)), (Some(false), Some(false)));
    assert_eq!((alt(water.program), alt(w)), (Some(true), Some(true)));
    // Variants other slots needed are listed too: gbuffers_block draws gbuffers_terrain
    // with the entity profile.
    let block = dim.geometry[&GeometryProgram::Block].program;
    assert_eq!(dim.programs[block as usize].draw_profile.as_deref(), Some("vanilla_entity"));
    assert_eq!(terrain.variants.get("vanilla_entity"), Some(&block));
    assert_eq!(alt(water.variants["vanilla_entity"]), Some(true));
    assert!(!terrain.variants.contains_key("vanilla_terrain"), "the slot's own profile is not a variant");
    // A DH override follows the synthesis of its slot (here: the native DH vertex format
    // instead of the synthesized programs' vanilla-lightmap variant).
    let dh = &dim.geometry[&GeometryProgram::DhTerrain];
    assert_eq!(dim.programs[dh.program as usize].draw_profile.as_deref(), Some(sb_transform::DH_SYNTH_PROFILE));
    let native = &dim.programs[dh.variants["dh_terrain"] as usize];
    assert_eq!((native.name.as_str(), native.synthesized_from.as_deref()), ("dh_terrain", Some("gbuffers_terrain")));
    // compile_variant produces the same programs.
    let mut session = PackSession::new(&pack, settings);
    for (g, idx) in [(GeometryProgram::Terrain, t), (GeometryProgram::Water, w)] {
        let p = compile_variant(&mut session, "", g, "sodium_terrain").unwrap().program;
        assert_eq!(p.bindings_used, dim.programs[idx as usize].bindings_used, "{g:?}");
    }
}

/// The model's binding table describes what the SPIR-V declares (rectangle samplers as
/// 2D, emulated comparison samplers as plain samplers), and emulated uses are flagged.
#[test]
fn binding_kinds_match_the_spirv() {
    let mut files = base_files();
    files.retain(|(n, _)| n != "composite1.fsh");
    files.push((
        "composite1.fsh".into(),
        "#version 130\nuniform sampler2DShadow shadowtex0;\nuniform sampler2DRect colortex5;\nuniform usampler2DRect colortex6;\nuniform sampler2DRectShadow depthtex1;\nin vec2 texcoord;\nvoid main() {\n    float s = shadow2D(shadowtex0, vec3(texcoord, 0.5)).r + texture(depthtex1, vec3(texcoord * 4.0, 0.5));\n    gl_FragData[0] = vec4(s) + texture2DRect(colortex5, texcoord * 4.0) + vec4(texelFetch(colortex6, ivec2(1)));\n}\n".into(),
    ));
    let pack = pack_of(&files);
    for comparison_samplers in [true, false] {
        let mut settings = vulkan_only();
        settings.env.device.comparison_samplers = comparison_samplers;
        let out = compile_pack(&pack, &settings);
        assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
        let dim = &out.pack.dimensions[0];
        let kind = |n: &str| dim.bindings.get(n).unwrap_or_else(|| panic!("no binding {n}")).kind.clone();
        let sampler = |dim: &str, shadow: bool, st: &str| ResourceKind::Sampler { dim: dim.into(), shadow, sample_type: st.into() };
        assert_eq!(kind("shadowtex0"), sampler("2d", comparison_samplers, "float"));
        assert_eq!(kind("depthtex1"), sampler("2d", comparison_samplers, "float"));
        assert_eq!(kind("colortex5"), sampler("2d", false, "float"));
        assert_eq!(kind("colortex6"), sampler("2d", false, "uint"));
        let c1 = program(dim, "composite1");
        for name in ["shadowtex0", "depthtex1", "colortex5", "colortex6"] {
            let used = c1.bindings_used.iter().find(|b| b.name == name).unwrap();
            assert_eq!(used.shadow_emulated, !comparison_samplers && name != "colortex5" && name != "colortex6", "{name}");
        }
        assert_reflection_matches(&out);
    }
}

/// Every descriptor of every SPIR-V module has a binding-table entry at its set/binding
/// whose kind is the reflected kind.
fn assert_reflection_matches(out: &CompileOutput) {
    for dim in &out.pack.dimensions {
        for p in &dim.programs {
            for st in &p.stages {
                let Some(words) = st.spirv.and_then(|b| out.blobs.get_spirv(b)) else { continue };
                let refl = sb_compile::reflect(&words).unwrap();
                for d in &refl.descriptors {
                    let Some(reflected) = resource_kind_of(&d.kind) else { continue };
                    if matches!(reflected, ResourceKind::UniformBuffer) {
                        continue;
                    }
                    let e = dim.bindings.entries.iter().find(|e| e.set == d.set && e.binding == d.binding);
                    let e = e.unwrap_or_else(|| panic!("{}: no entry for {} at {}/{}", p.name, d.name, d.set, d.binding));
                    let strip = |k: &ResourceKind| match k {
                        ResourceKind::StorageImage { dim, sample_type, .. } => format!("image {dim} {sample_type}"),
                        k => format!("{k:?}"),
                    };
                    assert_eq!(strip(&e.kind), strip(&reflected), "{}: `{}`", p.name, e.name);
                }
            }
        }
    }
}

/// One `gbuffers_terrain` draws solid, cutout and translucent terrain. As in Iris, it keeps
/// the blend of each vanilla draw (solid and cutout do not blend, water does) and each slot
/// tests alpha with Iris' reference for its geometry; a `blend.` / `alphaTest.` directive
/// applies to every slot the program draws.
#[test]
fn slots_carry_iris_blend_and_alpha_defaults() {
    let out = compile_pack(&pack_of(&base_files()), &vulkan_only());
    assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
    let dim = &out.pack.dimensions[0];
    let slot = |g: GeometryProgram| &dim.geometry[&g];
    let terrain = &dim.programs[slot(GeometryProgram::TerrainSolid).program as usize];
    assert!(terrain.inherit_blend);
    assert_eq!(terrain.alpha_test.map(|a| a.func), Some(sb_core::program::AlphaFunc::Greater));
    let solid = slot(GeometryProgram::TerrainSolid);
    let cutout = slot(GeometryProgram::TerrainCutout);
    let water = slot(GeometryProgram::Water);
    assert_eq!(solid.blend_for(terrain), None);
    assert_eq!(cutout.blend_for(terrain), None);
    assert_eq!(water.blend_for(terrain), Some(sb_core::program::BlendMode::TRANSLUCENT));
    assert_eq!(solid.alpha_test_ref(terrain), f32::MIN);
    assert_eq!(cutout.alpha_test_ref(terrain), 0.5);
    assert_eq!(water.alpha_test_ref(terrain), 0.1);
    // Composite-style programs blend only through directives (Iris `CompositeRenderer`).
    for p in dim.programs.iter().filter(|p| matches!(p.kind, sb_core::model::ProgramKind::Composite { .. })) {
        assert!(!p.inherit_blend, "{}", p.name);
    }

    let mut files = base_files();
    files.push((
        "shaders.properties".into(),
        "blend.gbuffers_terrain=ONE ZERO ONE ZERO\nalphaTest.gbuffers_terrain=GREATER 0.3\n".into(),
    ));
    let out = compile_pack(&pack_of(&files), &vulkan_only());
    assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
    let dim = &out.pack.dimensions[0];
    let terrain = &dim.programs[dim.geometry[&GeometryProgram::TerrainSolid].program as usize];
    assert!(!terrain.inherit_blend);
    for g in [GeometryProgram::TerrainSolid, GeometryProgram::TerrainCutout, GeometryProgram::Water] {
        let slot = &dim.geometry[&g];
        assert_eq!(slot.alpha_test_ref(terrain), 0.3, "{g:?}");
        assert_eq!(slot.blend_for(terrain).map(|b| b.src_color), Some(sb_core::program::BlendFactor::One), "{g:?}");
    }
}

/// A long-lived session recompiling after every option change keeps at most two compiles'
/// worth of cache entries in every table (preprocessed and analyzed stages, variants,
/// SPIR-V, validation, Renderpearl checks), however many recompiles it runs.
#[test]
fn session_caches_stay_bounded_across_recompiles() {
    let files: Vec<(String, String)> = base_files()
        .into_iter()
        .map(|(name, text)| {
            if name.ends_with(".fsh") {
                let text = text.replacen("#version 120\n", "#version 120\n#define LEVEL 1 // [1 2 3 4 5 6 7 8]\nconst float sbLevel = float(LEVEL);\n", 1);
                (name, text)
            } else {
                (name, text)
            }
        })
        .collect();
    let pack = pack_of(&files);
    let mut settings = vulkan_only();
    settings.validate_spirv = true;
    settings.env.targets = vec![OutputTarget::Vulkan, OutputTarget::Renderpearl];
    let mut session = PackSession::new(&pack, settings);
    let first = session.compile();
    assert!(errors(&first).is_empty(), "{:#?}", errors(&first));
    let one = session.caches.entry_counts();
    assert!(one.iter().all(|&n| n > 0), "{one:?}");
    for level in 2..=8 {
        session.set_option_values(OptionValues::from_pairs([("LEVEL", level.to_string().as_str())]));
        let out = session.compile();
        assert!(errors(&out).is_empty(), "{:#?}", errors(&out));
        let now = session.caches.entry_counts();
        for (table, (n, limit)) in now.iter().zip(one.iter()).enumerate() {
            assert!(*n <= 2 * limit, "table {table} holds {n} entries after {level} compiles (one compile: {limit})");
        }
    }
}

/// glslang's diagnostics are cached with their pack locations: after lines move in a pack
/// file between two compiles of a session, the recompile reports the new lines.
#[test]
fn session_reports_moved_lines() {
    let dir = tempfile::tempdir().unwrap();
    let shaders = dir.path().join("shaders");
    std::fs::create_dir_all(&shaders).unwrap();
    for (name, text) in base_files() {
        std::fs::write(shaders.join(name), text).unwrap();
    }
    let body = "uniform sampler2D colortex0;\nuniform int frameCounter;\nvarying vec2 texcoord;\nvoid main() {\n    vec4 c = texture2D(colortex0, texcoord);\n    switch (frameCounter) { case 0: c.r = 1.0; break; default: }\n    gl_FragColor = c;\n}\n";
    std::fs::write(shaders.join("final.fsh"), format!("#version 130\n{body}")).unwrap();
    let pack = ShaderPack::open(dir.path()).unwrap();
    let mut session = PackSession::new(&pack, vulkan_only());
    let lines = |out: &CompileOutput| -> Vec<u32> {
        out.pack
            .diagnostics
            .iter()
            .filter(|d| d.code.starts_with("spv.") && d.location.as_ref().is_some_and(|l| l.file.ends_with("final.fsh")))
            .filter_map(|d| d.location.as_ref().map(|l| l.line))
            .collect()
    };
    let first = session.compile();
    let before = lines(&first);
    assert!(!before.is_empty(), "no glslang diagnostic for final.fsh: {:#?}", first.pack.diagnostics.iter().map(|d| d.to_string()).collect::<Vec<_>>());
    std::fs::write(shaders.join("final.fsh"), format!("#version 130\n// 1\n// 2\n// 3\n// 4\n// 5\n{body}")).unwrap();
    session.set_settings(vulkan_only());
    let second = session.compile();
    let after = lines(&second);
    assert_eq!(after, before.iter().map(|l| l + 5).collect::<Vec<_>>());
    let fresh = compile_pack(&pack, &vulkan_only());
    assert_eq!(lines(&fresh), after);
}

/// An on-demand variant reports its own diagnostics: here why it needs the raw Vulkan path.
#[test]
fn compile_variant_returns_its_diagnostics() {
    let mut s = vulkan_only();
    s.env.device.max_descriptors_per_program = Some(1);
    let pack = pack_of(&base_files());
    let mut session = PackSession::new(&pack, s);
    let v = compile_variant(&mut session, "", GeometryProgram::TerrainSolid, "sodium_terrain").unwrap();
    assert!(v.program.requires_raw_vulkan);
    assert!(v.diagnostics.iter().any(|d| d.code == "xf.too-many-descriptors"), "{:?}", v.diagnostics);
}
