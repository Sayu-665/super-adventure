//! The `sodium_terrain` profile end to end: the test pack's terrain, water and shadow
//! programs are rebuilt for Sodium's compact vertex format with the decoder code, inputs,
//! push constants, host block and semantics read from
//! `crates/sb-transform/profiles/sodium_terrain.toml`, and must render the same image as
//! the `vanilla_terrain` programs (the runtime encodes the same world in both formats).
//! This cross-checks the runtime's Sodium encoder against the profile's decoder.

mod common;

use common::shaders::{FRAME_DECL, PRELUDE};
use common::{GPU_LOCK, Variant, assert_no_validation_messages, mean_abs_diff, render, render_dir, runtime, small_scene, test_pack};
use sb_compile::{CompileOptions, compile_glsl};
use sb_core::model::*;
use sb_core::ShaderStage;
use std::path::Path;

/// Program indices of the test pack.
const TERRAIN: usize = 0;
const WATER: usize = 4;
const SHADOW: usize = 5;

struct Profile {
    table: toml::Table,
}

impl Profile {
    fn load() -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../sb-transform/profiles/sodium_terrain.toml");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Self { table: text.parse().unwrap_or_else(|e| panic!("{}: {e}", path.display())) }
    }

    fn semantic(&self, key: &str) -> String {
        let s = self.table["semantics"][key].as_str().unwrap_or_else(|| panic!("semantic `{key}` missing"));
        format!("({s})")
    }

    /// Inputs, push constants, host blocks and helper code as sb-transform declares them.
    fn declarations(&self) -> String {
        let mut out = String::new();
        for i in self.table["inputs"].as_array().expect("inputs") {
            out += &format!("layout(location = {}) in {} {};\n", i["location"].as_integer().expect("location"), i["type"].as_str().expect("type"), i["name"].as_str().expect("name"));
        }
        if let Some(p) = self.table.get("push_constants").and_then(|v| v.as_str()) {
            out += &format!("layout(push_constant) uniform sb_hPush {{ {p} }};\n");
        }
        for b in self.table["blocks"].as_array().expect("blocks") {
            out += &format!(
                "layout(std140, set = 0, binding = 7) uniform {} {{ {} }} {};\n",
                b["name"].as_str().expect("block name"),
                b["members"].as_str().expect("members"),
                b["instance"].as_str().expect("instance")
            );
        }
        out += self.table["code"]["vertex"].as_str().expect("vertex code");
        out
    }
}

fn gbuffers_vsh(p: &Profile) -> String {
    let body = format!(
        r#"
layout(location = 0) out vec2 texcoord;
layout(location = 1) out vec4 color;
layout(location = 2) out vec2 lmcoord;
layout(location = 3) out vec3 normal;
layout(location = 4) flat out int blockId;
void main() {{
    vec4 pos = {position};
    mat4 mv = {model_view};
    gl_Position = {projection} * (mv * pos);
    texcoord = {uv0}.xy;
    color = {color};
    lmcoord = ({lightmap}.xy + 8.0) / 256.0;
    normal = mat3(mv) * {normal};
    blockId = int({entity}.x);
    SB_DEPTH_EPILOGUE
}}
"#,
        position = p.semantic("position"),
        model_view = p.semantic("model_view"),
        projection = p.semantic("projection"),
        uv0 = p.semantic("uv0"),
        color = p.semantic("color"),
        lightmap = p.semantic("lightmap"),
        normal = p.semantic("normal"),
        entity = p.semantic("entity"),
    );
    format!("#version 460\n{PRELUDE}{FRAME_DECL}{}\n{body}", p.declarations())
}

/// The shadow pass: world-space profiles use `shadowModelView` / `shadowProjection`.
fn shadow_vsh(p: &Profile) -> String {
    let body = format!(
        r#"
layout(location = 0) out vec2 texcoord;
layout(location = 1) out vec4 color;
void main() {{
    gl_Position = shadowProjection * (shadowModelView * {position});
    texcoord = {uv0}.xy;
    color = {color};
    SB_DEPTH_EPILOGUE
}}
"#,
        position = p.semantic("position"),
        uv0 = p.semantic("uv0"),
        color = p.semantic("color"),
    );
    format!("#version 460\n{PRELUDE}{FRAME_DECL}{}\n{body}", p.declarations())
}

