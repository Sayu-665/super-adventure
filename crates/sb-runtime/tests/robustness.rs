//! Model inconsistencies must never panic, fail the render or produce Vulkan validation
//! errors: each mutation of the minimal pack below renders a tiny frame and must come
//! back `Ok` with zero validation errors (the problem is recorded in the stats instead).

mod common;

use common::{GPU_LOCK, Variant, runtime, small_scene, test_pack};
use sb_core::model::*;
use sb_core::program::GeometryProgram;
use sb_core::{GlslType, PassGroup, TextureFormat};
use sb_runtime::{NoTextures, RenderRequest};

type Mutation = (&'static str, fn(&mut CompiledPack, &mut BlobTable));

fn dim(p: &mut CompiledPack) -> &mut DimensionPipeline {
    &mut p.dimensions[0]
}

fn set_resource(p: &mut CompiledPack, name: &str, r: ResourceRef) {
    if let Some(e) = dim(p).bindings.entries.iter_mut().find(|e| e.name == name) {
        e.resource = r;
    }
}

const MUTATIONS: &[Mutation] = &[
    ("draw buffer out of range", |p, _| dim(p).programs[0].draw_buffers = vec![0, 40, 2]),
    ("output slots inconsistent", |p, _| dim(p).programs[0].output_slots = vec![2, 1, 0, 7]),
    ("gbuffer attachments without writers", |p, _| dim(p).gbuffer_attachments = vec![0, 1, 2, 5, 9]),
    ("no shared attachments", |p, _| {
        dim(p).gbuffer_attachments.clear();
        dim(p).shadow_attachments.clear();
    }),
    ("sampler bound to shadow hw / missing depth", |p, _| {
        set_resource(p, "colortex1", ResourceRef::ShadowTexHw(5));
        set_resource(p, "colortex2", ResourceRef::DepthTex(9));
    }),
    ("sampler bound to missing custom things", |p, _| {
        set_resource(p, "noisetex", ResourceRef::CustomTexture("composite.nope".into()));
        set_resource(p, "depthtex1", ResourceRef::Image("nope".into()));
        set_resource(p, "shadowcolor0", ResourceRef::Ssbo(3));
        set_resource(p, "dhDepthTex0", ResourceRef::DhDepthTex(7));
    }),
    ("shadow sampler bound to a colour target", |p, _| set_resource(p, "shadowtex1", ResourceRef::ColorTex(0))),
    ("ssbo missing", |p, _| dim(p).targets.buffers.clear()),
    ("integer and exotic target formats", |p, _| {
        let t = &mut dim(p).targets.colortex;
        t[0].format = TextureFormat::RGB9_E5;
        t[1].format = TextureFormat::RGBA32UI;
        t[2].format = TextureFormat::R11F_G11F_B10F;
    }),
    ("odd target sizes", |p, _| {
        let t = &mut dim(p).targets.colortex;
        t[1].size = TargetSize::Absolute { width: 7, height: 3 };
        t[2].size = TargetSize::PerAxis { x: AxisSize::Relative(0.5), y: AxisSize::Absolute(5) };
    }),
    ("flip schedule garbage", |p, _| {
        for pass in &mut dim(p).passes {
            pass.flip_state = vec![true; 100];
            pass.flips_after = vec![99, 0, 0, 31];
        }
        dim(p).end_of_frame_copies = vec![0, 7, 1000];
    }),
    ("passes out of order and duplicated", |p, _| {
        let d = dim(p);
        let final_pass = d.passes.pop().unwrap();
        d.passes.insert(0, final_pass);
        let opaque = d.passes[2].clone();
        d.passes.push(opaque);
    }),
    ("no passes at all", |p, _| dim(p).passes.clear()),
    ("pass program indices wrong", |p, _| {
        let d = dim(p);
        d.passes[3].program = Some(0); // a geometry program as a fullscreen pass
        d.passes[4].program = Some(7); // a compute program as final
        d.passes[3].computes = vec![6, 500];
    }),
    ("huge and zero dispatches", |p, _| {
        dim(p).programs[7].compute = Some(ComputeInfo { local_size: [8, 8, 1], work_groups: WorkGroups::Absolute { x: 1 << 30, y: 0, z: 1 }, indirect: None });
    }),
    ("indirect dispatch from a missing buffer", |p, _| {
        dim(p).programs[7].compute = Some(ComputeInfo { local_size: [8, 8, 1], work_groups: WorkGroups::Relative { x: 1.0, y: 1.0 }, indirect: Some((9, 4)) });
    }),
    ("viewport scale nonsense", |p, _| {
        dim(p).programs[6].viewport = ViewportScale { scale: f32::NAN, offset_x: -3.0, offset_y: f32::INFINITY };
        dim(p).programs[8].viewport = ViewportScale { scale: -1.0, offset_x: 0.0, offset_y: 0.0 };
    }),
    ("mipmaps on missing and integer targets", |p, _| {
        dim(p).programs[6].mipmap_targets = vec![0, 1, 17];
        dim(p).targets.colortex[1].format = TextureFormat::R32UI;
        dim(p).targets.colortex[1].mipmap_programs = vec![6];
        dim(p).targets.colortex[0].mipmap_programs = vec![6];
    }),
    ("shadow settings extreme", |p, _| {
        let s = &mut dim(p).targets.shadow;
        s.resolution = 1;
        s.distance = 0.0;
        s.interval_size = f32::NAN;
        s.near_plane = 10.0;
        s.far_plane = 10.0;
        s.fov = Some(-5.0);
    }),
    ("shadow disabled with shadow programs", |p, _| dim(p).targets.shadow.enabled = false),
    ("custom uniform garbage", |p, _| {
        dim(p).custom_uniforms.push(CustomUniform { name: "x y".into(), ty: GlslType::FLOAT, expression: "((((".into(), is_variable: false, location: None });
        dim(p).custom_uniforms[0].expression = "frameTimeCounter / 0 + undefinedThing".into();
    }),
    ("uniform layout garbage", |p, _| {
        let f = &mut dim(p).uniforms.frame;
        f.members.push(BlockMember { name: "big".into(), ty: GlslType::MAT4.with_array(4), offset: 100_000, source: UniformSource::Builtin("gbufferModelView".into()), default: Some(vec![1.0; 3]) });
        f.members.push(BlockMember { name: "arr".into(), ty: GlslType::FLOAT.with_array(3), offset: 580, source: UniformSource::Unset, default: Some(vec![1.0; 99]) });
        f.size = 16;
        dim(p).uniforms.draw.size = 0;
    }),
    ("dh native without dh programs", |p, _| {
        dim(p).distant_horizons = DhPipeline { strategy: DhStrategy::Native, unified_projection: false, shadow_enabled: true };
        dim(p).geometry.shift_remove(&GeometryProgram::DhTerrain);
    }),
    ("geometry slot pointing at a compute program", |p, _| {
        dim(p).geometry.insert(GeometryProgram::Entities, GeometrySlot { program: 7, resolved_from: GeometryProgram::Entities });
    }),
    ("stage blobs missing or swapped", |p, b| {
        let d = dim(p);
        d.programs[2].stages[0].spirv = None;
        let fs = d.programs[1].stages[1].spirv;
        d.programs[1].stages[0].spirv = fs; // fragment module in the vertex slot
        d.programs[3].stages[1].spirv = Some(BlobId(9999));
        let _ = b;
    }),
    ("empty program", |p, _| dim(p).programs[5].stages.clear()),
    ("custom textures and images that cannot load", |p, _| {
        let t = &mut dim(p).targets;
        t.custom_textures.push(CustomTexture { sampler: "a".into(), stage: "composite".into(), source: sb_core::model::TextureSource::PackImage { path: "../../etc/passwd".into() }, blur: true, clamp: false });
        t.custom_textures.push(CustomTexture {
            sampler: "b".into(),
            stage: "composite".into(),
            source: sb_core::model::TextureSource::Raw { path: "x.bin".into(), target: "4d".into(), dimensions: 4, format: TextureFormat::RGBA32F, size: [1 << 20, 1 << 20, 1 << 20], pixel_format: "RGBA".into(), pixel_type: "FLOAT".into() },
            blur: false,
            clamp: false,
        });
        t.images.push(CustomImage { name: "img".into(), sampler_name: None, format: TextureFormat::RGB9_E5, pixel_format: "RGB".into(), pixel_type: "FLOAT".into(), clear: true, size: ImageSize::Absolute3D { width: 0, height: 5, depth: 5 } });
        t.noise_texture_resolution = 0;
    }),
    ("unused dimension folder and duplicate entries", |p, _| {
        let mut extra = p.dimensions[0].clone();
        extra.folder = "world-1".into();
        extra.programs.clear();
        p.dimensions.push(extra);
        let e = p.dimensions[0].bindings.entries[7].clone();
        p.dimensions[0].bindings.entries.insert(0, e);
    }),
];

#[test]
fn malformed_models_render_without_errors() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    let mut failures = Vec::new();
    for (name, mutate) in MUTATIONS {
        let (mut pack, mut blobs) = test_pack(Variant::default());
        mutate(&mut pack, &mut blobs);
        let result = rt.render(&RenderRequest {
            pack: &pack,
            blobs: &blobs,
            dimension: "world0",
            width: 48,
            height: 27,
            frames: 2,
            scene: sb_runtime::SceneParams { render_distance: 1, dh_render_distance: 4, ..small_scene() },
            depth_mode: DepthMode::ForwardZeroToOne,
            textures: &NoTextures,
            capture_targets: true,
        });
        match result {
            Ok(out) => {
                let errors: Vec<&String> = out.validation_errors().collect();
                eprintln!(
                    "{name}: ok, {} skipped, {} warnings, {} validation errors",
                    out.stats.programs_skipped.len(),
                    out.stats.warnings.len(),
                    errors.len()
                );
                if !errors.is_empty() {
                    failures.push(format!("{name}: {} validation errors, first: {}", errors.len(), errors[0]));
                }
            }
            Err(e) => failures.push(format!("{name}: render failed: {e}")),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    let _ = PassGroup::Final;
}
