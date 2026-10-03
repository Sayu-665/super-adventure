//! Spec §3.4 / ARCHITECTURE §8: cross-stage interface linking — locations, slot sizes,
//! `flat`, missing and never-written outputs (Iris rules a and b), type mismatches
//! (rule c), arrays, interface blocks, geometry and tessellation stages, fixed-function
//! varyings, and storage-qualifier conversion.

use std::collections::BTreeMap;

use sb_compile::Reflection;
use sb_core::ShaderStage;

use crate::harness::*;

/// name -> (location, location count, flat) of the reflected inputs or outputs.
/// Members of interface blocks are keyed `<Block>.<member>` (instance names differ
/// between stages).
fn iface(r: &Reflection, outputs: bool) -> BTreeMap<String, (u32, u32, bool)> {
    let v = if outputs { &r.outputs } else { &r.inputs };
    v.iter()
        .map(|x| {
            // Generated names: `sb_vin_X` (per-vertex inputs) pairs with `sb_v_X`, and the
            // forwarded profile global `X` (fragment input) and `sb_varyin_X` (per-vertex
            // input) with the output `sb_vary_X`.
            let name = x.name.replace("sb_varyin_", "").replace("sb_vin_", "sb_v_").replace("sb_vary_", "");
            let key = match name.split_once('.') {
                Some((_, member)) => format!("<block>.{member}"),
                None => name,
            };
            (key, (x.location, x.location_count, x.flat))
        })
        .collect()
}

/// Every input of `consumer` has an output of `producer` with the same name and location.
#[track_caller]
fn linked(out: &Out, producer: ShaderStage, consumer: ShaderStage) {
    let outs = iface(out.refl(producer), true);
    for (name, (loc, count, flat)) in iface(out.refl(consumer), false) {
        let Some(o) = outs.get(&name) else { panic!("{consumer} input `{name}` has no {producer} output: {outs:?}") };
        assert_eq!((o.0, o.1), (loc, count), "`{name}`");
        if consumer == ShaderStage::Fragment {
            assert_eq!(o.2, flat, "flatness of `{name}`");
        }
    }
}

#[test]
fn legacy_storage_qualifiers_become_in_and_out() {
    let vs = "#version 120\nattribute vec4 mc_Entity;\nvarying vec2 uv;\nvarying float id;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.xy; id = mc_Entity.x; }\n";
    let fs = "#version 120\nvarying vec2 uv;\nvarying float id;\nvoid main() { gl_FragData[0] = vec4(uv, id, 1.0); }\n";
    let out = T::gbuffers().vs(vs).fs(fs).run();
    contains_all(out.vs(), &["out vec2 uv;", "out float id;"]);
    contains_all(out.fs(), &["in vec2 uv;", "in float id;"]);
    contains_none(out.vs(), &["varying", "attribute"]);
    contains_none(out.fs(), &["varying"]);
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
}

#[test]
fn locations_follow_slot_sizes_and_match_by_name() {
    let vs = "#version 410\nout vec2 a;\nout mat3 m;\nout dvec4 d;\nout float arr[3];\nout vec4 b;\n\
              void main() { gl_Position = vec4(0.0, 0.0, 0.0, 1.0); a = vec2(1.0); m = mat3(1.0); d = dvec4(1.0); for (int i = 0; i < 3; i++) arr[i] = 1.0; b = vec4(1.0); }\n";
    // The consumer declares them in another order.
    let fs = "#version 410\nin vec4 b;\nflat in dvec4 d;\nin float arr[3];\nin mat3 m;\nin vec2 a;\nout vec4 c;\n\
              void main() { c = b + vec4(a, arr[2], m[0].x) + vec4(d); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    let ins = iface(out.refl(ShaderStage::Fragment), false);
    assert_eq!(ins["b"], (0, 1, false));
    assert_eq!(ins["d"], (1, 2, true));
    assert_eq!(ins["arr"].0, 3);
    assert_eq!(ins["arr"].1, 3);
    assert_eq!(ins["m"], (6, 3, false));
    assert_eq!(ins["a"], (9, 1, false));
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
}

