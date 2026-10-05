//! Tests of the [`api`](crate::api) layer: small generated packs, the handle registry
//! under concurrency, the disk cache, variants, profiles, evaluators and (when present)
//! the ComplementaryReimagined pack of the small corpus (`$SB_CORPUS_DIRS`, colon-separated,
//! or the default test-data corpus).

use crate::api::{self, PackKind};
use crate::error::Error;
use sb_core::model::{BlobInfo, BlobKind, CompileEnvironment, CompiledPack, OptionsModel, UniformSource};
use sb_expr::Value;
use std::path::{Path, PathBuf};

const SPIRV_MAGIC: u32 = 0x0723_0203;

/// Registered draw profiles are process-wide and part of the compile cache key: the
/// test that registers one and the cache test must not interleave.
static PROFILE_REGISTRY: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn profile_registry() -> std::sync::MutexGuard<'static, ()> {
    PROFILE_REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}
const SCRATCH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test-data");

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}

const FINAL_VSH: &str = "#version 120\nvarying vec2 texcoord;\nvoid main() {\n  gl_Position = ftransform();\n  texcoord = gl_MultiTexCoord0.xy;\n}\n";
const FINAL_FSH: &str = "#version 120\n#define BRIGHTNESS 1.0 // [0.5 1.0 1.5]\n//#define FLAG\nuniform sampler2D colortex0;\nvarying vec2 texcoord;\nvoid main() {\n  vec4 c = texture2D(colortex0, texcoord) * BRIGHTNESS;\n#ifdef FLAG\n  c.r = 1.0;\n#endif\n  gl_FragColor = c;\n}\n";

/// A pack directory `<tmp>/<name>/shaders/...` with a final pass (and `extra` files).
fn tiny_pack(tmp: &Path, name: &str, extra: &[(&str, &str)]) -> PathBuf {
    let root = tmp.join(name);
    write(&root.join("shaders"), &[("final.vsh", FINAL_VSH), ("final.fsh", FINAL_FSH)]);
    write(&root.join("shaders"), extra);
    root
}

fn env_json() -> String {
    let env = CompileEnvironment { distant_horizons: false, ..CompileEnvironment::default() };
    serde_json::to_string(&env).unwrap()
}

/// Every blob of `infos` lies in `buffer`, and SPIR-V blobs start with the magic number.
fn check_blobs(infos: &[BlobInfo], buffer: &[u8]) {
    for (i, b) in infos.iter().enumerate() {
        let (start, end) = (b.offset as usize, (b.offset + b.len) as usize);
        assert!(end <= buffer.len(), "blob {i} ({start}..{end}) outside the {}-byte buffer", buffer.len());
        assert_eq!(start % 8, 0, "blob {i} is not 8-byte aligned");
        if b.kind == BlobKind::Spirv {
            let magic = u32::from_le_bytes(buffer[start..start + 4].try_into().unwrap());
            assert_eq!(magic, SPIRV_MAGIC, "blob {i}");
            assert_eq!(b.len % 4, 0);
        }
    }
}

/// Compile and return the parsed model and the blob buffer.
fn compile(session: u64, values: &str, settings: &str) -> (CompiledPack, Vec<u8>) {
    let json = api::compile(session, &env_json(), values, settings).unwrap();
    let model = CompiledPack::from_json(&json).unwrap();
    let size = api::blob_size(session).unwrap();
    let buffer = api::with_blob_data(session, |b| b.to_vec()).unwrap();
    assert_eq!(size as usize, buffer.len());
    check_blobs(&model.blobs, &buffer);
    (model, buffer)
}

#[test]
fn version_matches_the_workspace() {
    assert_eq!(api::version(), env!("CARGO_PKG_VERSION"));
    assert_eq!(api::version(), sb_core::SHADERBRIDGE_VERSION);
}

