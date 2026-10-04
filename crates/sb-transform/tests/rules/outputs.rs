//! ARCHITECTURE §8 "Fragment outputs" / spec §3.2–§3.3: `gl_FragColor`, `gl_FragData`,
//! user outputs, location remapping, alpha test, Renderpearl output order and
//! `gl_FragDepth`.

use sb_core::ShaderStage;
use sb_core::model::{AlphaTest, OutputTarget};
use sb_core::program::AlphaFunc;

use crate::harness::*;

const VS: &str = "#version 130\nvoid main() { gl_Position = ftransform(); }\n";

fn outputs(out: &Out) -> Vec<(u32, &str)> {
    out.prog.fragment_outputs.iter().map(|(l, t)| (*l, t.as_str())).collect()
}

/// Reflected fragment output locations.
fn reflected(out: &Out) -> Vec<(u32, String)> {
    let mut v: Vec<(u32, String)> = out.refl(ShaderStage::Fragment).outputs.iter().map(|o| (o.location, o.name.clone())).collect();
    v.sort();
    v
}

#[test]
fn frag_color_is_location_zero() {
    let out = T::fullscreen().vs(VS).fs("#version 120\nvoid main() { gl_FragColor = vec4(1.0); }\n").run();
    contains_all(out.fs(), &["layout(location = 0) out vec4 sb_FragData0;", "sb_FragData0 = vec4(1.0);"]);
    contains_none(out.fs(), &["gl_FragColor", "gl_FragData"]);
    assert_eq!(outputs(&out), [(0, "float")]);
    assert_eq!(reflected(&out), [(0, "sb_FragData0".to_string())]);
}

#[test]
fn literal_frag_data_indices() {
    let fs = "#version 120\nconst int N = 2;\nvoid main() { gl_FragData[0] = vec4(1.0); gl_FragData[N] = vec4(0.5); gl_FragData[N + 1].rg = vec2(0.25); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(
        out.fs(),
        &[
            "layout(location = 0) out vec4 sb_FragData0;",
            "layout(location = 2) out vec4 sb_FragData2;",
            "layout(location = 3) out vec4 sb_FragData3;",
            "sb_FragData2 = vec4(0.5);",
            "sb_FragData3.rg = vec2(0.25);",
        ],
    );
    assert_eq!(outputs(&out), [(0, "float"), (2, "float"), (3, "float")]);
}

#[test]
fn dynamic_frag_data_index_uses_an_array() {
    let fs = "#version 130\nuniform int frameCounter;\nvoid main() { for (int i = 0; i < 3; i++) gl_FragData[i] = vec4(float(i)); gl_FragData[frameCounter & 1] *= 0.5; }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(
        out.fs(),
        &["vec4 sb_FragDataArr[8] = vec4[8](vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0));", "sb_FragDataArr[i] = vec4(float(i));", "layout(location = 7) out vec4 sb_FragData7;", "sb_FragData7 = sb_FragDataArr[7];"],
    );
    assert_eq!(out.prog.fragment_outputs.len(), 8);
    // With an attachment list, only the entries with an attachment are copied out; the
    // array keeps gl_MaxDrawBuffers (8) elements, so a dynamic index past the list
    // (legal in GL, the write is just dropped) stays in bounds.
    let out = T::fullscreen().vs(VS).fs(fs).with(|o| o.output_locations = Some(vec![4, 1, 2])).run();
    contains_all(
        out.fs(),
        &["vec4 sb_FragDataArr[8] = vec4[8](vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0), vec4(0.0));", "layout(location = 4) out vec4 sb_FragData0;", "layout(location = 1) out vec4 sb_FragData1;", "sb_FragData0 = sb_FragDataArr[0];"],
    );
    contains_none(out.fs(), &["sb_FragData3 ="]);
    assert_eq!(outputs(&out), [(1, "float"), (2, "float"), (4, "float")]);
}