#[test]
fn integer_varyings_are_flat_on_both_sides() {
    let vs = "#version 130\nout int id;\nflat out vec3 n;\nout uvec2 packed;\nvoid main() { gl_Position = vec4(0.0); id = 1; n = vec3(0.0); packed = uvec2(1u); }\n";
    let fs = "#version 130\nflat in int id;\nin vec3 n;\nflat in uvec2 packed;\nvoid main() { gl_FragData[0] = vec4(float(id), n.x, vec2(packed)); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_all(out.vs(), &["flat out int id;", "flat out uvec2 packed;"]);
    contains_all(out.fs(), &["flat in int id;", "flat in uvec2 packed;"]);
    // A float the producer declares `flat` but the fragment stage reads smoothly is
    // interpolated: the fragment input's qualifier decides (GLSL 4.30+, NVIDIA, Vulkan).
    contains_all(out.vs(), &["out vec3 n;"]);
    contains_all(out.fs(), &["in vec3 n;"]);
    contains_none(out.vs(), &["flat out vec3 n;"]);
    contains_none(out.fs(), &["flat in vec3 n;"]);
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
    // ... and a smooth producer output read `flat` is flat.
    let out = T::fullscreen()
        .vs("#version 130\nout vec3 n;\nvoid main() { gl_Position = vec4(0.0); n = vec3(0.0); }\n")
        .fs("#version 130\nflat in vec3 n;\nvoid main() { gl_FragData[0] = vec4(n, 1.0); }\n")
        .run();
    contains_all(out.vs(), &["flat out vec3 n;"]);
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
    // An integer input without `flat` in the pack (lenient drivers accept it in the VS).
    let vs = "#version 130\nout int id;\nvoid main() { gl_Position = vec4(0.0); id = 1; }\n";
    let fs = "#version 130\nflat in int id;\nvoid main() { gl_FragData[0] = vec4(float(id)); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_all(out.vs(), &["flat out int id;"]);
}

#[test]
fn rule_a_missing_producer_outputs_read_zero() {
    let vs = "#version 120\nvarying vec2 uv;\nvoid main() { gl_Position = ftransform(); uv = vec2(0.5); }\n";
    let fs = "#version 120\nvarying vec2 uv;\nvarying vec3 normal;\nvarying vec4 unusedHere;\nvoid main() { gl_FragData[0] = vec4(uv, normal.xy); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_all(out.vs(), &["out vec3 normal;", "normal = vec3(0);"]);
    // Inputs nobody reads and nobody writes are removed.
    contains_none(out.fs(), &["unusedHere"]);
    assert!(out.has_diag("xf.missing-varying"));
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
}

#[test]
fn rule_b_never_written_outputs_read_zero() {
    let vs = "#version 130\nout vec3 normal;\nout float fog;\nvoid main() { gl_Position = vec4(0.0); fog = 1.0; }\n";
    let fs = "#version 130\nin vec3 normal;\nin float fog;\nvoid main() { gl_FragData[0] = vec4(normal, fog); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    let main = &out.vs()[out.vs().find("void main()").unwrap()..];
    contains_all(main, &["normal = vec3(0);\n    sb_user_main();"]);
    contains_none(main, &["fog = float(0)"]);
    assert!(out.has_diag("xf.unwritten-varying"));
}

#[test]
fn unconsumed_outputs_become_plain_globals() {
    let vs = "#version 130\nout vec4 debug;\nout vec2 uv;\nvoid main() { gl_Position = vec4(0.0); debug = vec4(1.0); uv = vec2(debug.x); }\n";
    let fs = "#version 130\nin vec2 uv;\nvoid main() { gl_FragData[0] = vec4(uv, 0.0, 1.0); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_all(out.vs(), &["\nvec4 debug;", "debug = vec4(1.0);"]);
    contains_none(out.vs(), &["out vec4 debug"]);
    assert!(!iface(out.refl(ShaderStage::Vertex), true).contains_key("debug"));
}

#[test]
fn rule_c_type_mismatches_are_converted_to_the_consumer_type() {
    let vs = "#version 120\nvarying vec4 color;\nvarying float light;\nvarying vec3 pos;\nvoid main() { gl_Position = ftransform(); color = gl_Color; light = 0.5; pos = vec3(1.0); }\n";
    let fs = "#version 120\nvarying vec3 color;\nvarying vec2 light;\nvarying vec3 pos;\nvoid main() { gl_FragData[0] = vec4(color + vec3(light, 0.0) + pos, 1.0); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_all(
        out.vs(),
        &[
            "out vec3 color;",
            "out vec2 light;",
            "vec4 sb_tmp_color = vec4(0);",
            "sb_tmp_color = sb_gl_Color;",
            "color = vec3((sb_tmp_color).xyz);",
            "light = vec2((sb_tmp_light), float(0));",
        ],
    );
    assert!(out.has_diag("xf.varying-type"));
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
    // Each repair is reported once (the link diagnostics used to be reported twice).
    assert_eq!(out.prog.diagnostics.iter().filter(|d| d.code == "xf.varying-type").count(), 2);
}

#[test]
fn rule_c_pads_wider_consumer_vectors_with_zeros() {
    // Iris pads with zeros, `w` included (`vec4(tmp, vec4(0))`); 1 in `w` is only for
    // vertex attributes.
    let vs = "#version 130\nout vec3 normal;\nout vec2 uv;\nflat out int id;\nvoid main() { gl_Position = ftransform(); normal = vec3(0.0, 1.0, 0.0); uv = vec2(0.5); id = 3; }\n";
    let fs = "#version 130\nin vec4 normal;\nin vec4 uv;\nflat in ivec4 id;\nvoid main() { gl_FragData[0] = normal + uv + vec4(id); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_all(
        out.vs(),
        &["normal = vec4((sb_tmp_normal), float(0));", "uv = vec4((sb_tmp_uv), float(0), float(0));", "id = ivec4((sb_tmp_id), int(0), int(0), int(0));"],
    );
    contains_none(out.vs(), &["float(1)", "int(1)"]);
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
}

