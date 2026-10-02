//! Renders a hand-written minimal pack (terrain, entities, sky, DH LODs, water, shadows,
//! a compute pass, composite lighting and final) on lavapipe with validation.

mod common;

use common::*;
use sb_core::model::DepthMode;
use sb_runtime::{NoTextures, PngRenderSettings, RenderRequest, RuntimeOptions, SceneParams};

#[test]
fn minimal_pack_renders_terrain_and_sky() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    let (pack, blobs) = test_pack(Variant::default());
    let out = render(&mut rt, &pack, &blobs, small_scene(), (320, 180), 2, true);
    out.image.save(render_dir().join("minimal_pack.png")).ok();
    for (name, img) in &out.targets {
        img.save(render_dir().join(format!("minimal_pack_{name}.png"))).ok();
    }
    eprintln!("stats: {:#?}", out.stats);
    assert_no_validation_errors(&out);
    assert!(out.stats.programs_skipped.is_empty(), "{:?}", out.stats.programs_skipped);
    assert_eq!(out.stats.frames, 2);
    assert!(out.stats.draws > 50, "{}", out.stats.draws);
    assert_eq!(out.stats.dispatches, 1);
    // Geometry the pack has no program for is recorded.
    assert!(out.stats.geometry_skipped.iter().any(|g| g.starts_with("gbuffers_skytextured")), "{:?}", out.stats.geometry_skipped);
    assert!(out.stats.geometry_skipped.iter().any(|g| g.starts_with("shadow_entities")), "{:?}", out.stats.geometry_skipped);

    // Not uniform: sky at the top, terrain at the bottom.
    let img = &out.image;
    assert!(luminance_variance(img) > 0.002, "variance {}", luminance_variance(img));
    let (w, h) = img.dimensions();
    let avg = |y0: u32, y1: u32| {
        let mut s = [0f64; 3];
        let mut n = 0.0;
        for y in y0..y1 {
            for x in 0..w {
                let p = img.get_pixel(x, y);
                for c in 0..3 {
                    s[c] += f64::from(p[c]);
                }
                n += 1.0;
            }
        }
        s.map(|v| v / n)
    };
    let top = avg(0, h / 10);
    let bottom = avg(h * 9 / 10, h);
    eprintln!("top {top:?} bottom {bottom:?}");
    assert!(top[2] > top[0], "sky should be blue: {top:?}");
    assert!((top[0] - bottom[0]).abs() + (top[1] - bottom[1]).abs() + (top[2] - bottom[2]).abs() > 30.0, "top {top:?} bottom {bottom:?}");
    // The custom uniform, the uniform default, the compute pass and the SSBO all feed the
    // final multiplier: a black image means one of them failed.
    assert!(bottom.iter().sum::<f64>() > 20.0, "{bottom:?}");
    // Captured targets.
    let names: Vec<&str> = out.targets.iter().map(|(n, _)| n.as_str()).collect();
    for n in ["colortex0", "colortex1", "colortex2", "shadowcolor0", "depthtex0", "depthtex1", "shadowtex0"] {
        assert!(names.contains(&n), "{names:?}");
    }
    // The shadow map has geometry (not all far plane).
    let shadow = &out.targets.iter().find(|(n, _)| n == "shadowtex0").unwrap().1;
    assert!(shadow.pixels().any(|p| p[0] < 250), "empty shadow map");
}

#[test]
fn dh_lods_beyond_render_distance_are_visible() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    let (with, blobs) = test_pack(Variant::default());
    let (without, blobs2) = test_pack(Variant { without_dh_programs: true, ..Default::default() });
    let scene = SceneParams { render_distance: 2, dh_render_distance: 16, pitch: 4.0, ..Default::default() };
    let a = render(&mut rt, &with, &blobs, scene.clone(), (256, 144), 1, false);
    let b = render(&mut rt, &without, &blobs2, scene, (256, 144), 1, false);
    a.image.save(render_dir().join("dh_with_lods.png")).ok();
    b.image.save(render_dir().join("dh_without_lods.png")).ok();
    assert_no_validation_errors(&a);
    assert_no_validation_errors(&b);
    assert!(b.stats.geometry_skipped.iter().any(|g| g.starts_with("dh_terrain")), "{:?}", b.stats.geometry_skipped);
    // LODs fill pixels that are sky without them.
    let changed = a.image.pixels().zip(b.image.pixels()).filter(|(p, q)| (0..3).map(|c| p[c].abs_diff(q[c]) as u32).sum::<u32>() > 30).count();
    let total = (a.image.width() * a.image.height()) as usize;
    eprintln!("{changed} of {total} pixels differ");
    assert!(changed > total / 50, "{changed} of {total} pixels differ");
}

#[test]
fn native_dh_uses_separate_depth() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    let (pack, blobs) = test_pack(Variant { native_dh: true, ..Default::default() });
    let out = render(&mut rt, &pack, &blobs, SceneParams { render_distance: 4, dh_render_distance: 16, ..Default::default() }, (256, 144), 1, true);
    out.image.save(render_dir().join("native_dh.png")).ok();
    assert!(luminance_variance(&out.image) > 0.002, "variance {}", luminance_variance(&out.image));
    assert_no_validation_errors(&out);
    assert!(out.stats.programs_skipped.is_empty(), "{:?}", out.stats.programs_skipped);
    let dh = &out.targets.iter().find(|(n, _)| n == "dhDepthTex0").expect("dhDepthTex0 captured").1;
    assert!(dh.pixels().any(|p| p[0] < 255), "DH depth is empty");
}

