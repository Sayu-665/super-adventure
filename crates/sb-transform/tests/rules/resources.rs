//! ARCHITECTURE §5 / §8 "Uniforms" and "Version and extensions": the `sb_Frame` /
//! `sb_Draw` blocks, loose-uniform resolution, sampler aliases and bindings, images,
//! SSBOs and UBOs, the Renderpearl target, `requires_raw_vulkan` and extensions.

use sb_compile::DescriptorKind;
use sb_core::ShaderStage;
use sb_core::model::{OutputTarget, ResourceRef};
use sb_uniforms::{ProgramClass, ResourceContext};

use crate::harness::*;

const VS: &str = "#version 130\nvarying vec2 uv;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.xy; }\n";

/// Every reflected descriptor of every stage sits at its binding-table set/binding.
#[track_caller]
fn bindings_match_the_table(out: &Out) {
    for r in &out.refl {
        for d in &r.descriptors {
            let Some(e) = out.pack.bindings.get(&d.name) else {
                // Host blocks and samplers are looked up by the name they provide.
                continue;
            };
            assert_eq!((d.set, d.binding), (e.set, e.binding), "{} in the {} stage", d.name, r.stage);
        }
    }
}

#[test]
fn loose_uniforms_become_offset_members_of_the_frame_block() {
    let fs = "#version 130\nuniform float frameTimeCounter;\nuniform vec3 cameraPosition;\nuniform float viewWidth, viewHeight;\nuniform int unusedHere;\nvarying vec2 uv;\n\
              void main() { gl_FragData[0] = vec4(cameraPosition * frameTimeCounter, viewWidth / viewHeight); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    let f = out.fs();
    contains_all(f, &["layout(std140, set = 0, binding = 0) uniform sb_Frame {", "vec3 cameraPosition;", "float frameTimeCounter;", "float viewWidth;"]);
    contains_none(f, &["uniform float frameTimeCounter", "unusedHere", "} sb_"]);
    // Offsets are the pack-global layout's.
    for name in ["frameTimeCounter", "cameraPosition", "viewWidth", "viewHeight"] {
        let m = out.pack.layout.frame.member(name).unwrap();
        contains_all(f, &[&format!("layout(offset = {}) {} {name};", m.offset, m.ty.glsl_name())]);
    }
    let mut used = out.prog.frame_members_used.clone();
    used.sort();
    assert_eq!(used, ["cameraPosition", "frameTimeCounter", "viewHeight", "viewWidth"]);
    // The reflected block matches the layout.
    let block = out.refl(ShaderStage::Fragment).descriptors.iter().find(|d| d.block_type_name.as_deref() == Some("sb_Frame")).unwrap();
    assert_eq!((block.set, block.binding), (0, 0));
}

#[test]
fn reflected_frame_members_sit_at_the_pack_layout_offsets() {
    // Two programs declare overlapping sets of uniforms of every std140 shape (scalars,
    // vec3 followed by a scalar, bool, matrices, arrays): every member a stage declares
    // must sit, after glslang's own std140 layout, exactly at the pack-global offset,
    // with std140 array and matrix strides, so the host writes one buffer for all.
    let other = "#version 130\nuniform float a;\nuniform vec3 b;\nuniform int c;\nuniform mat3 m3;\nuniform float arr[3];\nuniform bool flag;\nuniform vec2 v2;\nuniform mat2 m2;\n\
                 void main() { gl_FragData[0] = vec4(a + b.x + float(c) + m3[0][0] + arr[1] + float(flag) + v2.x + m2[1][1]); }\n";
    let fs = "#version 130\nuniform vec2 v2;\nuniform float arr[3];\nuniform mat3 m3;\nuniform vec3 b;\nuniform ivec3 iv;\nuniform mat4 m4;\nuniform uint u;\nuniform mat2 m2;\nuniform bool flag;\nvarying vec2 uv;\n\
              void main() { gl_FragData[0] = vec4(v2.x + arr[2] + m3[2][2] + b.z + float(iv.y) + m4[3][3] + float(u) + m2[1][0] + float(flag)); }\n";
    let out = T::fullscreen().other(ShaderStage::Fragment, other, ProgramClass::Fullscreen).vs(VS).fs(fs).run();
    let block = out.refl(ShaderStage::Fragment).descriptors.iter().find(|d| d.block_type_name.as_deref() == Some("sb_Frame")).unwrap();
    let members = block.kind.members().unwrap();
    assert_eq!(members.len(), 9);
    for m in members {
        let l = out.pack.layout.frame.member(&m.name).unwrap_or_else(|| panic!("{} is not in the layout", m.name));
        assert_eq!(m.offset, l.offset, "offset of {}", m.name);
        if l.ty.array.is_some() {
            assert_eq!(m.array_stride, Some(16), "std140 array stride of {}", m.name);
        }
        if l.ty.is_matrix() {
            assert_eq!(m.matrix_stride, Some(16), "std140 matrix stride of {}", m.name);
        }
    }
    // Members never overlap.
    let mut spans: Vec<(u32, u32)> = members.iter().map(|m| (m.offset, m.offset + m.size)).collect();
    spans.sort();
    assert!(spans.windows(2).all(|w| w[0].1 <= w[1].0), "{spans:?}");
}

