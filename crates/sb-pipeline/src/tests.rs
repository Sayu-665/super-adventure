//! End-to-end unit tests on small in-memory packs.

use super::*;
use pretty_assertions::assert_eq;
use sb_core::PassGroup;
use sb_core::model::{DhStrategy, OutputTarget, ResourceRef};

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
    let (p, blobs) = compile_variant(&mut session, "", GeometryProgram::Block, "vanilla_entity").unwrap();
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

#[test]
fn probe_variant_profiles() {
    let mut files = base_files();
    files.push(("shadow.vsh".into(), TERRAIN_VSH.into()));
    files.push(("shadow.fsh".into(), "#version 120\nvarying vec2 uv;\nvarying vec4 color;\nvoid main() { gl_FragData[0] = color; }\n".into()));
    let pack = pack_of(&files);
    let mut session = PackSession::new(&pack, vulkan_only());
    session.compile();
    for p in sb_transform::builtin_profiles() {
        for g in [GeometryProgram::TerrainSolid, GeometryProgram::Water, GeometryProgram::ShadowSolid, GeometryProgram::Entities, GeometryProgram::DhTerrain] {
            let r = compile_variant(&mut session, "", g, &p.name);
            eprintln!("PROBE {:28} {:?}: {}", p.name, g, match &r { Ok(_) => "ok".to_string(), Err(d) => d.iter().filter(|x| x.is_error()).map(|x| x.to_string()).next().unwrap_or_default() });
        }
    }
}
