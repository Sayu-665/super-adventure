//! Comparison-sampler emulation (`TransformOptions::emulate_shadow_samplers`, for hosts
//! whose samplers cannot compare) and shadow-pass semantics.

use sb_compile::DescriptorKind;
use sb_core::ShaderStage;
use sb_core::model::{DepthMode, OutputTarget};

use crate::harness::*;

const VS: &str = "#version 130\nvarying vec2 uv;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.xy; }\n";

fn emulated() -> T {
    T::fullscreen().with(|o| o.emulate_shadow_samplers = true)
}

/// Whether the fragment stage binds a comparison sampler named `name`.
fn shadow_descriptor(out: &Out, name: &str) -> Option<bool> {
    out.refl(ShaderStage::Fragment).descriptors.iter().find(|d| d.name == name).map(|d| match &d.kind {
        DescriptorKind::CombinedImageSampler { shadow, .. } => *shadow,
        other => panic!("{name} is not a combined image sampler: {other:?}"),
    })
}

#[test]
fn comparison_samplers_stay_without_emulation() {
    let fs = "#version 130\nuniform sampler2DShadow shadowtex0;\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = vec4(shadow2D(shadowtex0, vec3(uv, 0.5)).r); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["uniform sampler2DShadow shadowtex0;", "vec4(texture(shadowtex0, vec3(uv, 0.5))).r"]);
    contains_none(out.fs(), &["sb_shadowCompare"]);
    assert_eq!(shadow_descriptor(&out, "shadowtex0"), Some(true));
}

#[test]
fn every_lookup_form_is_emulated_with_bilinear_pcf() {
    let fs = "#version 130\n\
        uniform sampler2DShadow shadowtex0;\n\
        uniform sampler2DShadow shadowtex1;\n\
        varying vec2 uv;\n\
        void main() {\n\
          vec3 p = vec3(uv, 0.5);\n\
          vec4 q = vec4(uv, 0.5, 1.0);\n\
          float s = 0.0;\n\
          s += shadow2D(shadowtex0, p).r;\n\
          s += shadow2DLod(shadowtex0, p, 0.0).x;\n\
          s += shadow2DProj(shadowtex1, q).r;\n\
          s += shadow2DProjLod(shadowtex1, q, 1.0).r;\n\
          s += texture(shadowtex0, p);\n\
          s += texture(shadowtex0, p, 0.5);\n\
          s += textureLod(shadowtex0, p, 2.0);\n\
          s += textureGrad(shadowtex0, p, vec2(0.0), vec2(0.0));\n\
          s += textureOffset(shadowtex0, p, ivec2(1, -1));\n\
          s += textureLodOffset(shadowtex0, p, 0.0, ivec2(1, 0));\n\
          s += textureGradOffset(shadowtex0, p, vec2(0.0), vec2(0.0), ivec2(0, 1));\n\
          s += textureProj(shadowtex1, q);\n\
          s += textureProjLod(shadowtex1, q, 0.0);\n\
          s += textureProjOffset(shadowtex1, q, ivec2(1));\n\
          s += textureProjGradOffset(shadowtex1, q, vec2(0.0), vec2(0.0), ivec2(1));\n\
          vec4 g = textureGather(shadowtex0, uv, 0.5) + textureGatherOffset(shadowtex1, uv, 0.5, ivec2(1, 1));\n\
          ivec2 size = textureSize(shadowtex0, 0);\n\
          gl_FragData[0] = vec4(s) + g + vec4(size, 0.0, 0.0);\n\
        }\n";
    let out = emulated().vs(VS).fs(fs).run();
    let f = out.fs();
    contains_all(
        f,
        &[
            // Declarations become plain samplers.
            "uniform sampler2D shadowtex0;",
            "uniform sampler2D shadowtex1;",
            // Legacy forms (splatted to vec4 first).
            "vec4(sb_shadowCompare(shadowtex0, p)).r",
            "vec4(sb_shadowCompare(shadowtex0, p)).x",
            "vec4(sb_shadowCompareProj(shadowtex1, q)).r",
            // Core forms: bias, LOD and gradients are dropped; offsets are kept.
            "s += sb_shadowCompare(shadowtex0, p);",
            "s += sb_shadowCompareOffset(shadowtex0, p, ivec2(1, -1));",
            "s += sb_shadowCompareOffset(shadowtex0, p, ivec2(1, 0));",
            "s += sb_shadowCompareOffset(shadowtex0, p, ivec2(0, 1));",
            "s += sb_shadowCompareProj(shadowtex1, q);",
            "s += sb_shadowCompareProjOffset(shadowtex1, q, ivec2(1));",
            "sb_shadowGather(shadowtex0, uv, 0.5)",
            "sb_shadowGatherOffset(shadowtex1, uv, 0.5, ivec2(1, 1))",
            "textureSize(shadowtex0, 0)",
            // The helper: 2x2 gather, LEQUAL compare against the clamped reference, bilinear weights.
            "float sb_shadowCompare(sampler2D sb_s, vec3 sb_p)",
            "lessThanEqual(vec4(clamp(sb_p.z, 0.0, 1.0)), textureGather(sb_s, sb_p.xy))",
            "fract(sb_p.xy * vec2(textureSize(sb_s, 0)) - 0.5)",
            "mix(mix(sb_lit.w, sb_lit.z, sb_f.x), mix(sb_lit.x, sb_lit.y, sb_f.x), sb_f.y)",
            "sb_q.xyz / sb_q.w",
        ],
    );
    contains_none(f, &["sampler2DShadow", "greaterThanEqual", "texture(shadowtex", "textureLod(shadowtex", "textureProj(shadowtex"]);
    assert_eq!(shadow_descriptor(&out, "shadowtex0"), Some(false));
    assert_eq!(shadow_descriptor(&out, "shadowtex1"), Some(false));
}