fn use_sodium(pack: &mut CompiledPack, blobs: &mut BlobTable) {
    let profile = Profile::load();
    let reversed = pack.info.environment.depth_mode == DepthMode::ReversedZeroToOne;
    let dim = &mut pack.dimensions[0];
    dim.bindings.entries.push(BindingEntry { name: "u_Globals".into(), set: 0, binding: 7, kind: ResourceKind::UniformBuffer, resource: ResourceRef::UniformBlock("u_Globals".into()) });
    for (index, src) in [(TERRAIN, gbuffers_vsh(&profile)), (WATER, gbuffers_vsh(&profile)), (SHADOW, shadow_vsh(&profile))] {
        // The depth-mode define of the test pack, right after `#version`.
        let src = if reversed { src.replacen("#version 460\n", "#version 460\n#define SB_REVERSED\n", 1) } else { src };
        let words = compile_glsl(&src, ShaderStage::Vertex, "sodium", &CompileOptions::default(), None).unwrap_or_else(|e| panic!("{e}\n{}\n{src}", e.log));
        let id = blobs.push_spirv(&words);
        let p = &mut dim.programs[index];
        p.draw_profile = Some("sodium_terrain".into());
        for s in &mut p.stages {
            if s.stage == ShaderStage::Vertex {
                s.spirv = Some(id);
            }
        }
    }
}

#[test]
fn sodium_terrain_renders_like_vanilla_terrain() {
    let _g = GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut rt) = runtime() else { return };
    for depth_mode in [DepthMode::ForwardZeroToOne, DepthMode::ReversedZeroToOne] {
        let (vanilla, vblobs) = test_pack(Variant { depth_mode, ..Default::default() });
        let (mut sodium, mut sblobs) = (vanilla.clone(), vblobs.clone());
        use_sodium(&mut sodium, &mut sblobs);
        let scene = small_scene();
        let a = render(&mut rt, &vanilla, &vblobs, scene.clone(), (256, 144), 1, true);
        let b = render(&mut rt, &sodium, &sblobs, scene, (256, 144), 1, true);
        let tag = if depth_mode == DepthMode::ReversedZeroToOne { "_reversed" } else { "" };
        a.image.save(render_dir().join(format!("sodium_ref_vanilla{tag}.png"))).ok();
        b.image.save(render_dir().join(format!("sodium_terrain{tag}.png"))).ok();
        assert_no_validation_messages(&a);
        assert_no_validation_messages(&b);
        assert!(b.stats.programs_skipped.is_empty(), "{:?}", b.stats.programs_skipped);
        assert!(!b.stats.warnings.iter().any(|w| w.contains("vertex layout") || w.contains("reads zero")), "{:#?}", b.stats.warnings);
        // Same number of quads, drawn per region instead of per section.
        assert!(b.stats.draws > 0 && b.stats.draws < a.stats.draws, "{} vs {}", b.stats.draws, a.stats.draws);
        for (name, img) in &b.targets {
            let reference = &a.targets.iter().find(|(n, _)| n == name).expect("same targets").1;
            let d = mean_abs_diff(reference, img);
            if d >= 0.5 / 255.0 {
                save_diff(reference, img, &format!("sodium_diff_{name}{tag}.png"));
            }
            assert!(d < 0.5 / 255.0, "{name}{tag}: mean difference {d} between Sodium and vanilla terrain");
        }
        let d = mean_abs_diff(&a.image, &b.image);
        assert!(d < 0.5 / 255.0, "final image{tag}: mean difference {d}");
    }
}

/// Save `|a - b| * 8` (debugging aid for failures).
fn save_diff(a: &image::RgbaImage, b: &image::RgbaImage, file: &str) {
    let mut d = a.clone();
    for (p, q) in d.pixels_mut().zip(b.pixels()) {
        for c in 0..3 {
            p[c] = p[c].abs_diff(q[c]).saturating_mul(8);
        }
        p[3] = 255;
    }
    d.save(render_dir().join(file)).ok();
}