#[test]
fn unknown_handles_are_errors_never_panics() {
    for h in [0, 1 << 40, u64::MAX] {
        let unknown = |e: Error| assert!(matches!(e, Error::UnknownHandle { .. }), "{e}");
        unknown(api::close_pack(h).unwrap_err());
        unknown(api::get_options(h, "en_us").unwrap_err());
        unknown(api::compile(h, &env_json(), "", "").unwrap_err());
        unknown(api::blob_size(h).unwrap_err());
        unknown(api::with_blob_data(h, |_| ()).unwrap_err());
        unknown(api::compile_variant(h, "", "gbuffers_terrain", "vanilla_terrain").unwrap_err());
        unknown(api::variant_blob_size(h).unwrap_err());
        unknown(api::with_variant_blob_data(h, |_| ()).unwrap_err());
        unknown(api::create_uniform_evaluator(h, "world0").unwrap_err());
        unknown(api::evaluate_uniforms(h, &mut [0u8; 64], 0.016).unwrap_err());
        unknown(api::destroy_uniform_evaluator(h).unwrap_err());
        unknown(api::normalize_option_values(h, "A=1").unwrap_err());
    }
}

#[test]
fn list_packs_reports_directories_and_zips() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    tiny_pack(dir, "Good Pack", &[]);
    write(dir, &[("NoShaders/readme.txt", "hi"), ("Empty/shaders/lib/common.glsl", "float x;"), (".hidden/shaders/final.fsh", "void main(){}")]);
    write(dir, &[("notes.txt", "not a pack"), ("broken.zip", "this is not a zip archive")]);
    {
        use std::io::Write;
        let file = std::fs::File::create(dir.join("Zipped.ZIP")).unwrap();
        let mut w = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("Zipped/shaders/final.fsh", opts).unwrap();
        w.write_all(FINAL_FSH.as_bytes()).unwrap();
        w.finish().unwrap();
    }
    let packs = api::list_packs(dir).unwrap();
    let names: Vec<&str> = packs.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["broken.zip", "Empty", "Good Pack", "NoShaders", "Zipped.ZIP"]);
    let by = |n: &str| packs.iter().find(|p| p.name == n).unwrap();
    assert!(by("Good Pack").valid && by("Good Pack").error.is_none() && by("Good Pack").kind == PackKind::Dir);
    assert!(by("Zipped.ZIP").valid && by("Zipped.ZIP").kind == PackKind::Zip, "{:?}", by("Zipped.ZIP"));
    assert!(!by("broken.zip").valid && by("broken.zip").error.as_deref().unwrap().contains("zip"));
    assert!(by("NoShaders").error.as_deref().unwrap().contains("not a shader pack"));
    assert_eq!(by("Empty").error.as_deref(), Some("the pack has no shader programs"));
    for p in &packs {
        assert!(Path::new(&p.path).is_absolute(), "{}", p.path);
        assert_eq!(Path::new(&p.path).file_name().unwrap().to_string_lossy(), p.name);
    }

    // The JSON shape of the contract.
    let json: serde_json::Value = serde_json::from_str(&api::list_packs_json(dir).unwrap()).unwrap();
    let good = json.as_array().unwrap().iter().find(|v| v["name"] == "Good Pack").unwrap();
    assert_eq!(good["kind"], "dir");
    assert_eq!(good["valid"], true);
    assert!(good["error"].is_null());
    let mut keys: Vec<&str> = good.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["error", "kind", "name", "path", "valid"]);
    assert_eq!(json.as_array().unwrap().iter().find(|v| v["name"] == "Zipped.ZIP").unwrap()["kind"], "zip");

    assert!(matches!(api::list_packs(&dir.join("missing")).unwrap_err(), Error::Io { .. }));
}