#[test]
fn rule_c_array_lengths_follow_the_consumer() {
    let vs = "#version 130\nout vec2 taps[4];\nout float w[1];\nvoid main() { gl_Position = vec4(0.0); for (int i = 0; i < 4; i++) taps[i] = vec2(float(i)); w[0] = 1.0; }\n";
    let fs = "#version 130\nin vec2 taps[2];\nin float w[3];\nvoid main() { gl_FragData[0] = vec4(taps[0], taps[1].x, w[2]); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_all(
        out.vs(),
        &["out vec2 taps[2];", "vec2 sb_tmp_taps[4];", "taps[1] = sb_tmp_taps[1];", "out float w[3];", "w[0] = sb_tmp_w[0];", "w[2] = float(0);"],
    );
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
    // Matrices and vectors cannot be converted into each other.
    let fs = "#version 130\nin mat2 taps[2];\nin float w[3];\nvoid main() { gl_FragData[0] = vec4(taps[0][0], w[2], 1.0); }\n";
    let errors = T::fullscreen().vs(vs).fs(fs).errors();
    assert!(errors.iter().any(|e| e == "xf.varying-mismatch"), "{errors:?}");
}

#[test]
fn interface_blocks_link_with_member_locations() {
    let vs = "#version 330\nout VertexData { vec2 uv; flat int id; vec3 n; } vd;\nvoid main() { gl_Position = vec4(0.0); vd.uv = vec2(0.0); vd.id = 1; vd.n = vec3(0.0); }\n";
    let fs = "#version 330\nin VertexData { vec2 uv; flat int id; vec3 n; } vd;\nout vec4 c;\nvoid main() { c = vec4(vd.uv, float(vd.id), vd.n.x); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
    // Member locations are kept (shifted) and the block itself gets none.
    let vs = "#version 330\nout Data { layout(location = 3) vec2 uv; layout(location = 5) vec4 c; } o;\nvoid main() { gl_Position = vec4(0.0); o.uv = vec2(0.0); o.c = vec4(1.0); }\n";
    let fs = "#version 330\nin Data { layout(location = 3) vec2 uv; layout(location = 5) vec4 c; } i;\nout vec4 color;\nvoid main() { color = vec4(i.uv, 0.0, 0.0) + i.c; }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
}

#[test]
fn geometry_stage_arrays_are_linked_per_vertex() {
    let vs = "#version 150\nout vec2 uv;\nflat out int layer;\nvoid main() { gl_Position = vec4(0.0); uv = vec2(1.0); layer = 0; }\n";
    let gs = "#version 150\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\nin vec2 uv[];\nflat in int layer[];\nout vec2 guv;\n\
              void main() { for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; guv = uv[i] * float(layer[i]); EmitVertex(); } EndPrimitive(); }\n";
    let fs = "#version 150\nin vec2 guv;\nout vec4 c;\nvoid main() { c = vec4(guv, 0.0, 1.0); }\n";
    let out = T::fullscreen().vs(vs).gs(gs).fs(fs).run();
    linked(&out, ShaderStage::Vertex, ShaderStage::Geometry);
    linked(&out, ShaderStage::Geometry, ShaderStage::Fragment);
    assert!(out.refl(ShaderStage::Geometry).inputs.iter().all(|i| i.per_vertex));
    assert!(out.prog.requires_raw_vulkan);
    // Array dimensions on the type are inner ones: `vec4[2] w[]` is `vec4 w[][2]`, the
    // per-vertex array of the vertex stage's `vec4 w[2]` (two locations per vertex).
    let vs = "#version 430\nout vec4 w[2];\nvoid main() { gl_Position = vec4(0.0); w[0] = vec4(1.0); w[1] = vec4(2.0); }\n";
    let gs = "#version 430\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\nin vec4[2] w[];\nout vec4 g;\n\
              void main() { for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; g = w[i][1]; EmitVertex(); } EndPrimitive(); }\n";
    let fs = "#version 430\nin vec4 g;\nout vec4 c;\nvoid main() { c = g; }\n";
    let out = T::fullscreen().vs(vs).gs(gs).fs(fs).run();
    linked(&out, ShaderStage::Vertex, ShaderStage::Geometry);
    assert!(!out.has_diag("xf.varying-type") && !out.has_diag("xf.varying-mismatch"));
    let w = out.refl(ShaderStage::Geometry).inputs.iter().find(|i| i.name == "w").unwrap();
    assert_eq!((w.location_count, w.array_len), (2, Some(2)));
}

