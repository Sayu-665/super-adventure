//! Regression tests for host-contract bugs found in the adversarial review of the
//! runtime:
//!
//! * depthtex2 and the `centerDepthSmooth` sample are taken at Iris' `beginHand`, i.e.
//!   before translucent geometry, like depthtex1 (they used to be taken after water);
//! * a depth texture declared as a shadow sampler gets a comparison sampler (lookups
//!   used to go through a plain sampler);
//! * cutout terrain is back-face culled like every Minecraft terrain layer unless
//!   `backFace.cutout` asks for back faces (leaves used to be drawn double-sided);
//! * fullscreen draw buffers the fragment shader never writes are masked, so they keep
//!   their contents (Vulkan leaves such attachments undefined; lavapipe happens to
//!   preserve them and the validation layer does not flag it, so this test documents
//!   the contract rather than reproducing a failure);
//! * the runtime can be created, used and dropped repeatedly (a smoke test; object and
//!   memory leaks are checked precisely by the `lifetime_tests` unit test).
//!
//! The depth-copy, comparison-sampler and culling tests fail on the code before their
//! fixes.

mod common;

use common::shaders::PRELUDE;
use common::{GPU_LOCK, assert_no_validation_messages, mean_abs_diff, render, render_dir, runtime, small_scene, test_pack, Variant};
use sb_compile::{CompileOptions, compile_glsl};
use sb_core::model::*;
use sb_core::{GlslType, ShaderStage};
use sb_runtime::SceneParams;

/// Index of the `final` program in the test pack.
const FINAL: usize = 8;
/// Index of the `composite` program in the test pack.
const COMPOSITE: usize = 6;

fn compile(src: &str, stage: ShaderStage) -> Vec<u32> {
    compile_glsl(src, stage, "review", &CompileOptions::default(), None).unwrap_or_else(|e| panic!("{e}\n{}\n{src}", e.log))
}

fn dim(p: &mut CompiledPack) -> &mut DimensionPipeline {
    &mut p.dimensions[0]
}

/// Replace the fragment stage of program `index`.
fn replace_fragment(pack: &mut CompiledPack, blobs: &mut BlobTable, index: usize, src: &str) {
    let words = compile(src, ShaderStage::Fragment);
    let id = blobs.push_spirv(&words);
    let p = &mut dim(pack).programs[index];
    for s in &mut p.stages {
        if s.stage == ShaderStage::Fragment {
            s.spirv = Some(id);
        }
    }
}

fn add_sampler(pack: &mut CompiledPack, name: &str, binding: u32, shadow: bool, resource: ResourceRef) {
    let kind = ResourceKind::Sampler { dim: "2d".into(), shadow, sample_type: "float".into() };
    let entries = &mut dim(pack).bindings.entries;
    entries.retain(|e| e.name != name);
    entries.push(BindingEntry { name: name.into(), set: 1, binding, kind, resource });
}

/// The camera above a sea (seed 1): the screen centre shows water over a floor five
/// blocks deeper.
fn water_scene(pitch: f64) -> SceneParams {
    SceneParams { camera_xz: [-87.5, 0.5], camera_height: 3.0, pitch, yaw: 0.0, render_distance: 2, dh_render_distance: 0, entities: false, ..Default::default() }
}

fn channel_count(img: &image::RgbaImage, c: usize, pred: impl Fn(u8) -> bool) -> usize {
    img.pixels().filter(|p| pred(p[c])).count()
}

