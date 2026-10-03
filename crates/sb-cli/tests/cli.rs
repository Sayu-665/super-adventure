//! Smoke tests of the `shaderbridge` commands through [`sb_cli::run`].

use std::path::Path;

const VSH: &str = "#version 120\nvarying vec2 uv;\nvarying vec4 color;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.xy; color = gl_Color; }\n";
const FSH: &str = "#version 120\nuniform sampler2D texture;\nvarying vec2 uv;\nvarying vec4 color;\n/* RENDERTARGETS: 0 */\nvoid main() { gl_FragData[0] = texture2D(texture, uv) * color; }\n";
const QUAD: &str = "#version 120\nvarying vec2 texcoord;\nvoid main() { gl_Position = ftransform(); texcoord = gl_MultiTexCoord0.xy; }\n";
const COMPOSITE: &str = "#version 120\n#define BLOOM // [comment]\n#ifdef BLOOM\n#endif\n#define STRENGTH 2 // [1 2 3]\nuniform sampler2D colortex0;\nvarying vec2 texcoord;\nvoid main() { gl_FragData[0] = texture2D(colortex0, texcoord) * float(STRENGTH); }\n";

fn write_pack(dir: &Path, broken: bool) {
    let s = dir.join("shaders");
    std::fs::create_dir_all(&s).unwrap();
    std::fs::write(s.join("gbuffers_terrain.vsh"), VSH).unwrap();
    std::fs::write(s.join("gbuffers_terrain.fsh"), if broken { "void main() { nope(); }" } else { FSH }).unwrap();
    std::fs::write(s.join("composite.vsh"), QUAD).unwrap();
    std::fs::write(s.join("composite.fsh"), COMPOSITE).unwrap();
    std::fs::write(s.join("shaders.properties"), "screen=BLOOM STRENGTH\nsliders=STRENGTH\nprofile.LOW=!BLOOM STRENGTH=1\n").unwrap();
}

fn run(args: &[&str]) -> (i32, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let argv = std::iter::once("shaderbridge".to_string()).chain(args.iter().map(|s| s.to_string()));
    let code = sb_cli::run(argv, &mut out, &mut err);
    (code, String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned())
}

#[test]
fn profiles_and_usage() {
    let (code, out, _) = run(&["profiles"]);
    assert_eq!(code, 0);
    assert!(out.contains("vanilla_terrain") && out.contains("dh_terrain") && out.contains("fullscreen"), "{out}");
    let (code, out, _) = run(&["profiles", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v.as_array().unwrap().len() >= 10);
    let (code, _, err) = run(&["frobnicate"]);
    assert_eq!(code, 2);
    assert!(!err.is_empty());
    let (code, _, err) = run(&["inspect", "/definitely/not/a/pack"]);
    assert_eq!(code, 1);
    assert!(err.contains("cannot open pack"), "{err}");
}

#[test]
fn inspect_options_compile_validate() {
    let tmp = tempfile::tempdir().unwrap();
    let good = tmp.path().join("good");
    write_pack(&good, false);
    let p = good.to_str().unwrap();

    let (code, out, _) = run(&["inspect", p]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("gbuffers_terrain") && out.contains("composite") && out.contains("DH: synthesized"), "{out}");

    let (code, out, _) = run(&["options", p]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("BLOOM = true") && out.contains("STRENGTH = 2 [1 2 3]") && out.contains("profile.LOW"), "{out}");

    let dir = tmp.path().join("out");
    let (code, out, err) = run(&["compile", p, "-o", dir.to_str().unwrap(), "--set", "STRENGTH=3", "--validate"]);
    assert_eq!(code, 0, "{out}{err}");
    for f in ["pack.json", "blobs.bin", "diagnostics.txt", "composite.fsh.spv", "composite.fsh.vk.glsl", "composite.fsh.rp.glsl"] {
        assert!(dir.join(f).is_file(), "{f} missing");
    }
    let glsl = std::fs::read_to_string(dir.join("composite.fsh.vk.glsl")).unwrap();
    assert!(glsl.contains("3"), "option value applied: {glsl}");
    let json = std::fs::read_to_string(dir.join("pack.json")).unwrap();
    let model = sb_core::model::CompiledPack::from_json(&json).unwrap();
    let bin = std::fs::read(dir.join("blobs.bin")).unwrap();
    let blobs = sb_core::model::BlobTable::from_concat(&model.blobs, &bin).unwrap();
    let first = &model.dimensions[0].programs[0].stages[0];
    assert_eq!(blobs.get_spirv(first.spirv.unwrap()).unwrap()[0], 0x0723_0203);
    assert_eq!(model.options.options.iter().find(|o| o.name == "STRENGTH").map(|o| o.value.as_str()), Some("3"));

    // Validate: a directory of packs, one broken.
    let broken = tmp.path().join("broken");
    write_pack(&broken, true);
    let (code, out, _) = run(&["validate", tmp.path().to_str().unwrap(), "--no-spirv-val"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("good") && out.contains("PASS"), "{out}");
    assert!(out.contains("broken") && out.contains("FAIL"), "{out}");
    let (code, out, _) = run(&["validate", p, "--json"]);
    assert_eq!(code, 0, "{out}");
    let reports: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(reports[0]["programs_failed"].as_array().unwrap().len(), 0);
    assert!(reports[0]["programs_ok"].as_u64().unwrap() >= 3);
}

#[test]
fn find_packs_skips_non_packs() {
    let tmp = tempfile::tempdir().unwrap();
    write_pack(&tmp.path().join("a/b"), false);
    std::fs::create_dir_all(tmp.path().join("docs")).unwrap();
    std::fs::write(tmp.path().join("docs/shaders.properties"), "x=1").unwrap();
    let found = sb_cli::find_packs(tmp.path());
    assert_eq!(found, vec![tmp.path().join("a/b")]);
    // An explicit shaders root is accepted.
    assert_eq!(sb_cli::find_packs(&tmp.path().join("a/b/shaders")).len(), 1);
}

#[test]
fn render_smoke() {
    // Needs a Vulkan device (lavapipe in CI); skipped otherwise.
    if sb_runtime::Runtime::new(&sb_runtime::RuntimeOptions { validation: false, prefer_cpu_device: true, device_name_filter: None }).is_err() {
        eprintln!("no Vulkan device; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("pack");
    write_pack(&pack, false);
    let png = tmp.path().join("r.png");
    let (code, out, err) = run(&["render", pack.to_str().unwrap(), "-o", png.to_str().unwrap(), "--width", "64", "--height", "36", "--frames", "1", "--cpu", "--no-validation"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(png.is_file());
    assert!(out.contains("rendered 1 frames"), "{out}");
}