#[test]
fn depth_mode_parity_forward_vs_reversed() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    for native_dh in [false, true] {
        let (fwd, fb) = test_pack(Variant { native_dh, ..Default::default() });
        let (rev, rb) = test_pack(Variant { native_dh, depth_mode: DepthMode::ReversedZeroToOne, ..Default::default() });
        let scene = small_scene();
        let a = render(&mut rt, &fwd, &fb, scene.clone(), (256, 144), 2, false);
        let b = render(&mut rt, &rev, &rb, scene, (256, 144), 2, false);
        assert_no_validation_errors(&a);
        assert_no_validation_errors(&b);
        a.image.save(render_dir().join(format!("parity_forward{}.png", if native_dh { "_native" } else { "" }))).ok();
        b.image.save(render_dir().join(format!("parity_reversed{}.png", if native_dh { "_native" } else { "" }))).ok();
        let d = mean_abs_diff(&a.image, &b.image);
        eprintln!("native_dh={native_dh}: mean abs diff {d}");
        assert!(d < 2.0 / 255.0, "forward and reversed renders differ: {d}");
        assert!(luminance_variance(&a.image) > 0.001);
    }
}

#[test]
fn gl_depth_mode_with_depth_clip_control() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    if !rt.device_info().depth_clip_control {
        eprintln!("skipping: no VK_EXT_depth_clip_control");
        return;
    }
    // A GL-convention pack: no depth remap at all. Our forward shaders remap, so build
    // the pack in forward mode and just check that the request is served cleanly.
    let (pack, blobs) = test_pack(Variant::default());
    let out = rt
        .render(&RenderRequest {
            pack: &pack,
            blobs: &blobs,
            dimension: "world0",
            width: 64,
            height: 36,
            frames: 1,
            scene: small_scene(),
            depth_mode: DepthMode::GlNegOneToOne,
            textures: &NoTextures,
            capture_targets: false,
        })
        .expect("render");
    assert_no_validation_errors(&out);
    assert!(out.stats.warnings.iter().any(|w| w.contains("depth mode")), "{:?}", out.stats.warnings);
}

#[test]
fn invalid_requests_are_errors_not_panics() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    let (pack, blobs) = test_pack(Variant::default());
    let req = |dimension: &'static str, width: u32| RenderRequest {
        pack: &pack,
        blobs: &blobs,
        dimension,
        width,
        height: 16,
        frames: 1,
        scene: small_scene(),
        depth_mode: DepthMode::ForwardZeroToOne,
        textures: &NoTextures,
        capture_targets: false,
    };
    assert!(rt.render(&req("world-1", 16)).is_err());
    assert!(rt.render(&req("world0", 0)).is_err());
    assert!(rt.render(&req("world0", 100_000)).is_err());
}

#[test]
fn broken_programs_are_skipped_not_fatal() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    let (mut pack, mut blobs) = test_pack(Variant::default());
    let dim = &mut pack.dimensions[0];
    // 1. A program whose SPIR-V blob is garbage.
    let bad = blobs.push(sb_core::model::BlobKind::Spirv, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    dim.programs[4].stages[1].spirv = Some(bad);
    // 2. A geometry slot pointing at a program that does not exist.
    dim.geometry.insert(sb_core::program::GeometryProgram::SkyTextured, sb_core::model::GeometrySlot { program: 99, resolved_from: sb_core::program::GeometryProgram::SkyTextured });
    // 3. A pass referencing a missing compute program and a bogus flip state.
    dim.passes[3].computes.push(1234);
    dim.passes[4].flip_state = vec![false; 3];
    let out = render(&mut rt, &pack, &blobs, small_scene(), (128, 72), 1, false);
    assert_no_validation_errors(&out);
    assert!(out.stats.programs_skipped.iter().any(|s| s.name == "gbuffers_water"), "{:?}", out.stats.programs_skipped);
    assert!(out.stats.warnings.iter().any(|w| w.contains("99")), "{:?}", out.stats.warnings);
    assert!(out.stats.warnings.iter().any(|w| w.contains("1234")), "{:?}", out.stats.warnings);
    assert!(out.stats.warnings.iter().any(|w| w.contains("flip_state")), "{:?}", out.stats.warnings);
}

#[test]
fn render_to_png_writes_files() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if runtime().is_none() {
        return;
    }
    let (pack, blobs) = test_pack(Variant::default());
    let dir = render_dir().join("render_to_png");
    let settings = PngRenderSettings {
        output: dir.join("out.png"),
        width: 96,
        height: 54,
        frames: 1,
        dimension: "world0".into(),
        scene: small_scene(),
        depth_mode: None,
        runtime: RuntimeOptions { validation: true, prefer_cpu_device: true, device_name_filter: None },
        capture_dir: Some(dir.join("targets")),
    };
    let out = sb_runtime::render_to_png(&pack, &blobs, &NoTextures, &settings).expect("render_to_png");
    assert_no_validation_errors(&out);
    let png = image::open(dir.join("out.png")).expect("png written").to_rgba8();
    assert_eq!(png.dimensions(), (96, 54));
    assert!(dir.join("targets/colortex0.png").exists());
}