#[test]
fn geometry_stage_forwards_fixed_function_varyings() {
    let vs = "#version 120\nvoid main() { gl_Position = ftransform(); gl_TexCoord[0] = gl_MultiTexCoord0; gl_FrontColor = gl_Color; }\n";
    let gs = "#version 150 compatibility\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\n\
              void main() { for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; gl_TexCoord[0] = gl_TexCoordIn[i][0]; EmitVertex(); } EndPrimitive(); }\n";
    let fs = "#version 120\nvoid main() { gl_FragData[0] = gl_Color * gl_TexCoord[0]; }\n";
    let out = T::fullscreen().vs(vs).gs(gs).fs(fs).run();
    let g = out.glsl(ShaderStage::Geometry);
    contains_all(g, &["in vec4 sb_vin_TexCoord[][1];", "sb_v_TexCoord[0] = sb_vin_TexCoord[i][0];", "sb_v_Color = sb_vin_Color[0];"]);
    linked(&out, ShaderStage::Vertex, ShaderStage::Geometry);
    linked(&out, ShaderStage::Geometry, ShaderStage::Fragment);
}

#[test]
fn tessellation_stages_link() {
    let vs = "#version 410\nout vec2 vuv;\nvoid main() { gl_Position = vec4(0.0, 0.0, 0.0, 1.0); vuv = vec2(0.5); }\n";
    let tcs = "#version 410\nlayout(vertices = 3) out;\nin vec2 vuv[];\nout vec2 tuv[];\npatch out float level;\n\
               void main() { tuv[gl_InvocationID] = vuv[gl_InvocationID]; gl_out[gl_InvocationID].gl_Position = gl_in[gl_InvocationID].gl_Position; level = 2.0; gl_TessLevelInner[0] = level; gl_TessLevelOuter[0] = level; gl_TessLevelOuter[1] = level; gl_TessLevelOuter[2] = level; }\n";
    let tes = "#version 410\nlayout(triangles, equal_spacing, ccw) in;\nin vec2 tuv[];\npatch in float level;\nout vec2 euv;\n\
               void main() { euv = tuv[0] * gl_TessCoord.x + tuv[1] * gl_TessCoord.y + tuv[2] * gl_TessCoord.z; gl_Position = gl_in[0].gl_Position * level; }\n";
    let fs = "#version 410\nin vec2 euv;\nout vec4 c;\nvoid main() { c = vec4(euv, 0.0, 1.0); }\n";
    let out = T::fullscreen()
        .stage(ShaderStage::Vertex, vs)
        .stage(ShaderStage::TessControl, tcs)
        .stage(ShaderStage::TessEval, tes)
        .stage(ShaderStage::Fragment, fs)
        .run();
    linked(&out, ShaderStage::Vertex, ShaderStage::TessControl);
    linked(&out, ShaderStage::TessControl, ShaderStage::TessEval);
    linked(&out, ShaderStage::TessEval, ShaderStage::Fragment);
    // The depth remap and `invariant gl_Position` go to the last pre-raster stage.
    contains_all(out.glsl(ShaderStage::TessEval), &["gl_Position.z = 0.5 * (gl_Position.z + gl_Position.w);", "invariant gl_Position;"]);
    contains_none(out.vs(), &["gl_Position.z = 0.5"]);
    assert!(out.prog.requires_raw_vulkan);
}

/// The interface checks of Mojang's 26.3 `PipelineBuilder` (Renderpearl GLSL path):
/// every user input/output is a plain variable (no struct/block member, no 64-bit type)
/// with a `Location` and no `Component`, and each fragment input location has a vertex
/// output of the same base type, vector size and flatness.
#[track_caller]
pub fn mojang_interface_ok(out: &Out) {
    for r in &out.refl {
        for v in r.inputs.iter().chain(&r.outputs) {
            assert!(!v.name.contains('.') && v.base_type != "struct", "{} interface variable `{}` is a struct/block member", r.stage, v.name);
            assert!(!v.base_type.contains("64") && v.base_type != "double", "{} interface variable `{}` is 64-bit", r.stage, v.name);
            assert_eq!(v.component, 0, "{} interface variable `{}` has a Component decoration", r.stage, v.name);
        }
    }
    let vs = out.refl(ShaderStage::Vertex);
    for i in &out.refl(ShaderStage::Fragment).inputs {
        let o = vs.outputs.iter().find(|o| o.location == i.location).unwrap_or_else(|| panic!("no vertex output at location {}", i.location));
        assert_eq!((&o.base_type, o.vec_size, o.flat), (&i.base_type, i.vec_size, i.flat), "location {} (`{}` / `{}`)", i.location, o.name, i.name);
    }
}