#[test]
fn helpers_are_emitted_only_when_used() {
    let fs = "#version 130\nuniform sampler2DShadow shadowtex0;\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = vec4(texture(shadowtex0, vec3(uv, 0.5))); }\n";
    let out = emulated().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["float sb_shadowCompare("]);
    contains_none(out.fs(), &["sb_shadowCompareProj", "sb_shadowCompareOffset", "sb_shadowGather"]);
    // A program without comparison samplers gets no helper at all.
    let out = emulated().vs(VS).fs(FS_UV).run();
    contains_none(out.fs(), &["sb_shadow"]);
}

#[test]
fn comparison_sampler_parameters_are_retyped() {
    let fs = "#version 130\n\
        uniform sampler2DShadow shadowtex0;\n\
        varying vec2 uv;\n\
        float pcf(sampler2DShadow s, vec3 p);\n\
        float pcf(sampler2DShadow s, vec3 p) {\n\
          float sum = 0.0;\n\
          for (int i = -1; i <= 1; i++) sum += shadow2D(s, p + vec3(float(i) / 1024.0, 0.0, 0.0)).r;\n\
          return sum / 3.0;\n\
        }\n\
        float plain(sampler2D s, vec2 p) { return texture(s, p).r; }\n\
        void main() { gl_FragData[0] = vec4(pcf(shadowtex0, vec3(uv, 0.5)), plain(shadowtex0, uv), 0.0, 1.0); }\n";
    let out = emulated().vs(VS).fs(fs).run();
    contains_all(
        out.fs(),
        &[
            "float pcf(sampler2D s, vec3 p);",
            "float pcf(sampler2D s, vec3 p) {",
            "vec4(sb_shadowCompare(s, p + vec3(float(i) / 1024.0, 0.0, 0.0))).r",
            // A plain-sampler parameter keeps its plain reads.
            "return texture(s, p).r;",
        ],
    );
}