#[test]
fn a_single_user_output_without_location_is_location_zero() {
    let fs = "#version 150\nout vec4 color;\nvoid main() { color = vec4(1.0); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["layout(location = 0) out vec4 color;"]);
    assert!(!out.has_diag("xf.output-location"));
}

#[test]
fn a_lone_out_color_n_keeps_location_n() {
    // OptiFine binds `outColorN` to location N (Iris `transformFragmentCore` likewise),
    // also when it is the only output: it writes the N-th RENDERTARGETS entry.
    let fs = "#version 150\nout vec4 outColor3;\nvoid main() { outColor3 = vec4(1.0); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["layout(location = 3) out vec4 outColor3;"]);
    assert_eq!(reflected(&out), [(3, "outColor3".to_string())]);
    // Only outColor0..7 are bound by name.
    let fs = "#version 150\nout vec4 outColor9;\nvoid main() { outColor9 = vec4(1.0); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["layout(location = 0) out vec4 outColor9;"]);
    // Through the shared attachment list: logical 3 -> physical slot 1.
    let fs = "#version 150\nout vec4 outColor3;\nvoid main() { outColor3 = vec4(1.0); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).with(|o| o.output_locations = Some(vec![0, 5, 6, 1])).run();
    assert_eq!(reflected(&out), [(1, "outColor3".to_string())]);
}

#[test]
fn out_color_names_give_locations_and_others_follow_declaration_order() {
    let fs = "#version 150\nout vec4 extra;\nout vec4 outColor2;\nout vec4 more[2];\nout vec4 outColor0;\nlayout(location = 6) out vec4 fixedLoc;\n\
              void main() { extra = vec4(1.0); outColor2 = vec4(2.0); more[0] = vec4(3.0); more[1] = vec4(3.5); outColor0 = vec4(4.0); fixedLoc = vec4(5.0); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(
        out.fs(),
        &[
            "layout(location = 0) out vec4 outColor0;",
            "layout(location = 2) out vec4 outColor2;",
            "layout(location = 6) out vec4 fixedLoc;",
            // `extra` takes the first free location; the array needs two consecutive ones.
            "layout(location = 1) out vec4 extra;",
            "layout(location = 3) out vec4 more[2];",
        ],
    );
    assert!(out.has_diag("xf.output-location"));
    assert_eq!(outputs(&out).iter().map(|o| o.0).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 6]);
}

#[test]
fn output_arrays_without_location_do_not_overlap() {
    // arc-shader (cascaded shadows, max options) declares these as fragment outputs.
    let fs = "#version 150\nout vec4 outColor0;\nout vec4 outColor1;\nout vec3 shadowPos[2];\nout float shadowBias[2];\n\
              void main() { outColor0 = vec4(1.0); outColor1 = vec4(0.0); shadowPos[0] = vec3(0.0); shadowBias[1] = 1.0; }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["layout(location = 2) out vec3 shadowPos[2];", "layout(location = 4) out float shadowBias[2];"]);
}

#[test]
fn mixing_user_outputs_and_frag_data() {
    let fs = "#version 130\nout vec4 velocity;\nvoid main() { gl_FragData[0] = vec4(1.0); gl_FragData[1] = vec4(0.0); velocity = vec4(0.5); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["layout(location = 0) out vec4 sb_FragData0;", "layout(location = 1) out vec4 sb_FragData1;", "layout(location = 2) out vec4 velocity;"]);
}

#[test]
fn integer_outputs_report_their_base_type() {
    let fs = "#version 150\nlayout(location = 0) out vec4 c;\nlayout(location = 1) out uvec2 ids;\nlayout(location = 2) out int mat;\nvoid main() { c = vec4(1.0); ids = uvec2(1u); mat = 3; }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    assert_eq!(outputs(&out), [(0, "float"), (1, "uint"), (2, "int")]);
}