#[test]
fn renderpearl_flattens_interface_blocks() {
    // Mojang's 26.3 pipeline builder rejects struct-typed interface variables: for the
    // Renderpearl target, vertex/fragment interface blocks become one variable per
    // member, matched by position (the member names differ between the stages here).
    let vs = "#version 330\nout VertexData { vec2 uv; flat uint tint; noperspective vec3 n[2]; } v;\nout Extra { float e; };\n\
              void main() { gl_Position = ftransform(); v.uv = gl_MultiTexCoord0.xy; v.tint = 3u; v.n[0] = vec3(1.0); v.n[1] = gl_Normal; e = 0.5; }\n";
    let fs = "#version 330\nin VertexData { vec2 coord; flat uint tint; noperspective vec3 normals[2]; } vin;\nin Extra { float e; };\nout vec4 c;\n\
              void main() { float uv = 1.0; c = vec4(vin.coord * uv, float(vin.tint) + e, vin.normals[1].x); }\n";
    let out = T::gbuffers().vs(vs).fs(fs).with(|o| o.target = sb_core::model::OutputTarget::Renderpearl).run();
    contains_all(
        out.vs(),
        &[") out vec2 sb_ib_VertexData_0;", "flat out uint sb_ib_VertexData_1;", "noperspective out vec3 sb_ib_VertexData_2[2];", "sb_ib_VertexData_2[1] = ", "sb_ib_Extra_0 = 0.5;"],
    );
    contains_all(out.fs(), &["vec4(sb_ib_VertexData_0 * uv, float(sb_ib_VertexData_1) + sb_ib_Extra_0, sb_ib_VertexData_2[1].x)"]);
    contains_none(out.vs(), &["VertexData {", "Extra {"]);
    contains_none(out.fs(), &["VertexData {", "Extra {"]);
    mojang_interface_ok(&out);
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
    // The Vulkan target keeps the blocks (Vulkan matches their members by location).
    let out = T::gbuffers().vs(vs).fs(fs).run();
    contains_all(out.vs(), &["VertexData {"]);
    let vs_out = &out.refl(ShaderStage::Vertex).outputs;
    for i in &out.refl(ShaderStage::Fragment).inputs {
        let o = vs_out.iter().find(|o| o.location == i.location).unwrap_or_else(|| panic!("no output at location {}", i.location));
        assert_eq!((o.location_count, o.flat, &o.base_type), (i.location_count, i.flat, &i.base_type), "`{}` / `{}`", o.name, i.name);
    }
}

#[test]
fn renderpearl_expands_struct_varyings() {
    // Struct-typed varyings (photon's `flat out FogParameters fog_params;`) are rejected
    // by Mojang's pipeline builder too: for Renderpearl they travel as one varying per
    // leaf member, copied from the pack's (now plain) struct global after the vertex
    // `main` and into it before the fragment `main`.
    let common = "struct Inner { float a; vec2 b[2]; };\nstruct Params { vec3 color; Inner inner[2]; int mode; };\n";
    let vs = format!(
        "#version 330\n{common}flat out Params params;\nParams make() {{ Params p; p.color = vec3(1.0); p.inner[0] = Inner(1.0, vec2[2](vec2(1.0), vec2(2.0))); p.inner[1] = p.inner[0]; p.mode = 2; return p; }}\n\
         void main() {{ gl_Position = ftransform(); params = make(); }}\n"
    );
    let fs = format!("#version 330\n{common}flat in Params params;\nout vec4 c;\nvoid main() {{ c = vec4(params.color * params.inner[1].b[1].y, float(params.mode)); }}\n");
    let out = T::gbuffers().vs(&vs).fs(&fs).with(|o| o.target = sb_core::model::OutputTarget::Renderpearl).run();
    contains_all(
        out.vs(),
        &[
            "\nParams params;",
            ") flat out vec3 sb_is_params_0;",
            "flat out vec2 sb_is_params_2[2];",
            "flat out int sb_is_params_5;",
            "sb_user_main();\n    gl_Position.z = 0.5 * (gl_Position.z + gl_Position.w);\n    sb_is_params_0 = params.color;",
            "sb_is_params_4 = params.inner[1].b;",
            "sb_is_params_5 = params.mode;",
        ],
    );
    contains_all(out.fs(), &["\nParams params;", "params.color = sb_is_params_0;\n", "params.inner[1].b = sb_is_params_4;", "params.mode = sb_is_params_5;\n    sb_user_main();"]);
    mojang_interface_ok(&out);
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
}