#[test]
fn local_names_shadowing_a_comparison_sampler_are_not_rewritten() {
    let fs = "#version 130\n\
        uniform sampler2DShadow shadowtex0;\n\
        uniform sampler2D colortex0;\n\
        varying vec2 uv;\n\
        float f(sampler2D shadowtex0) { return texture(shadowtex0, uv).r; }\n\
        void main() { gl_FragData[0] = vec4(f(colortex0) + texture(shadowtex0, vec3(uv, 0.5))); }\n";
    let out = emulated().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["return texture(shadowtex0, uv).r;", "sb_shadowCompare(shadowtex0, vec3(uv, 0.5))"]);
}

#[test]
fn reversed_depth_compares_greater_equal_against_the_flipped_reference() {
    let fs = "#version 130\nuniform sampler2DShadow shadowtex0;\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = vec4(shadow2D(shadowtex0, vec3(uv, 0.25)).r + textureProj(shadowtex0, vec4(uv, 0.25, 1.0))); }\n";
    let out = emulated()
        .with(|o| {
            o.depth_mode = DepthMode::ReversedZeroToOne;
            o.invert_depth_reads = true;
        })
        .vs(VS)
        .fs(fs)
        .run();
    contains_all(
        out.fs(),
        &[
            "vec4(sb_shadowCompare(shadowtex0, sb_revRef3(vec3(uv, 0.25)))).r",
            "sb_shadowCompareProj(shadowtex0, sb_revRefProj(vec4(uv, 0.25, 1.0)))",
            "greaterThanEqual(vec4(clamp(sb_p.z, 0.0, 1.0)), textureGather(sb_s, sb_p.xy))",
        ],
    );
    contains_none(out.fs(), &["lessThanEqual"]);
}

#[test]
fn emulation_works_for_the_renderpearl_target_and_gbuffers() {
    let fs = "#version 130\nuniform sampler2D texture;\nuniform sampler2DShadow shadowtex1;\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = texture2D(texture, uv) * shadow2D(shadowtex1, vec3(uv, 0.5)).r; }\n";
    let out = T::gbuffers()
        .with(|o| {
            o.emulate_shadow_samplers = true;
            o.target = OutputTarget::Renderpearl;
        })
        .vs(VS)
        .fs(fs)
        .run();
    contains_all(out.fs(), &["uniform sampler2D shadowtex1;", "uniform sampler2D Sampler0;", "sb_shadowCompare(shadowtex1"]);
    contains_none(out.fs(), &["binding"]);
}

#[test]
fn other_comparison_sampler_types_are_reported() {
    let fs = "#version 150\nuniform samplerCubeShadow shadowCube;\nuniform sampler2DShadow shadowtex0;\nin vec2 uv;\nout vec4 c;\nvoid main() { c = vec4(texture(shadowCube, vec4(1.0, 0.0, 0.0, 0.5)) + texture(shadowtex0, vec3(uv, 0.5))); }\n";
    let vs = "#version 150\nout vec2 uv;\nvoid main() { gl_Position = vec4(0.0, 0.0, 0.0, 1.0); uv = vec2(0.0); }\n";
    let out = emulated().vs(vs).fs(fs).run();
    assert!(out.has_diag("xf.shadow-emulation"), "{:?}", out.prog.diagnostics);
    contains_all(out.fs(), &["samplerCubeShadow", "texture(", "sb_shadowCompare(shadowtex0"]);
}

#[test]
fn gather_offsets_on_a_comparison_sampler_is_an_error() {
    let fs = "#version 400\nuniform sampler2DShadow shadowtex0;\nin vec2 uv;\nout vec4 c;\nconst ivec2 o[4] = ivec2[4](ivec2(0), ivec2(1), ivec2(2), ivec2(3));\nvoid main() { c = textureGatherOffsets(shadowtex0, uv, 0.5, o); }\n";
    let vs = "#version 400\nout vec2 uv;\nvoid main() { gl_Position = vec4(0.0, 0.0, 0.0, 1.0); uv = vec2(0.0); }\n";
    let errors = emulated().vs(vs).fs(fs).errors();
    assert!(errors.iter().any(|e| e == "xf.shadow-emulation"), "{errors:?}");
}