#[test]
fn renderpearl_target_has_no_set_or_binding() {
    let fs = "#version 130\nuniform float frameTimeCounter;\nuniform sampler2D colortex0;\nuniform sampler2D gaux1;\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = texture2D(colortex0, uv) * texture2D(gaux1, uv) * frameTimeCounter; }\n";
    let out = T::fullscreen().with(|o| o.target = OutputTarget::Renderpearl).vs(VS).fs(fs).run();
    contains_all(out.fs(), &["layout(std140) uniform sb_Frame {", "uniform sampler2D colortex0;", "uniform sampler2D colortex4;"]);
    contains_none(out.fs(), &["(set =", ", set =", "binding ="]);
}

#[test]
fn draw_block_members_and_type_conflicts() {
    // `worldTime` is an int builtin; another program of this pack declares it first (as
    // an int): the first type wins, this program reads its own `sb_as_float_worldTime` member.
    let fs = "#version 130\nuniform float worldTime;\nuniform int entityId;\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = vec4(worldTime, float(entityId), uv); }\n";
    let other = "#version 130\nuniform int worldTime;\nvoid main() { gl_FragData[0] = vec4(float(worldTime)); }\n";
    let out = T::new("vanilla_entity").other(ShaderStage::Fragment, other, ProgramClass::Gbuffers).vs(VS).fs(fs).run();
    let f = out.fs();
    let renamed = out.pack.members.member_name("worldTime", sb_core::GlslType::FLOAT).unwrap().to_string();
    assert_eq!(renamed, "sb_as_float_worldTime");
    assert!(!f.contains("__"), "GLSL reserves names containing `__`:\n{f}");
    contains_all(f, &[&format!("float {renamed};"), &format!("vec4({renamed}, float(entityId), uv)"), "uniform sb_Draw {", "int entityId;"]);
    assert!(out.prog.draw_members_used.contains(&"entityId".to_string()));
}

#[test]
fn uniform_arrays_and_initializers() {
    let fs = "#version 330\nuniform vec4 lights[4];\nuniform float customStrength = 2.5;\nout vec4 c;\nvoid main() { c = lights[3] * customStrength; }\n";
    let out = T::fullscreen().vs("#version 330\nvoid main() { gl_Position = vec4(0.0); }\n").fs(fs).run();
    contains_all(out.fs(), &["vec4 lights[4];", "float customStrength;", "c = lights[3] * customStrength;"]);
    assert_eq!(out.pack.layout.frame.member("customStrength").unwrap().default, Some(vec![2.5]));
    assert_eq!(out.pack.layout.frame.member("lights").unwrap().ty.array, Some(4));
}