/// depthtex2 holds the depth before translucents (and the hand), exactly like depthtex1
/// in a scene without a hand; `centerDepthSmooth` is the depth at the screen centre
/// before translucents (Iris samples it at `beginHand`).
#[test]
fn depthtex2_and_center_depth_are_taken_before_translucents() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    let (mut pack, mut blobs) = test_pack(Variant::default());
    add_sampler(&mut pack, "depthtex2", 14, false, ResourceRef::DepthTex(2));
    {
        let f = &mut dim(&mut pack).uniforms.frame;
        f.members.push(BlockMember { name: "centerDepthSmooth".into(), ty: GlslType::FLOAT, offset: 592, source: UniformSource::Builtin("centerDepthSmooth".into()), default: None });
        f.size = 608;
    }
    let src = format!(
        "#version 460\n{PRELUDE}{}",
        r#"
layout(std140, set = 0, binding = 0) uniform sb_Frame {
    layout(offset = 556) float viewWidth;
    layout(offset = 560) float viewHeight;
    layout(offset = 592) float centerDepthSmooth;
};
layout(set = 1, binding = 5) uniform sampler2D depthtex0;
layout(set = 1, binding = 8) uniform sampler2D depthtex1;
layout(set = 1, binding = 14) uniform sampler2D depthtex2;
layout(location = 0) in vec2 texcoord;
layout(location = 0) out vec4 sb_FragData0;
void main() {
    float d0 = texture(depthtex0, texcoord).r;
    float d1 = texture(depthtex1, texcoord).r;
    float d2 = texture(depthtex2, texcoord).r;
    ivec2 centre = ivec2(int(viewWidth) / 2, int(viewHeight) / 2);
    float c0 = texelFetch(depthtex0, centre, 0).r;
    float c1 = texelFetch(depthtex1, centre, 0).r;
    sb_FragData0 = vec4(d2 == d1 ? 1.0 : 0.0, d0 != d1 ? 1.0 : 0.0,
                        abs(centerDepthSmooth - c1) < 1e-7 ? 1.0 : 0.0,
                        abs(centerDepthSmooth - c0) < 1e-7 ? 0.0 : 1.0);
}
"#
    );
    replace_fragment(&mut pack, &mut blobs, FINAL, &src);
    let out = render(&mut rt, &pack, &blobs, water_scene(90.0), (64, 48), 2, false);
    out.image.save(render_dir().join("review_depth_copies.png")).ok();
    assert_no_validation_messages(&out);
    assert!(out.stats.programs_skipped.is_empty(), "{:?}", out.stats.programs_skipped);
    let n = (64 * 48) as usize;
    // Precondition: water covers part of the image (depthtex0 differs from depthtex1).
    assert!(channel_count(&out.image, 1, |v| v == 255) > n / 4, "no water in view");
    // depthtex2 == depthtex1 everywhere.
    assert_eq!(channel_count(&out.image, 0, |v| v == 255), n, "depthtex2 differs from depthtex1");
    // The centre depth is the pre-translucent depth, not the water surface.
    assert_eq!(channel_count(&out.image, 2, |v| v == 255), n, "centerDepthSmooth is not depthtex1 at the centre");
    assert_eq!(channel_count(&out.image, 3, |v| v == 255), n, "centerDepthSmooth is the post-translucent depth");
}

/// `sampler2DShadow depthtex1` compares against the stored depth (LEQUAL in forward
/// mode) instead of returning raw depth through a non-comparison sampler.
#[test]
fn depth_texture_shadow_sampler_compares() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    let (mut pack, mut blobs) = test_pack(Variant::default());
    add_sampler(&mut pack, "depthtex1", 8, true, ResourceRef::DepthTex(1));
    add_sampler(&mut pack, "depthtex2", 14, false, ResourceRef::DepthTex(2));
    let src = format!(
        "#version 460\n{PRELUDE}{}",
        r#"
layout(set = 1, binding = 8) uniform sampler2DShadow depthtex1;
layout(set = 1, binding = 14) uniform sampler2D depthtex2;
layout(location = 0) in vec2 texcoord;
layout(location = 0) out vec4 sb_FragData0;
void main() {
    float d = texture(depthtex2, texcoord).r;
    float lit = texture(depthtex1, vec3(texcoord, d - 0.0005));
    float shadowed = texture(depthtex1, vec3(texcoord, d + 0.0005));
    sb_FragData0 = vec4(lit, d < 0.999 ? shadowed : 0.0, 0.0, 1.0);
}
"#
    );
    replace_fragment(&mut pack, &mut blobs, FINAL, &src);
    let out = render(&mut rt, &pack, &blobs, small_scene(), (64, 48), 1, false);
    assert_no_validation_messages(&out);
    let n = (64 * 48) as usize;
    assert_eq!(channel_count(&out.image, 0, |v| v == 255), n, "reference below the stored depth must pass");
    assert_eq!(channel_count(&out.image, 1, |v| v == 0), n, "reference above the stored depth must fail");
}