#[test]
fn shadow_pass_uses_shadow_matrices_for_world_space_profiles() {
    let vs = "#version 120\nvarying vec2 uv;\nvoid main() { gl_Position = gl_ProjectionMatrix * gl_ModelViewMatrix * gl_Vertex; uv = gl_MultiTexCoord0.xy; }\n";
    // World-space profiles (terrain, DH, Sodium): Iris's shadow programs see
    // gl_ModelViewMatrix == shadowModelView and gl_ProjectionMatrix == shadowProjection.
    for profile in ["vanilla_terrain", "vanilla_terrain_basic", "dh_terrain", "sodium_terrain"] {
        let out = T::new(profile).with(|o| o.is_shadow_pass = true).vs(vs).fs(FS_UV).run();
        contains_all(out.vs(), &["mat4 sb_ModelView = shadowModelView;", "mat4 sb_Projection = shadowProjection;"]);
        assert!(out.prog.frame_members_used.iter().any(|m| m == "shadowModelView"), "{profile}");
        // Outside the shadow pass the profile's own matrices are used.
        let out = T::new(profile).vs(vs).fs(FS_UV).run();
        contains_none(out.vs(), &["shadowModelView", "shadowProjection"]);
    }
    // Entity profiles keep the host model-view (the host composes the shadow view).
    let out = T::new("vanilla_entity").with(|o| o.is_shadow_pass = true).vs(vs).fs(FS_UV).run();
    contains_all(out.vs(), &["mat4 sb_ModelView = sb_hDynamic.ModelViewMat;"]);
    contains_none(out.vs(), &["shadowModelView"]);
}

#[test]
fn shadow_pass_matrices_reach_ftransform_and_derived_matrices() {
    let vs = "#version 120\nvoid main() { gl_Position = ftransform() + gl_ModelViewMatrixInverse[3] * 0.0; }\n";
    let fs = "#version 120\nvoid main() { gl_FragData[0] = vec4(1.0); }\n";
    let out = T::gbuffers().with(|o| o.is_shadow_pass = true).vs(vs).fs(fs).run();
    contains_all(
        out.vs(),
        &["mat4 sb_ModelView = shadowModelView;", "sb_Projection * (sb_ModelView * sb_gl_Vertex)", "mat4 sb_ModelViewInverse = inverse(sb_ModelView);"],
    );
}

#[test]
fn overloads_differing_only_by_comparison_samplers_stay_distinct() {
    // BSL: `texture2DShadow(sampler2D, vec3)` (manual PCF) next to
    // `texture2DShadow(sampler2DShadow, vec3)` (hardware compare).
    let fs = "#version 130\n\
        uniform sampler2DShadow shadowtex0;\n\
        uniform sampler2D shadowcolor0;\n\
        varying vec2 uv;\n\
        float texture2DShadow(sampler2D s, vec3 p) { return step(p.z, texture2D(s, p.xy).r); }\n\
        float texture2DShadow(sampler2DShadow s, vec3 p) { return shadow2D(s, p).x; }\n\
        void main() { vec3 p = vec3(uv, 0.5); gl_FragData[0] = vec4(texture2DShadow(shadowtex0, p), texture2DShadow(shadowcolor0, p), 0.0, 1.0); }\n";
    for reversed in [false, true] {
        let out = emulated()
            .with(|o| {
                if reversed {
                    o.depth_mode = DepthMode::ReversedZeroToOne;
                    o.invert_depth_reads = true;
                }
            })
            .vs(VS)
            .fs(fs)
            .run();
        contains_all(out.fs(), &["float texture2DShadow(sampler2D s, vec3 p)", "float sb_cmp_texture2DShadow(sampler2D s, vec3 p)"]);
        if reversed {
            // The depth-read clones are distinct too.
            contains_all(out.fs(), &["sb_cmp_sb_depth_0_texture2DShadow(shadowtex0, p)"]);
        } else {
            contains_all(out.fs(), &["sb_cmp_texture2DShadow(shadowtex0, p)", "texture2DShadow(shadowcolor0, p)"]);
        }
    }
}