#[test]
fn sampler_aliases_are_canonicalized_and_bound() {
    let fs = "#version 130\nuniform sampler2D gcolor;\nuniform sampler2D gdepth;\nuniform sampler2D gnormal;\nuniform sampler2D composite;\nuniform sampler2D gaux1;\nuniform sampler2D gaux4;\n\
              uniform sampler2D gdepthtex;\nuniform sampler2D shadow;\nuniform sampler2D shadowcolor;\nuniform sampler2D dhDepthTex;\nuniform sampler2D noisetex;\nuniform sampler2D somethingElse;\nvarying vec2 uv;\n\
              void main() { gl_FragData[0] = texture2D(gcolor, uv) + texture2D(gdepth, uv) + texture2D(gnormal, uv) + texture2D(composite, uv) + texture2D(gaux1, uv) + texture2D(gaux4, uv)\n\
                + texture2D(gdepthtex, uv) + texture2D(shadow, uv) + texture2D(shadowcolor, uv) + texture2D(dhDepthTex, uv) + texture2D(noisetex, uv) + texture2D(somethingElse, uv); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(
        out.fs(),
        &[
            "texture(colortex0, uv) + texture(colortex1, uv) + texture(colortex2, uv) + texture(colortex3, uv) + texture(colortex4, uv) + texture(colortex7, uv)",
            "texture(depthtex0, uv) + texture(shadowtex0, uv) + texture(shadowcolor0, uv) + texture(dhDepthTex0, uv) + texture(noisetex, uv)",
        ],
    );
    let b = &out.pack.bindings;
    assert_eq!(b.get("colortex7").unwrap().resource, ResourceRef::ColorTex(7));
    assert_eq!(b.get("shadowtex0").unwrap().resource, ResourceRef::ShadowTex(0));
    assert_eq!(b.get("dhDepthTex0").unwrap().resource, ResourceRef::DhDepthTex(0));
    assert!(b.entries.iter().all(|e| e.set != 1 || matches!(e.kind, sb_core::model::ResourceKind::Sampler { .. })));
    bindings_match_the_table(&out);
    let used: Vec<&str> = out.prog.resources_used.iter().map(|(n, _)| n.as_str()).collect();
    assert!(used.contains(&"colortex4") && used.contains(&"depthtex0"), "{used:?}");
}

#[test]
fn watershadow_changes_the_shadow_alias() {
    let fs = "#version 130\nuniform sampler2D shadow;\nuniform sampler2D watershadow;\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = texture2D(shadow, uv) + texture2D(watershadow, uv); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["texture(shadowtex1, uv) + texture(shadowtex0, uv)"]);
}

#[test]
fn custom_textures_and_explicit_renames() {
    let fs = "#version 130\nuniform sampler2D lutTex;\nuniform sampler3D volumeTex;\nuniform sampler2D renamedTex;\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = texture2D(lutTex, uv) + texture3D(volumeTex, vec3(uv, 0.5)) + texture2D(renamedTex, uv); }\n";
    let rc = ResourceContext::new(ProgramClass::Fullscreen)
        .with_custom_texture(sb_uniforms::CUSTOM_STAGE, "lutTex")
        .with_raw_texture("composite", "volumeTex", "3d");
    let out = T::fullscreen()
        .resources(rc)
        .with(|o| {
            o.custom_texture_renames.insert("renamedTex".into(), "colortex9".into());
        })
        .other(ShaderStage::Fragment, "#version 130\nuniform sampler2D colortex9;\nvoid main() { gl_FragData[0] = texture2D(colortex9, vec2(0.0)); }\n", ProgramClass::Fullscreen)
        .vs(VS)
        .fs(fs)
        .run();
    let names: Vec<&str> = out.prog.resources_used.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"colortex9"), "{names:?}");
    contains_all(out.fs(), &["texture(colortex9, uv)"]);
    contains_none(out.fs(), &["renamedTex"]);
    assert!(out.pack.bindings.entries.iter().any(|e| matches!(&e.resource, ResourceRef::CustomTexture(id) if id.contains("lutTex"))));
    assert!(out.prog.requires_raw_vulkan, "3D textures need raw Vulkan");
    bindings_match_the_table(&out);
}