/// A composite listing a draw buffer its shader never writes: the attachment is
/// masked (no undefined values, no validation message) and the buffer keeps the
/// contents of the image it flips to.
#[test]
fn fullscreen_buffers_without_shader_output_are_masked() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    let (mut pack, blobs) = test_pack(Variant::default());
    {
        let d = dim(&mut pack);
        // composite writes only location 0 but lists colortex2 too, and flips it.
        d.programs[COMPOSITE].draw_buffers = vec![0, 2];
        d.programs[COMPOSITE].output_slots = vec![0, 1];
        d.programs[COMPOSITE].output_types = vec!["float".into(), "float".into()];
        let composite = d.passes.iter_mut().find(|p| p.program == Some(COMPOSITE as u32)).expect("composite pass");
        composite.flips_after = vec![0, 2];
        let fin = d.passes.iter_mut().find(|p| p.program == Some(FINAL as u32)).expect("final pass");
        fin.flip_state = vec![true, false, true];
        d.targets.colortex[2].clear_color = Some([0.25, 0.5, 0.75, 1.0]);
    }
    let out = render(&mut rt, &pack, &blobs, small_scene(), (64, 48), 1, true);
    assert_no_validation_messages(&out);
    // colortex2 is read from the image the composite was bound to: untouched since the
    // frame's clear.
    let c2 = &out.targets.iter().find(|(n, _)| n == "colortex2").expect("colortex2 captured").1;
    assert!(c2.pixels().all(|p| p.0 == [64, 128, 191, 255]), "colortex2 alt was written: {:?}", c2.get_pixel(10, 10));
}

/// Leaves (cutout terrain) are back-face culled by default; `backFace.cutout=true`
/// renders their back faces, which are visible through the cutout holes.
#[test]
fn cutout_back_faces_follow_back_face_setting() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    let (pack, blobs) = test_pack(Variant::default());
    let mut with_back_faces = pack.clone();
    dim(&mut with_back_faces).settings.back_face.insert("cutout".into(), true);
    let scene = SceneParams { entities: false, ..small_scene() };
    let a = render(&mut rt, &pack, &blobs, scene.clone(), (160, 90), 1, false);
    let b = render(&mut rt, &with_back_faces, &blobs, scene, (160, 90), 1, false);
    assert_no_validation_messages(&a);
    assert_no_validation_messages(&b);
    a.image.save(render_dir().join("review_leaves_culled.png")).ok();
    b.image.save(render_dir().join("review_leaves_back_faces.png")).ok();
    let d = mean_abs_diff(&a.image, &b.image);
    assert!(d > 0.0, "back faces of leaves are drawn although backFace.cutout is off");
    // The setting only touches leaves: the difference stays small.
    assert!(d < 0.05, "{d}");
}

/// Creating, using and dropping the runtime repeatedly works and stays clean.
#[test]
fn runtime_can_be_recreated_repeatedly() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (pack, blobs) = test_pack(Variant::default());
    for i in 0..4 {
        let Some(mut rt) = runtime() else { return };
        for _ in 0..2 {
            let out = render(&mut rt, &pack, &blobs, SceneParams { render_distance: 1, dh_render_distance: 3, seed: i, ..Default::default() }, (32, 24), 1, i % 2 == 0);
            assert_no_validation_messages(&out);
        }
        drop(rt);
    }
}