#[test]
fn tessellation_control_barrier_stays_in_main() {
    // GLSL allows `barrier()` only in the tessellation control `main` itself: such a
    // `main` is not wrapped; the generated prologue (a varying profile global read from
    // the patch) and epilogue (a forwarded fixed-function varying) are called from it,
    // the epilogue before every `return`.
    let vs = "#version 120\nvoid main() { gl_Position = ftransform(); gl_TexCoord[0] = gl_MultiTexCoord0; }\n";
    let tcs = "#version 400 compatibility\nlayout(vertices = 3) out;\nuniform vec4 entityColor;\nout vec3 q[];\n\
               void main() { q[gl_InvocationID] = gl_in[gl_InvocationID].gl_Position.xyz; gl_out[gl_InvocationID].gl_Position = gl_in[gl_InvocationID].gl_Position; barrier();\n\
                 float l = q[(gl_InvocationID + 1) % 3].x + entityColor.a;\n\
                 if (gl_InvocationID != 0) return;\n\
                 gl_TessLevelOuter[0] = l; gl_TessLevelOuter[1] = 1.0; gl_TessLevelOuter[2] = 1.0; gl_TessLevelInner[0] = 1.0; }\n";
    let tes = "#version 400 compatibility\nlayout(triangles) in;\nin vec3 q[];\nout vec3 r;\nvoid main() { r = q[0] * gl_TessCoord.x + q[1] * gl_TessCoord.y + q[2] * gl_TessCoord.z; gl_Position = gl_in[0].gl_Position; }\n";
    let fs = "#version 120\nvarying vec3 r;\nvoid main() { gl_FragData[0] = vec4(r, 1.0) * gl_TexCoord[0]; }\n";
    let out = T::new("vanilla_entity")
        .vs(vs)
        .stage(ShaderStage::TessControl, tcs)
        .stage(ShaderStage::TessEval, tes)
        .fs(fs)
        .run();
    let tc = out.glsl(ShaderStage::TessControl);
    contains_none(tc, &["sb_user_main"]);
    contains_all(
        tc,
        &[
            "void sb_prologue();",
            "void sb_epilogue();",
            "void main() {\n    sb_prologue();",
            "{\n        sb_epilogue();\n        return;\n    }",
            "gl_TessLevelInner[0] = 1.0;\n    sb_epilogue();\n}",
            "entityColor = sb_varyin_entityColor[gl_InvocationID];",
            "sb_v_TexCoord[gl_InvocationID] = sb_vin_TexCoord[gl_InvocationID];",
        ],
    );
    linked(&out, ShaderStage::Vertex, ShaderStage::TessControl);
    linked(&out, ShaderStage::TessControl, ShaderStage::TessEval);
    linked(&out, ShaderStage::TessEval, ShaderStage::Fragment);
}

#[test]
fn fixed_function_varyings_and_their_defaults() {
    let vs = "#version 120\nvoid main() { gl_Position = ftransform(); gl_TexCoord[0] = gl_MultiTexCoord0; gl_TexCoord[1] = gl_MultiTexCoord1; gl_FogFragCoord = 1.0; }\n";
    let fs = "#version 120\nvoid main() { gl_FragData[0] = gl_Color * gl_TexCoord[1] + gl_FogFragCoord; }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    // gl_Color is read but never written: the producer passes the profile color.
    contains_all(out.vs(), &["out vec4 sb_v_TexCoord[2];", "out float sb_v_FogFragCoord;", "out vec4 sb_v_Color;", "sb_v_Color = sb_gl_Color;"]);
    contains_all(out.fs(), &["in vec4 sb_v_Color;", "in vec4 sb_v_TexCoord[2];", "sb_v_Color * sb_v_TexCoord[1] + sb_v_FogFragCoord"]);
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
    // Secondary color and front/back colors.
    let vs = "#version 120\nvoid main() { gl_Position = ftransform(); gl_FrontColor = gl_Color; gl_BackColor = gl_Color; gl_FrontSecondaryColor = vec4(0.5); }\n";
    let fs = "#version 120\nvoid main() { gl_FragColor = gl_Color + gl_SecondaryColor; }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_all(out.vs(), &["sb_v_Color = sb_gl_Color;", "sb_v_SecondaryColor = vec4(0.5);"]);
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
    // Two-sided color is never enabled: a back color written after the front color does
    // not replace what the fragment stage reads.
    let vs = "#version 120\nvoid main() { gl_Position = ftransform(); gl_FrontColor = vec4(1.0); gl_BackColor = vec4(0.0); gl_BackSecondaryColor = vec4(0.0); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_all(out.vs(), &["sb_v_Color = vec4(1.0);", "sb_BackColor = vec4(0.0);", "sb_BackSecondaryColor = vec4(0.0);", "\nvec4 sb_BackColor;"]);
    contains_none(out.vs(), &["sb_v_Color = vec4(0.0);"]);
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
    // GL_CLAMP_VERTEX_COLOR (TRUE in compatibility contexts) clamps the colors per vertex
    // after the last pre-raster stage, before interpolation.
    let main = &out.vs()[out.vs().find("void main()").unwrap()..];
    contains_all(main, &["sb_user_main();", "sb_v_Color = clamp(sb_v_Color, 0.0, 1.0);", "sb_v_SecondaryColor = clamp(sb_v_SecondaryColor, 0.0, 1.0);"]);
    // Through a geometry stage only the geometry stage clamps (before every emit).
    let vs = "#version 120\nvoid main() { gl_Position = ftransform(); gl_FrontColor = gl_Color * 4.0; }\n";
    let gs = "#version 150 compatibility\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\n\
              void main() { for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; gl_FrontColor = gl_FrontColorIn[i]; EmitVertex(); } }\n";
    let fs = "#version 120\nvoid main() { gl_FragColor = gl_Color; }\n";
    let out = T::gbuffers().vs(vs).gs(gs).fs(fs).run();
    contains_none(out.vs(), &["clamp("]);
    contains_all(out.glsl(ShaderStage::Geometry), &["sb_v_Color = clamp(sb_v_Color, 0.0, 1.0);\n    vec4 sb_packPosition = gl_Position;"]);
    // Fixed-function colors nobody reads are not touched.
    let out = T::fullscreen().vs(vs).fs("#version 120\nvoid main() { gl_FragColor = vec4(1.0); }\n").run();
    contains_none(out.vs(), &["clamp("]);
}