#[test]
fn storage_images_keep_formats_and_access() {
    let cs = "#version 430\nlayout(local_size_x = 8, local_size_y = 8) in;\nlayout(rgba16f) uniform image2D colorimg2;\nuniform image2D colorimg3;\nlayout(r32ui) uniform uimage2D counters;\nuniform image2D colorimg5;\n\
              void main() { ivec2 p = ivec2(gl_GlobalInvocationID.xy);\n  vec4 c = imageLoad(colorimg2, p) + imageLoad(colorimg5, p);\n  imageStore(colorimg3, p, c);\n  imageAtomicAdd(counters, ivec2(0), 1u);\n}\n";
    let rc = ResourceContext::new(ProgramClass::Compute).with_custom_image("counters", None);
    let out = T::fullscreen()
        .resources(rc.clone())
        .with(|o| {
            o.program_class = ProgramClass::Compute;
            o.image_formats.insert("colorimg5".into(), "rgba8".into());
        })
        .stage(ShaderStage::Compute, cs)
        .run();
    let c = out.glsl(ShaderStage::Compute);
    contains_all(c, &["rgba16f) uniform image2D colorimg2;", "writeonly uniform image2D colorimg3;", "r32ui) uniform uimage2D counters;", "rgba8) uniform image2D colorimg5;"]);
    assert!(out.prog.requires_raw_vulkan);
    for d in &out.refl(ShaderStage::Compute).descriptors {
        if matches!(d.kind, DescriptorKind::StorageImage { .. }) {
            assert_eq!(d.set, 2, "{}", d.name);
        }
    }
    bindings_match_the_table(&out);
    // A read image without a format: read-without-format, or a warning when the device
    // cannot do that.
    let cs = "#version 430\nlayout(local_size_x = 1) in;\nuniform image2D colorimg1;\nvoid main() { imageStore(colorimg1, ivec2(0), imageLoad(colorimg1, ivec2(1))); }\n";
    let out = T::fullscreen().with(|o| o.program_class = ProgramClass::Compute).stage(ShaderStage::Compute, cs).run();
    contains_all(out.glsl(ShaderStage::Compute), &["#extension GL_EXT_shader_image_load_formatted : enable"]);
    let out = T::fullscreen()
        .with(|o| {
            o.program_class = ProgramClass::Compute;
            o.storage_image_read_without_format = false;
        })
        .stage(ShaderStage::Compute, cs)
        .run();
    assert!(out.has_diag("xf.image-format"));
    // An image passed to a pack function may be read there: it is not made `writeonly`
    // (a `writeonly` argument cannot be passed to a parameter without it).
    let cs = "#version 430\nlayout(local_size_x = 8, local_size_y = 8) in;\nuniform image2D colorimg0;\n\
              vec4 load(image2D img, ivec2 p) { return imageLoad(img, p); }\n\
              void main() { ivec2 p = ivec2(gl_GlobalInvocationID.xy); imageStore(colorimg0, p, load(colorimg0, p) * 2.0); }\n";
    let out = T::fullscreen().with(|o| o.program_class = ProgramClass::Compute).stage(ShaderStage::Compute, cs).run();
    contains_all(out.glsl(ShaderStage::Compute), &["#extension GL_EXT_shader_image_load_formatted : enable", ") uniform image2D colorimg0;"]);
    contains_none(out.glsl(ShaderStage::Compute), &["writeonly"]);
}

#[test]
fn shader_storage_and_uniform_blocks() {
    let cs = "#version 430\nlayout(local_size_x = 1) in;\nlayout(std430, binding = 2) buffer Data { vec4 values[]; } data;\nlayout(binding = 5) buffer Counts { uint count; };\nuniform Settings { vec4 tint; } settings;\n\
              void main() { data.values[gl_GlobalInvocationID.x] = settings.tint * float(count); atomicAdd(count, 1u); }\n";
    let out = T::fullscreen().with(|o| o.program_class = ProgramClass::Compute).stage(ShaderStage::Compute, cs).run();
    let c = out.glsl(ShaderStage::Compute);
    contains_all(c, &["layout(set = 2, binding = 2) layout(std430) buffer Data", "layout(std430, set = 2, binding = 5) buffer Counts", "layout(std140, set = 0, binding = 2) uniform Settings"]);
    assert!(out.prog.requires_raw_vulkan);
    assert_eq!(out.pack.bindings.entries.iter().filter(|e| matches!(e.resource, ResourceRef::Ssbo(2) | ResourceRef::Ssbo(5))).count(), 2);
    bindings_match_the_table(&out);
    // An SSBO without a binding cannot be bound.
    let cs = "#version 430\nlayout(local_size_x = 1) in;\nbuffer Data { vec4 v; };\nvoid main() { v = vec4(1.0); }\n";
    let errors = T::fullscreen().with(|o| o.program_class = ProgramClass::Compute).stage(ShaderStage::Compute, cs).errors();
    assert!(errors.iter().any(|e| e == "xf.unbound-resource"), "{errors:?}");
}