#[test]
fn open_compile_options_blobs_and_close() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tiny_pack(tmp.path(), "Tiny", &[]);
    assert!(matches!(api::open_pack(&tmp.path().join("nope")).unwrap_err(), Error::Pack(_)));
    let s = api::open_pack(&pack).unwrap();
    assert!(s > 0);
    // Zips and directories both work; `shaders/` itself is accepted as well.
    let s2 = api::open_pack(&pack.join("shaders")).unwrap();
    assert_ne!(s, s2);
    api::close_pack(s2).unwrap();

    // Nothing compiled yet.
    assert!(matches!(api::blob_size(s).unwrap_err(), Error::NotCompiled));
    assert!(matches!(api::create_uniform_evaluator(s, "").unwrap_err(), Error::NotCompiled));
    assert!(matches!(api::compile_variant(s, "", "gbuffers_basic", "vanilla_position").unwrap_err(), Error::NotCompiled));

    // Options before any compile: default values.
    let options: OptionsModel = serde_json::from_str(&api::get_options(s, "en_us").unwrap()).unwrap();
    let brightness = options.options.iter().find(|o| o.name == "BRIGHTNESS").unwrap();
    assert_eq!((brightness.default.as_str(), brightness.value.as_str()), ("1.0", "1.0"));
    assert_eq!(brightness.allowed, ["0.5", "1.0", "1.5"]);
    for bad in ["", "../../etc/passwd", "en us", "a/b"] {
        assert!(matches!(api::get_options(s, bad).unwrap_err(), Error::InvalidArgument(_)), "{bad:?}");
    }

    // Normalization: unknown options, invalid booleans and defaults are dropped.
    let normalized = api::normalize_option_values(s, "BRIGHTNESS=1.5\nUNKNOWN=1\nFLAG=maybe\n# comment\n").unwrap();
    assert_eq!(normalized.trim(), "BRIGHTNESS=1.5");
    assert_eq!(api::normalize_option_values(s, "FLAG=true\nBRIGHTNESS=1.0").unwrap().trim(), "FLAG=true");
    assert_eq!(api::normalize_option_values(s, "").unwrap().trim(), "");

    // Bad arguments leave the session untouched.
    assert!(matches!(api::compile(s, "{", "", "").unwrap_err(), Error::InvalidArgument(m) if m.starts_with("envJson")));
    assert!(matches!(api::compile(s, "", "", "").unwrap_err(), Error::InvalidArgument(_)));
    assert!(matches!(api::compile(s, &env_json(), "", "[1]").unwrap_err(), Error::InvalidArgument(m) if m.starts_with("settingsJson")));
    assert!(matches!(api::compile(s, &env_json(), "", r#"{"threads": -1}"#).unwrap_err(), Error::InvalidArgument(_)));

    let (model, buffer) = compile(s, "BRIGHTNESS=1.5\n", r#"{"dimensions": null, "validate": true, "cacheDir": null}"#);
    assert_eq!(model.info.name, "Tiny");
    assert_eq!(model.format_version, sb_core::MODEL_FORMAT_VERSION);
    assert!(!model.diagnostics.has_errors(), "{:?}", model.diagnostics);
    let program = model.dimensions.iter().flat_map(|d| &d.programs).find(|p| p.name.ends_with("final")).unwrap();
    let fragment = program.stages.iter().find(|m| m.stage == sb_core::ShaderStage::Fragment).unwrap();
    let glsl = &model.blobs[fragment.glsl_vulkan.unwrap().0 as usize];
    let text = std::str::from_utf8(&buffer[glsl.offset as usize..(glsl.offset + glsl.len) as usize]).unwrap();
    assert!(text.contains("#version") && text.contains("1.5"), "{text}");
    for stage in &program.stages {
        assert!(stage.spirv.is_some() && stage.glsl_renderpearl.is_some(), "{stage:?}");
    }

    // The options now reflect the compiled values.
    let options: OptionsModel = serde_json::from_str(&api::get_options(s, "en_us").unwrap()).unwrap();
    assert_eq!(options.options.iter().find(|o| o.name == "BRIGHTNESS").unwrap().value, "1.5");

    // A failed compile request (bad JSON) keeps the last result.
    assert!(api::compile(s, "null", "", "").is_err());
    assert_eq!(api::blob_size(s).unwrap() as usize, buffer.len());

    // `dimensions`: unknown folders are a warning; `threads` uses a sized pool.
    let json = api::compile(s, &env_json(), "", r#"{"dimensions": ["world7"], "threads": 2, "unknownKey": 1}"#).unwrap();
    let model = CompiledPack::from_json(&json).unwrap();
    assert!(model.dimensions.is_empty());
    assert!(model.diagnostics.iter().any(|d| d.code == "pipeline.unknown-dimension"));
    assert_eq!(api::blob_size(s).unwrap(), 0);

    api::close_pack(s).unwrap();
    assert!(matches!(api::close_pack(s).unwrap_err(), Error::UnknownHandle { .. }));
    assert!(matches!(api::blob_size(s).unwrap_err(), Error::UnknownHandle { .. }));
}

#[test]
fn custom_uniform_evaluator() {
    let tmp = tempfile::tempdir().unwrap();
    let props = "uniform.float.doubledTime = frameTimeCounter * 2.0\nvariable.float.v = 1.0\nuniform.vec2.pair = vec2(v, frameTimeCounter)\nuniform.int.frameParity = frameCounter % 2\n";
    let fsh = "#version 120\nuniform float doubledTime;\nuniform vec2 pair;\nuniform int frameParity;\nvoid main() { gl_FragColor = vec4(doubledTime, pair, float(frameParity)); }\n";
    let pack = tiny_pack(tmp.path(), "Custom", &[("shaders.properties", props), ("composite.fsh", fsh)]);
    let s = api::open_pack(&pack).unwrap();
    let (model, _) = compile(s, "", "");
    let dim = model.dimensions.iter().find(|d| d.folder.is_empty()).expect("root folder");
    let frame = &dim.uniforms.frame;
    let member = |name: &str| frame.members.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("no member {name}: {frame:?}"));
    assert_eq!(member("doubledTime").source, UniformSource::Custom("doubledTime".into()));
    assert_eq!(member("frameTimeCounter").source, UniformSource::Builtin("frameTimeCounter".into()));

    let ev = api::create_uniform_evaluator(s, "").unwrap();
    let mut block = vec![0u8; frame.size as usize];
    let ftc = member("frameTimeCounter");
    sb_expr::write_value(ftc.ty, Value::Float(1.25), &mut block[ftc.offset as usize..]).unwrap();
    let fc = member("frameCounter");
    sb_expr::write_value(fc.ty, Value::Int(7), &mut block[fc.offset as usize..]).unwrap();
    api::evaluate_uniforms(ev, &mut block, 1.0 / 60.0).unwrap();
    let read = |name: &str| {
        let m = member(name);
        sb_expr::read_value(m.ty, &block[m.offset as usize..]).unwrap()
    };
    assert_eq!(read("doubledTime"), Value::Float(2.5));
    assert_eq!(read("pair"), Value::Vec2([1.0, 1.25]));
    assert_eq!(read("frameParity"), Value::Int(1));
    // A larger buffer is fine; a smaller one is rejected without writing.
    let mut bigger = block.clone();
    bigger.extend_from_slice(&[0xAB; 32]);
    api::evaluate_uniforms(ev, &mut bigger, f32::NAN).unwrap();
    assert!(bigger[block.len()..].iter().all(|&b| b == 0xAB));
    let mut small = vec![0u8; frame.size as usize - 1];
    assert!(matches!(api::evaluate_uniforms(ev, &mut small, 0.0).unwrap_err(), Error::InvalidArgument(_)));
    assert!(small.iter().all(|&b| b == 0));

    // Unknown folder; the evaluator outlives its session; double destroy.
    assert!(matches!(api::create_uniform_evaluator(s, "world0").unwrap_err(), Error::Unavailable(_)));
    api::close_pack(s).unwrap();
    api::evaluate_uniforms(ev, &mut block, 0.0).unwrap();
    // A session handle is not an evaluator handle and vice versa.
    assert!(api::blob_size(ev).is_err());
    api::destroy_uniform_evaluator(ev).unwrap();
    assert!(api::destroy_uniform_evaluator(ev).is_err());
    assert!(api::evaluate_uniforms(ev, &mut block, 0.0).is_err());

    // A pack without custom uniforms has nothing to evaluate.
    let plain = api::open_pack(&tiny_pack(tmp.path(), "Plain", &[])).unwrap();
    compile(plain, "", "");
    let e = api::create_uniform_evaluator(plain, "").unwrap_err();
    assert!(matches!(&e, Error::Unavailable(m) if m.contains("no custom uniforms")), "{e}");
    api::close_pack(plain).unwrap();
}

const TEXTURED_VSH: &str = "#version 120\nvarying vec2 uv;\nvarying vec4 color;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.xy; color = gl_Color; }\n";
const TEXTURED_FSH: &str = "#version 120\nuniform sampler2D texture;\nvarying vec2 uv;\nvarying vec4 color;\nvoid main() { gl_FragData[0] = texture2D(texture, uv) * color; }\n";

const TEST_PROFILE: &str = r#"
name = "sbjni_test_profile"
description = "sb-jni unit test profile"

[[inputs]]
name = "sbjniPosition"
type = "vec3"
location = 0

[[inputs]]
name = "sbjniColor"
type = "vec4"
location = 1

[semantics]
position = "vec4(sbjniPosition, 1.0)"
color = "sbjniColor"
uv0 = "vec4(0.0, 0.0, 0.0, 1.0)"
model_view = "gbufferModelView"
projection = "gbufferProjection"
"#;

#[test]
fn variants_and_registered_profiles() {
    let _registry = profile_registry();
    let tmp = tempfile::tempdir().unwrap();
    let pack = tiny_pack(tmp.path(), "Variants", &[("gbuffers_textured.vsh", TEXTURED_VSH), ("gbuffers_textured.fsh", TEXTURED_FSH)]);
    let s = api::open_pack(&pack).unwrap();
    compile(s, "", "");

    // gbuffers_terrain falls back to gbuffers_textured; both spellings of the name work.
    for name in ["gbuffers_terrain", "terrain"] {
        let json = api::compile_variant(s, "", name, "vanilla_terrain").unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let program: sb_core::model::Program = serde_json::from_value(v["program"].clone()).unwrap();
        let infos: Vec<BlobInfo> = serde_json::from_value(v["blobs"].clone()).unwrap();
        // The variant's own diagnostics travel with it (an array, possibly empty).
        let _: Vec<sb_core::Diagnostic> = serde_json::from_value(v["diagnostics"].clone()).unwrap();
        assert_eq!(program.draw_profile.as_deref(), Some("vanilla_terrain"));
        let buffer = api::with_variant_blob_data(s, |b| b.to_vec()).unwrap();
        assert_eq!(api::variant_blob_size(s).unwrap() as usize, buffer.len());
        check_blobs(&infos, &buffer);
        for stage in &program.stages {
            let id = stage.spirv.expect("SPIR-V").0 as usize;
            assert_eq!(infos[id].kind, BlobKind::Spirv);
        }
    }
    assert!(matches!(api::compile_variant(s, "", "gbuffers_nonsense", "vanilla_terrain").unwrap_err(), Error::InvalidArgument(_)));
    let e = api::compile_variant(s, "", "gbuffers_terrain", "no_such_profile").unwrap_err();
    assert!(matches!(&e, Error::Unavailable(m) if m.contains("does not exist")), "{e}");
    assert!(matches!(api::compile_variant(s, "world0", "gbuffers_terrain", "vanilla_terrain").unwrap_err(), Error::Unavailable(_)));
    // A failed variant clears the previous variant's buffer.
    assert!(api::variant_blob_size(s).is_err());
    // No program in the chain: shadow.
    assert!(matches!(api::compile_variant(s, "", "shadow", "vanilla_terrain").unwrap_err(), Error::Unavailable(_)));

    // Profiles: invalid TOML is rejected; a valid one needs a recompile before use.
    assert!(matches!(api::register_profile("name = ").unwrap_err(), Error::InvalidArgument(_)));
    assert!(matches!(api::register_profile("name = \"x\"\n[[inputs]]\nname = \"1bad\"\ntype = \"vec3\"\nlocation = 0\n").unwrap_err(), Error::InvalidArgument(_)));
    assert_eq!(api::register_profile(TEST_PROFILE).unwrap(), "sbjni_test_profile");
    let e = api::compile_variant(s, "", "gbuffers_textured", "sbjni_test_profile").unwrap_err();
    assert!(matches!(&e, Error::Unavailable(m) if m.contains("registered after the last compile")), "{e}");
    compile(s, "", "");
    let json = api::compile_variant(s, "", "gbuffers_textured", "sbjni_test_profile").unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let inputs: Vec<&str> = v["program"]["vertex_inputs"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap()).collect();
    assert!(inputs.contains(&"sbjniPosition"), "{inputs:?}");
    // Re-registering replaces the profile.
    assert_eq!(api::register_profile(TEST_PROFILE).unwrap(), "sbjni_test_profile");
    api::close_pack(s).unwrap();
}

/// Any built-in draw profile works for a variant (the folder layout includes every
/// built-in profile's resources), for vanilla slots and for DH slots synthesized from the
/// pack's gbuffers programs.
#[test]
fn variants_for_every_builtin_profile() {
    let _registry = profile_registry();
    let tmp = tempfile::tempdir().unwrap();
    let pack = tiny_pack(tmp.path(), "AllProfiles", &[("gbuffers_textured.vsh", TEXTURED_VSH), ("gbuffers_textured.fsh", TEXTURED_FSH)]);
    let s = api::open_pack(&pack).unwrap();
    let env = serde_json::to_string(&CompileEnvironment { distant_horizons: true, ..CompileEnvironment::default() }).unwrap();
    let model = CompiledPack::from_json(&api::compile(s, &env, "", "").unwrap()).unwrap();
    assert!(model.dimensions[0].geometry.contains_key(&sb_core::program::GeometryProgram::DhTerrain));
    for profile in sb_transform::builtin_profiles() {
        for slot in ["terrain_solid", "gbuffers_entities", "shadow_solid", "dh_terrain"] {
            if slot == "shadow_solid" {
                // No shadow program in this pack.
                assert!(api::compile_variant(s, "", slot, &profile.name).is_err());
                continue;
            }
            let json = api::compile_variant(s, "", slot, &profile.name).unwrap_or_else(|e| panic!("{} for {slot}: {e}", profile.name));
            let v: serde_json::Value = serde_json::from_str(&json).unwrap();
            let program: sb_core::model::Program = serde_json::from_value(v["program"].clone()).unwrap();
            assert_eq!(program.draw_profile.as_deref(), Some(profile.name.as_str()));
            let expected = if slot == "dh_terrain" { "dh_terrain" } else { "gbuffers_textured" };
            assert_eq!(program.name, expected, "{} for {slot}", profile.name);
            let infos: Vec<BlobInfo> = serde_json::from_value(v["blobs"].clone()).unwrap();
            check_blobs(&infos, &api::with_variant_blob_data(s, |b| b.to_vec()).unwrap());
        }
    }
    api::close_pack(s).unwrap();
}

#[test]
fn disk_cache_round_trip() {
    let _registry = profile_registry();
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let pack = tiny_pack(tmp.path(), "Cached", &[("gbuffers_textured.vsh", TEXTURED_VSH), ("gbuffers_textured.fsh", TEXTURED_FSH)]);
    let settings = serde_json::json!({ "dimensions": null, "validate": false, "cacheDir": cache }).to_string();
    let s = api::open_pack(&pack).unwrap();
    let (first, first_buffer) = compile(s, "BRIGHTNESS=0.5", &settings);
    let mut entries: Vec<String> =
        std::fs::read_dir(&cache).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    entries.sort();
    assert_eq!(entries.len(), 2, "{entries:?}");
    let key = entries[0].strip_suffix(".bin").expect("<key>.bin").to_string();
    assert_eq!(entries[1], format!("{key}.json"));
    assert_eq!(std::fs::read(cache.join(format!("{key}.bin"))).unwrap(), first_buffer);

    // Mark the cached model to prove the next compile reads it.
    let path = cache.join(format!("{key}.json"));
    let mut cached = CompiledPack::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(cached.blobs, first.blobs);
    cached.info.name = "from-cache".into();
    std::fs::write(&path, serde_json::to_string(&cached).unwrap()).unwrap();
    let s2 = api::open_pack(&pack).unwrap();
    let (second, second_buffer) = compile(s2, "BRIGHTNESS=0.5", &settings);
    assert_eq!(second.info.name, "from-cache");
    assert_eq!(second.dimensions, first.dimensions);
    assert_eq!(second_buffer, first_buffer);
    // Variants after a cache hit compile the session on demand.
    api::compile_variant(s2, "", "gbuffers_terrain", "vanilla_terrain").unwrap();

    // Other option values miss the cache.
    let (third, _) = compile(s2, "BRIGHTNESS=1.5", &settings);
    assert_eq!(third.info.name, "Cached");
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 4);

    // An unwritable cache directory is a warning, not a failure.
    let file = tmp.path().join("not-a-dir");
    std::fs::write(&file, "x").unwrap();
    let bad = serde_json::json!({ "cacheDir": file.join("sub") }).to_string();
    let (model, _) = compile(s2, "BRIGHTNESS=1.0", &bad);
    assert!(model.diagnostics.iter().any(|d| d.code == "jni.cache"), "{:?}", model.diagnostics);
    api::close_pack(s).unwrap();
    api::close_pack(s2).unwrap();
}

#[test]
fn concurrent_calls_and_closing_are_safe() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tiny_pack(tmp.path(), "Concurrent", &[]);
    let s = api::open_pack(&pack).unwrap();
    std::thread::scope(|scope| {
        for i in 0..4 {
            scope.spawn(move || {
                for _ in 0..3 {
                    let r = match i % 3 {
                        0 => api::compile(s, &env_json(), "", "").map(drop),
                        1 => api::get_options(s, "en_us").map(drop),
                        _ => api::normalize_option_values(s, "BRIGHTNESS=1.5").map(drop),
                    };
                    if let Err(e) = r {
                        assert!(matches!(e, Error::UnknownHandle { .. }), "{e}");
                    }
                }
            });
        }
        scope.spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            api::close_pack(s).unwrap();
        });
    });
    assert!(matches!(api::blob_size(s).unwrap_err(), Error::UnknownHandle { .. }));
}