#[test]
fn many_varyings_warn_above_32_locations() {
    let mut vs = String::from("#version 130\n");
    let mut fs = String::from("#version 130\n");
    let mut sum = String::from("vec4(0.0)");
    for i in 0..34 {
        vs.push_str(&format!("out vec4 v{i};\n"));
        fs.push_str(&format!("in vec4 v{i};\n"));
        sum.push_str(&format!(" + v{i}"));
    }
    vs.push_str("void main() { gl_Position = vec4(0.0);");
    for i in 0..34 {
        vs.push_str(&format!(" v{i} = vec4({i}.0);"));
    }
    vs.push_str(" }\n");
    fs.push_str(&format!("void main() {{ gl_FragData[0] = {sum}; }}\n"));
    // glslang rejects more than 32 locations (gl_MaxVaryingComponents); the warning comes first.
    let prog = T::fullscreen().vs(&vs).fs(&fs).translate_ok();
    assert!(prog.diagnostics.iter().any(|d| d.code == "xf.too-many-varyings"));
}

#[test]
fn mismatched_interface_blocks_are_reported() {
    // shrimple (max options): the geometry stage declares a member only under a
    // condition the fragment stage does not check. No driver links this.
    let vs = "#version 330\nout V { vec2 uv; } o;\nvoid main() { gl_Position = vec4(0.0); o.uv = vec2(0.0); }\n";
    let fs = "#version 330\nin V { vec2 uv; flat vec2 tile; } i;\nout vec4 c;\nvoid main() { c = vec4(i.uv, i.tile); }\n";
    let errors = T::fullscreen().vs(vs).fs(fs).errors();
    assert!(errors.iter().any(|e| e == "xf.varying-mismatch"), "{errors:?}");
}

#[test]
fn varying_profile_globals_pass_through_a_geometry_stage() {
    let vs = "#version 150\nin vec3 vaPosition;\nvoid main() { gl_Position = vec4(vaPosition, 1.0); }\n";
    let gs = "#version 150\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\n\
              void main() { for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; EmitVertex(); } }\n";
    let fs = "#version 150\nuniform vec4 entityColor;\nout vec4 c;\nvoid main() { c = entityColor; }\n";
    let out = T::new("vanilla_entity").vs(vs).gs(gs).fs(fs).run();
    contains_all(out.vs(), &["sb_vary_entityColor = entityColor;"]);
    contains_all(
        out.glsl(ShaderStage::Geometry),
        &["in vec4 sb_varyin_entityColor[];", "out vec4 sb_vary_entityColor;", "sb_vary_entityColor = sb_varyin_entityColor[0];"],
    );
    contains_all(out.fs(), &["in vec4 entityColor;"]);
    linked(&out, ShaderStage::Vertex, ShaderStage::Geometry);
    linked(&out, ShaderStage::Geometry, ShaderStage::Fragment);
}

#[test]
fn varying_profile_globals_read_by_an_intermediate_stage_are_forwarded_once() {
    // The geometry stage reads entityColor itself (declared as a uniform, as packs do)
    // and the fragment stage reads it too: one per-vertex input, one output carrying the
    // geometry stage's value (it used to declare both twice).
    let vs = "#version 150\nin vec3 vaPosition;\nvoid main() { gl_Position = vec4(vaPosition, 1.0); }\n";
    let gs = "#version 150\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\nuniform vec4 entityColor;\nout vec4 tint;\n\
              void main() { for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; tint = entityColor * 0.5; EmitVertex(); } }\n";
    let fs = "#version 150\nuniform vec4 entityColor;\nin vec4 tint;\nout vec4 c;\nvoid main() { c = entityColor * tint; }\n";
    let out = T::new("vanilla_entity").vs(vs).gs(gs).fs(fs).run();
    let g = out.glsl(ShaderStage::Geometry);
    assert_eq!(g.matches("in vec4 sb_varyin_entityColor[];").count(), 1, "{g}");
    contains_all(g, &["entityColor = sb_varyin_entityColor[0];", "out vec4 sb_vary_entityColor;", "sb_vary_entityColor = entityColor;"]);
    linked(&out, ShaderStage::Vertex, ShaderStage::Geometry);
    linked(&out, ShaderStage::Geometry, ShaderStage::Fragment);
    // A fixed-function input the geometry stage reads is not declared twice either when
    // the stage does not write the matching output.
    let vs = "#version 120\nvoid main() { gl_Position = ftransform(); gl_FrontColor = gl_Color; }\n";
    let gs = "#version 150 compatibility\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\nout float shade;\n\
              void main() { for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; shade = gl_FrontColorIn[i].r; EmitVertex(); } }\n";
    let fs = "#version 120\nvarying float shade;\nvoid main() { gl_FragData[0] = gl_Color * shade; }\n";
    let out = T::gbuffers().vs(vs).gs(gs).fs(fs).run();
    let g = out.glsl(ShaderStage::Geometry);
    assert_eq!(g.matches("in vec4 sb_vin_Color[];").count(), 1, "{g}");
    contains_all(g, &["sb_v_Color = sb_vin_Color[0];"]);
    linked(&out, ShaderStage::Geometry, ShaderStage::Fragment);
}