#[test]
fn requires_raw_vulkan_only_for_features_renderpearl_lacks() {
    let fs = "#version 130\nuniform sampler2D colortex0;\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = texture2D(colortex0, uv); }\n";
    assert!(!T::fullscreen().vs(VS).fs(fs).run().prog.requires_raw_vulkan);
    let fs1 = "#version 130\nuniform sampler1D colortex0;\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = texture1D(colortex0, uv.x); }\n";
    assert!(T::fullscreen().vs(VS).fs(fs1).run().prog.requires_raw_vulkan);
    // Interfaces Mojang's pipeline builder rejects even after flattening (64-bit
    // varyings, arrayed interface blocks), decided alike for both targets.
    let vs = "#version 400\nflat out dvec2 d;\nvoid main() { gl_Position = ftransform(); d = dvec2(1.0); }\n";
    let fs = "#version 400\nflat in dvec2 d;\nout vec4 c;\nvoid main() { c = vec4(vec2(d), 0.0, 1.0); }\n";
    for target in [OutputTarget::Vulkan, OutputTarget::Renderpearl] {
        let out = T::fullscreen().vs(vs).fs(fs).with(|o| o.target = target).run();
        assert!(out.prog.requires_raw_vulkan && out.has_diag("xf.renderpearl-interface"), "{target:?}");
    }
    let vs = "#version 400\nout V { vec2 a; } v[2];\nvoid main() { gl_Position = ftransform(); v[0].a = vec2(1.0); v[1].a = vec2(2.0); }\n";
    let fs = "#version 400\nin V { vec2 a; } v[2];\nout vec4 c;\nvoid main() { c = vec4(v[0].a, v[1].a); }\n";
    assert!(T::fullscreen().vs(vs).fs(fs).run().prog.requires_raw_vulkan);
    // Blocks and struct varyings are flattened instead.
    let vs = "#version 400\nstruct S { vec2 a; };\nout S s;\nout V { vec2 b; } v;\nvoid main() { gl_Position = ftransform(); s.a = vec2(1.0); v.b = s.a; }\n";
    let fs = "#version 400\nstruct S { vec2 a; };\nin S s;\nin V { vec2 b; } v;\nout vec4 c;\nvoid main() { c = vec4(s.a, v.b); }\n";
    assert!(!T::fullscreen().vs(vs).fs(fs).run().prog.requires_raw_vulkan);
}

#[test]
fn extensions_are_kept_dropped_or_reported() {
    let fs = "#version 130\n#extension GL_ARB_shader_texture_lod : require\n#extension GL_EXT_gpu_shader4 : enable\n#extension GL_KHR_shader_subgroup_basic : enable\n#extension GL_NV_unknown_thing : enable\n\
              varying vec2 uv;\nvoid main() { gl_FragData[0] = vec4(uv, float(subgroupElect()), 1.0); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["#extension GL_KHR_shader_subgroup_basic : enable"]);
    contains_none(out.fs(), &["GL_ARB_shader_texture_lod", "GL_EXT_gpu_shader4", "GL_NV_unknown_thing"]);
    let warnings: Vec<&str> = out.prog.diagnostics.iter().filter(|d| d.code == "xf.extension").map(|d| d.message.as_str()).collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("GL_NV_unknown_thing"));
}

#[test]
fn darkness_factor_and_other_injected_builtins_need_no_declaration() {
    // Iris injects some uniforms; packs reference them without declaring them.
    let fs = "#version 130\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = vec4(uv, darknessFactor, 1.0); }\n";
    let out = T::fullscreen().vs(VS).fs(fs).run();
    contains_all(out.fs(), &["float darknessFactor;"]);
}