/// The small corpus's ComplementaryReimagined, if present.
fn complementary() -> Option<PathBuf> {
    let roots: Vec<PathBuf> = match std::env::var_os("SB_CORPUS_DIRS") {
        Some(v) => std::env::split_paths(&v).collect(),
        None => vec![Path::new(SCRATCH).join("corpus")],
    };
    roots.into_iter().map(|r| r.join("ComplementaryReimagined")).find(|p| p.join("shaders").is_dir())
}

#[test]
fn complementary_reimagined_end_to_end() {
    let Some(pack) = complementary() else {
        eprintln!("skipped: ComplementaryReimagined not found (set SB_CORPUS_DIRS)");
        return;
    };
    let corpus = pack.parent().unwrap();
    let listed = api::list_packs(corpus).unwrap();
    let entry = listed.iter().find(|p| p.name == "ComplementaryReimagined").unwrap();
    assert!(entry.valid, "{entry:?}");
    if let Some(tutorials) = listed.iter().find(|p| p.name == "MinecraftShaderProgramming") {
        assert!(!tutorials.valid);
    }

    let s = api::open_pack(&pack).unwrap();
    let options: OptionsModel = serde_json::from_str(&api::get_options(s, "en_us").unwrap()).unwrap();
    assert!(options.options.len() > 50, "{} options", options.options.len());
    assert!(!options.screens.is_empty() && !options.lang.is_empty());
    let boolean = options.options.iter().find(|o| o.allowed.is_empty() && o.kind == sb_core::model::OptionKind::BooleanDefine).unwrap();
    let flipped = if boolean.default == "true" { "false" } else { "true" };
    let values = format!("{}={flipped}\nNOT_AN_OPTION=3\n", boolean.name);
    assert_eq!(api::normalize_option_values(s, &values).unwrap().trim(), format!("{}={flipped}", boolean.name));

    let t = std::time::Instant::now();
    let (model, buffer) = compile(s, "", r#"{"dimensions": null, "validate": false, "cacheDir": null}"#);
    eprintln!("ComplementaryReimagined: {:.1} s, {} blobs, {} bytes", t.elapsed().as_secs_f64(), model.blobs.len(), buffer.len());
    let programs: Vec<_> = model.dimensions.iter().flat_map(|d| &d.programs).collect();
    assert!(programs.len() > 50, "{} programs", programs.len());
    for p in &programs {
        for stage in &p.stages {
            let id = stage.spirv.unwrap_or_else(|| panic!("{} has no SPIR-V", p.name)).0 as usize;
            assert_eq!(model.blobs[id].kind, BlobKind::Spirv);
        }
    }
    let errors: Vec<String> = model.diagnostics.errors().map(|d| d.to_string()).collect();
    eprintln!("{} error diagnostics", errors.len());

    // Custom uniforms (Complementary defines framemod2 = frameCounter % 2, ...).
    let world0 = model.dimensions.iter().find(|d| d.folder == "world0").unwrap();
    let ev = api::create_uniform_evaluator(s, "world0").unwrap();
    let frame = &world0.uniforms.frame;
    let mut block = vec![0u8; frame.size as usize];
    let fc = frame.member("frameCounter").unwrap();
    sb_expr::write_value(fc.ty, Value::Int(5), &mut block[fc.offset as usize..]).unwrap();
    api::evaluate_uniforms(ev, &mut block, 1.0 / 60.0).unwrap();
    let fm = frame.member("framemod2").unwrap();
    assert_eq!(sb_expr::read_value(fm.ty, &block[fm.offset as usize..]).unwrap(), Value::Float(1.0));
    api::destroy_uniform_evaluator(ev).unwrap();

    let json = api::compile_variant(s, "world0", "gbuffers_terrain", "vanilla_terrain").unwrap();
    assert!(json.starts_with("{\"program\":"));
    api::close_pack(s).unwrap();
}