#[test]
fn varying_profile_globals_pass_through_tessellation() {
    // entityColor used to reach the fragment stage as zero through tessellation stages.
    let vs = "#version 400\nin vec3 vaPosition;\nout vec3 p;\nvoid main() { p = vaPosition; }\n";
    let tcs = "#version 400\nlayout(vertices = 3) out;\nin vec3 p[];\nout vec3 q[];\n\
               void main() { q[gl_InvocationID] = p[gl_InvocationID]; gl_TessLevelOuter[0] = 1.0; gl_TessLevelOuter[1] = 1.0; gl_TessLevelOuter[2] = 1.0; gl_TessLevelInner[0] = 1.0; }\n";
    let tes = "#version 400\nlayout(triangles) in;\nin vec3 q[];\nvoid main() { gl_Position = vec4(q[0] * gl_TessCoord.x + q[1] * gl_TessCoord.y + q[2] * gl_TessCoord.z, 1.0); }\n";
    let fs = "#version 400\nuniform vec4 entityColor;\nout vec4 c;\nvoid main() { c = entityColor; }\n";
    let out = T::new("vanilla_entity")
        .vs(vs)
        .stage(ShaderStage::TessControl, tcs)
        .stage(ShaderStage::TessEval, tes)
        .fs(fs)
        .run();
    contains_all(out.glsl(ShaderStage::TessControl), &["sb_vary_entityColor[gl_InvocationID] = sb_varyin_entityColor[gl_InvocationID];"]);
    contains_all(out.glsl(ShaderStage::TessEval), &["sb_vary_entityColor = sb_varyin_entityColor[0];"]);
    contains_none(out.glsl(ShaderStage::TessEval), &["vec4(0)"]);
    linked(&out, ShaderStage::Vertex, ShaderStage::TessControl);
    linked(&out, ShaderStage::TessControl, ShaderStage::TessEval);
    linked(&out, ShaderStage::TessEval, ShaderStage::Fragment);
}

#[test]
fn varyings_named_after_builtin_functions_are_renamed_on_both_sides() {
    // The vertex stage calls `length()` (so its `length` varying must be renamed); the
    // fragment stage does not, but must use the same name.
    let vs = "#version 130\nout float length;\nvoid main() { gl_Position = vec4(0.0); length = length(vec2(3.0, 4.0)); }\n";
    let fs = "#version 130\nin float length;\nvoid main() { gl_FragData[0] = vec4(length); }\n";
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_all(out.vs(), &["out float sb_kw_length;", "sb_kw_length = length(vec2(3.0, 4.0));"]);
    contains_all(out.fs(), &["in float sb_kw_length;"]);
    assert!(!out.has_diag("xf.missing-varying"));
    linked(&out, ShaderStage::Vertex, ShaderStage::Fragment);
}

#[test]
fn block_members_renamed_in_one_stage_still_link() {
    // soft-voxels-lite shadow with vanilla_entity: the anonymous vertex block's member
    // `Normal` collides with the profile input `Normal` and is renamed in the vertex
    // stage only; interfaces match by location, so the program still links.
    let vs = "#version 330\nin vec3 vaNormal;\nout vertexOut { vec2 uv; vec3 Normal; };\nvoid main() { gl_Position = vec4(0.0); uv = vec2(0.0); Normal = vaNormal; }\n";
    let gs = "#version 330\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\nin vertexOut { vec2 uv; vec3 Normal; } vIn[];\nout vec3 n;\n\
              void main() { for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; n = vIn[i].Normal; EmitVertex(); } }\n";
    let fs = "#version 330\nin vec3 n;\nout vec4 c;\nvoid main() { c = vec4(n, 1.0); }\n";
    let out = T::new("vanilla_entity").vs(vs).gs(gs).fs(fs).run();
    contains_all(out.vs(), &["vec3 sbu_Normal;", "sbu_Normal = vaNormal;"]);
    contains_all(out.glsl(ShaderStage::Geometry), &["vec3 Normal; } vIn[];"]);
    let vs_outs = iface(out.refl(ShaderStage::Vertex), true);
    let gs_ins = iface(out.refl(ShaderStage::Geometry), false);
    let locs = |m: &BTreeMap<String, (u32, u32, bool)>| m.values().map(|v| (v.0, v.1)).collect::<Vec<_>>();
    assert_eq!(locs(&vs_outs), locs(&gs_ins));
}