#[test]
fn output_locations_remap_and_remove_outputs() {
    let fs = "#version 130\n/* RENDERTARGETS: 0,3,7 */\nout vec4 outColor2;\nvoid main() { gl_FragData[0] = vec4(1.0); gl_FragData[1] = vec4(0.0); outColor2 = vec4(0.5); }\n";
    // Logical 0 -> 5, logical 1 -> 2, logical 2 has no attachment.
    let out = T::fullscreen().vs(VS).fs(fs).with(|o| o.output_locations = Some(vec![5, 2])).run();
    contains_all(
        out.fs(),
        &["layout(location = 5) out vec4 sb_FragData0;", "layout(location = 2) out vec4 sb_FragData1;", "\nvec4 outColor2 = (vec4(0));", "outColor2 = vec4(0.5);"],
    );
    contains_none(out.fs(), &["out vec4 outColor2"]);
    assert!(out.has_diag("xf.output-removed"));
    assert_eq!(outputs(&out), [(2, "float"), (5, "float")]);
    assert_eq!(reflected(&out).iter().map(|o| o.0).collect::<Vec<_>>(), [2, 5]);
}

#[test]
fn remapped_output_arrays_are_split_when_not_consecutive() {
    let fs = "#version 150\nout vec4 outColor0[3];\nvoid main() { outColor0[0] = vec4(1.0); outColor0[1] = vec4(2.0); outColor0[2] = vec4(3.0); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).with(|o| o.output_locations = Some(vec![0, 1, 2])).run();
    contains_all(out.fs(), &["layout(location = 0) out vec4 outColor0[3];"]);
    let out = T::fullscreen().vs(VS).fs(fs).with(|o| o.output_locations = Some(vec![3, 1])).run();
    contains_all(
        out.fs(),
        &[
            "\nvec4 outColor0[3] = (vec4[3](vec4(0), vec4(0), vec4(0)));",
            "layout(location = 3) out vec4 sb_Out_outColor0_0;",
            "layout(location = 1) out vec4 sb_Out_outColor0_1;",
            "sb_Out_outColor0_0 = outColor0[0];",
            "sb_Out_outColor0_1 = outColor0[1];",
        ],
    );
    contains_none(out.fs(), &["sb_Out_outColor0_2"]);
    assert!(out.has_diag("xf.output-split"));
    assert_eq!(outputs(&out), [(1, "float"), (3, "float")]);
}

#[test]
fn alpha_test_epilogue() {
    let vs = "#version 120\nvarying vec4 c;\nvoid main() { gl_Position = ftransform(); c = gl_Color; }\n";
    let fs = "#version 120\nvarying vec4 c;\nvoid main() { if (c.r > 2.0) return; gl_FragData[0] = c; }\n";
    let at = |func, reference| move |o: &mut sb_transform::TransformOptions| o.alpha_test = Some(AlphaTest { func, reference });
    let out = T::gbuffers().with(at(AlphaFunc::Greater, 0.1)).vs(vs).fs(fs).run();
    let main = &out.fs()[out.fs().find("void main()").unwrap()..];
    contains_all(main, &["sb_user_main();\n    if (!(sb_FragData0.a > alphaTestRef)) discard;"]);
    assert!(out.prog.draw_members_used.contains(&"alphaTestRef".to_string()));
    let out = T::gbuffers().with(at(AlphaFunc::GEqual, 0.5)).vs(vs).fs(fs).run();
    contains_all(out.fs(), &["if (!(sb_FragData0.a >= alphaTestRef)) discard;"]);
    let out = T::gbuffers().with(at(AlphaFunc::Never, 0.0)).vs(vs).fs(fs).run();
    contains_all(&out.fs()[out.fs().find("void main()").unwrap()..], &["sb_user_main();\n    discard;"]);
    let out = T::gbuffers().with(at(AlphaFunc::Always, 0.0)).vs(vs).fs(fs).run();
    contains_none(out.fs(), &["discard"]);
    // No alpha test configured (fullscreen programs).
    let out = T::fullscreen().vs(vs).fs(fs).run();
    contains_none(out.fs(), &["discard", "alphaTestRef"]);
    // Output 0 is not a vec4: no alpha test.
    let fs = "#version 150\nlayout(location = 0) out vec2 v;\nvoid main() { v = vec2(1.0); }\n";
    let out = T::gbuffers().with(at(AlphaFunc::Greater, 0.1)).vs(vs).fs(fs).run();
    contains_none(out.fs(), &["discard"]);
    // Iris tests alpha only in compatibility-path stages writing gl_FragData[0] /
    // gl_FragColor: a user output at location 0 (here Photon's packed data: its alpha is
    // light levels, not coverage) or a core-profile stage tests alpha itself.
    for fs in [
        "#version 150\nout vec4 albedo;\nvoid main() { albedo = vec4(1.0); }\n",
        "#version 400 compatibility\nlayout(location = 0) out vec4 gbuffer_data_0;\nvoid main() { gbuffer_data_0 = vec4(0.5, 0.5, 0.5, 0.0); }\n",
        "#version 330 core\nvoid main() { gl_FragData[0] = vec4(1.0); }\n",
        "#version 150\nvoid main() { gl_FragColor = vec4(1.0); }\n",
        // Only a dynamically indexed gl_FragData: Iris leaves it alone (no iris_FragData0).
        "#version 120\nuniform int frameCounter;\nvoid main() { gl_FragData[frameCounter & 1] = vec4(1.0); }\n",
    ] {
        let out = T::gbuffers().with(at(AlphaFunc::Greater, 0.1)).vs(vs).fs(fs).run();
        contains_none(out.fs(), &["discard"]);
        assert!(out.has_diag("xf.alpha-test"), "{fs}");
    }
    // The compatibility profile at a newer version is tested, as are mixed outputs.
    for fs in [
        "#version 330 compatibility\nvoid main() { gl_FragColor = vec4(1.0); }\n",
        "#version 130\nuniform int frameCounter;\nvoid main() { gl_FragData[0] = vec4(1.0); gl_FragData[frameCounter & 1] *= 0.5; }\n",
    ] {
        let out = T::gbuffers().with(at(AlphaFunc::Greater, 0.1)).vs(vs).fs(fs).run();
        contains_all(out.fs(), &["alphaTestRef)) discard;"]);
        assert!(!out.has_diag("xf.alpha-test"), "{fs}");
    }
}

#[test]
fn renderpearl_outputs_are_touched_in_location_order() {
    let fs = "#version 130\nvoid main() { gl_FragData[2] = vec4(1.0); gl_FragData[0] = vec4(0.5); }\n";
    let out = T::fullscreen().with(|o| o.target = OutputTarget::Renderpearl).vs(VS).fs(fs).run();
    let f = out.fs();
    contains_all(
        f,
        &[
            "layout(location = 1) out vec4 sb_Unused1;",
            "void sb_touchOutputs() {\n    sb_FragData0 = vec4(0);\n    sb_Unused1 = vec4(0.0);\n    sb_FragData2 = vec4(0);\n}",
            "void main() {\n    sb_touchOutputs();\n    sb_user_main();",
        ],
    );
    assert_eq!(reflected(&out).iter().map(|o| o.0).collect::<Vec<_>>(), [0, 1, 2]);
    // The Vulkan target needs none of this.
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_none(out.fs(), &["sb_touchOutputs", "sb_Unused"]);
}

#[test]
fn frag_depth_written_or_only_read() {
    let fs = "#version 130\nvoid main() { gl_FragDepth = 0.5; gl_FragData[0] = vec4(gl_FragDepth); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["gl_FragDepth = 0.5;"]);
    assert!(out.refl(ShaderStage::Fragment).execution_modes.iter().any(|m| m.mode == "DepthReplacing"));
    // Read but never written: lenient drivers return the fragment depth.
    let fs = "#version 130\nvoid main() { gl_FragData[0] = vec4(gl_FragDepth); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["vec4(gl_FragCoord.z)"]);
    assert!(out.has_diag("xf.frag-depth"));
    assert!(!out.refl(ShaderStage::Fragment).execution_modes.iter().any(|m| m.mode == "DepthReplacing"));
}
