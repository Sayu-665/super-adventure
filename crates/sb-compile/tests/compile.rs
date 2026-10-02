//! GLSL -> SPIR-V compilation: every stage, error/warning parsing, line maps,
//! Vulkan GLSL rules, options and thread safety.

mod common;

use common::{assert_valid, compile_ok, compile_with};
use pretty_assertions::assert_eq;
use rayon::prelude::*;
use sb_compile::{
    CompileOptions, DIAG_CODE, SpirvTarget, VulkanTarget, compile_glsl, compile_glsl_detailed, disassemble, module,
    reflect,
};
use sb_core::{Severity, ShaderStage, SourceLocation};

const VERT: &str = r#"#version 450
layout(location = 0) in vec3 vPosition;
layout(location = 1) in vec2 vUV;
layout(location = 0) out vec2 texcoord;
layout(set = 0, binding = 0, std140) uniform sb_Frame { mat4 gbufferModelView; mat4 gbufferProjection; };
void main() {
    texcoord = vUV;
    gl_Position = gbufferProjection * gbufferModelView * vec4(vPosition, 1.0);
}
"#;

const FRAG: &str = r#"#version 450
layout(set = 1, binding = 0) uniform sampler2D gtexture;
layout(location = 0) in vec2 texcoord;
layout(location = 0) out vec4 sb_FragData_0;
void main() {
    sb_FragData_0 = texture(gtexture, texcoord);
}
"#;

const GEOM: &str = r#"#version 450
layout(triangles) in;
layout(triangle_strip, max_vertices = 3) out;
layout(location = 0) in vec2 texcoord[];
layout(location = 0) out vec2 gTexcoord;
void main() {
    for (int i = 0; i < 3; i++) {
        gl_Position = gl_in[i].gl_Position;
        gTexcoord = texcoord[i];
        EmitVertex();
    }
    EndPrimitive();
}
"#;

const TESC: &str = r#"#version 450
layout(vertices = 3) out;
layout(location = 0) in vec2 texcoord[];
layout(location = 0) out vec2 tcTexcoord[];
void main() {
    tcTexcoord[gl_InvocationID] = texcoord[gl_InvocationID];
    gl_out[gl_InvocationID].gl_Position = gl_in[gl_InvocationID].gl_Position;
    if (gl_InvocationID == 0) {
        gl_TessLevelOuter[0] = 2.0; gl_TessLevelOuter[1] = 2.0; gl_TessLevelOuter[2] = 2.0;
        gl_TessLevelInner[0] = 2.0;
    }
}
"#;

const TESE: &str = r#"#version 450
layout(triangles, equal_spacing, ccw) in;
layout(location = 0) in vec2 tcTexcoord[];
layout(location = 0) out vec2 texcoord;
void main() {
    texcoord = gl_TessCoord.x * tcTexcoord[0] + gl_TessCoord.y * tcTexcoord[1] + gl_TessCoord.z * tcTexcoord[2];
    gl_Position = gl_TessCoord.x * gl_in[0].gl_Position + gl_TessCoord.y * gl_in[1].gl_Position
                + gl_TessCoord.z * gl_in[2].gl_Position;
}
"#;

const COMP: &str = r#"#version 450
layout(local_size_x = 16, local_size_y = 8, local_size_z = 1) in;
layout(set = 2, binding = 0, rgba16f) uniform image2D colorimg0;
void main() {
    ivec2 p = ivec2(gl_GlobalInvocationID.xy);
    imageStore(colorimg0, p, imageLoad(colorimg0, p) * 0.5);
}
"#;

#[test]
fn compiles_every_stage() {
    for (src, stage) in [
        (VERT, ShaderStage::Vertex),
        (FRAG, ShaderStage::Fragment),
        (GEOM, ShaderStage::Geometry),
        (TESC, ShaderStage::TessControl),
        (TESE, ShaderStage::TessEval),
        (COMP, ShaderStage::Compute),
    ] {
        let spirv = compile_ok(src, stage);
        assert_eq!(spirv[0], module::MAGIC);
        let refl = reflect(&spirv).unwrap();
        assert_eq!(refl.stage, stage, "{stage}");
        assert_eq!(refl.entry_point, "main");
        assert_eq!(refl.spirv_version, (1, 5));
    }
}

#[test]
fn wrong_stage_is_a_compile_error() {
    // A vertex shader compiled as compute: gl_Position does not exist there.
    let err = compile_glsl(VERT, ShaderStage::Compute, "x.csh", &CompileOptions::default(), None).unwrap_err();
    assert!(err.errors.iter().any(|e| e.line == Some(2) && e.message.contains("compute")), "{err}");
    let err = compile_glsl(COMP, ShaderStage::Fragment, "x.fsh", &CompileOptions::default(), None).unwrap_err();
    assert!(err.errors.iter().any(|e| e.line == Some(2)), "{err}");
}

#[test]
fn error_lines_are_parsed_and_mapped_through_the_line_map() {
    let src = "#version 450\nlayout(location = 0) out vec4 c;\nvoid main() {\n    c = vec4(undefinedThing);\n}\n";
    let line_map: Vec<Option<SourceLocation>> = vec![
        None,
        Some(SourceLocation::new("world0/composite.fsh", 10)),
        Some(SourceLocation::new("world0/composite.fsh", 11)),
        Some(SourceLocation::new("lib/util.glsl", 42)),
        None,
    ];
    let err = compile_glsl(src, ShaderStage::Fragment, "world0/composite.fsh", &CompileOptions::default(), Some(&line_map))
        .unwrap_err();
    assert_eq!(err.name, "world0/composite.fsh");
    assert_eq!(err.stage, ShaderStage::Fragment);
    assert!(err.log.contains("ERROR: 0:4:"), "{}", err.log);
    assert_eq!(err.errors.len(), 1, "{:#?}", err.errors);
    let e = &err.errors[0];
    assert_eq!(e.line, Some(4));
    assert_eq!(e.column, Some(14));
    assert_eq!(e.message, "'undefinedThing' : undeclared identifier");
    assert_eq!(e.original, Some(SourceLocation::new("lib/util.glsl", 42)));

    let diags = err.to_diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, DIAG_CODE);
    assert_eq!(diags[0].severity, Severity::Error);
    assert_eq!(diags[0].location, Some(SourceLocation::new("lib/util.glsl", 42)));
    assert_eq!(diags[0].stage, Some(ShaderStage::Fragment));
    assert_eq!(diags[0].message, "'undefinedThing' : undeclared identifier");
}

#[test]
fn unmapped_lines_keep_the_glsl_line_in_the_diagnostic() {
    let src = "#version 450\nvoid main() {\n    int i = 1.5;\n}\n";
    let err = compile_glsl(src, ShaderStage::Fragment, "f", &CompileOptions::default(), Some(&[])).unwrap_err();
    assert_eq!(err.errors[0].line, Some(3));
    assert_eq!(err.errors[0].original, None);
    let d = &err.to_diagnostics()[0];
    assert!(d.message.contains("cannot convert"), "{}", d.message);
    assert!(d.message.ends_with("(translated GLSL line 3)"), "{}", d.message);
    assert_eq!(d.location, None);
}

#[test]
fn syntax_errors_drop_the_empty_token() {
    let src = "#version 450\nlayout(location = 0) out vec4 c;\nvoid main() { c = vec4(1.0)\n}\n";
    let err = compile_glsl(src, ShaderStage::Fragment, "f", &CompileOptions::default(), None).unwrap_err();
    assert_eq!(err.errors[0].line, Some(4));
    assert!(err.errors[0].message.starts_with("syntax error"), "{}", err.errors[0].message);
}

#[test]
fn missing_main_is_a_link_error() {
    let src = "#version 450\nlayout(location = 0) out vec4 c;\nvoid notMain() { c = vec4(1.0); }\n";
    let err = compile_glsl(src, ShaderStage::Fragment, "f", &CompileOptions::default(), None).unwrap_err();
    assert!(err.errors.iter().any(|e| e.message.contains("Missing entry point") && e.line.is_none()), "{err}");
}

#[test]
fn warnings_of_successful_compiles_are_reported() {
    let src = "#version 450\n#extension GL_SB_does_not_exist : enable\nlayout(location = 0) out vec4 c;\nvoid main() { c = vec4(1.0); }\n";
    let line_map = vec![None, Some(SourceLocation::new("final.fsh", 3))];
    let out = compile_glsl_detailed(src, ShaderStage::Fragment, "f", &CompileOptions::default(), Some(&line_map)).unwrap();
    assert_valid(&out.spirv, VulkanTarget::Vulkan1_2);
    let w = out.warnings.iter().find(|w| w.message.contains("GL_SB_does_not_exist")).expect("warning");
    assert_eq!(w.line, Some(2));
    assert_eq!(w.original, Some(SourceLocation::new("final.fsh", 3)));
    let diags = out.warning_diagnostics(ShaderStage::Fragment);
    assert!(diags.iter().all(|d| d.severity == Severity::Warning && d.code == DIAG_CODE));

    let quiet = CompileOptions { suppress_warnings: true, ..Default::default() };
    let out = compile_glsl_detailed(src, ShaderStage::Fragment, "f", &quiet, None).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

/// Vulkan GLSL has no default uniform block: every non-opaque uniform must live in
/// a block with an explicit layout. Packs declare dozens of loose uniforms
/// (`uniform float frameTimeCounter;`), which is why sb-transform packs them into
/// the pack-global `sb_Frame` std140 block (ARCHITECTURE §5.1).
#[test]
fn loose_uniforms_are_rejected_for_vulkan() {
    let src = "#version 450\nuniform float frameTimeCounter;\nlayout(location = 0) out vec4 c;\nvoid main() { c = vec4(frameTimeCounter); }\n";
    let err = compile_glsl(src, ShaderStage::Fragment, "f", &CompileOptions::default(), None).unwrap_err();
    let e = &err.errors[0];
    assert_eq!(e.line, Some(2));
    assert!(e.message.contains("non-opaque uniforms outside a block"), "{}", e.message);
    assert!(e.message.contains("Vulkan"), "{}", e.message);

    // The same value inside a block compiles.
    let ok = "#version 450\nlayout(set = 0, binding = 0, std140) uniform sb_Frame { float frameTimeCounter; };\n\
              layout(location = 0) out vec4 c;\nvoid main() { c = vec4(frameTimeCounter); }\n";
    compile_ok(ok, ShaderStage::Fragment);
}

/// Without auto-mapping, user varyings need explicit locations and opaque
/// uniforms explicit bindings: the translator must assign them (phase B / §5.2).
#[test]
fn explicit_locations_and_bindings_are_required_by_default() {
    let no_location = "#version 450\nin vec2 uv;\nlayout(location = 0) out vec4 c;\nvoid main() { c = vec4(uv, 0.0, 1.0); }\n";
    let err = compile_glsl(no_location, ShaderStage::Fragment, "f", &CompileOptions::default(), None).unwrap_err();
    assert!(err.errors[0].message.contains("location"), "{err}");
    assert_eq!(err.errors[0].line, Some(2));

    let no_binding = "#version 450\nuniform sampler2D tex;\nlayout(location = 0) out vec4 c;\nvoid main() { c = texture(tex, vec2(0.5)); }\n";
    let err = compile_glsl(no_binding, ShaderStage::Fragment, "f", &CompileOptions::default(), None).unwrap_err();
    assert!(err.errors[0].message.contains("binding"), "{err}");
}

/// Mojang's renderpearl GLSL has no set/binding/location decorations; with the
/// opt-in auto-mapping flags glslang assigns them (as shaderc does for Mojang).
#[test]
fn auto_mapping_assigns_bindings_and_locations() {
    let src = r#"#version 450
uniform sampler2D Sampler0;
uniform sampler2D Sampler2;
layout(std140) uniform DynamicTransforms { mat4 ModelViewMat; vec4 ColorModulator; };
in vec2 texCoord0;
in vec4 vertexColor;
out vec4 fragColor;
void main() {
    fragColor = texture(Sampler0, texCoord0) * texture(Sampler2, texCoord0) * vertexColor * ColorModulator;
}
"#;
    let spirv = compile_with(src, ShaderStage::Fragment, &CompileOptions::auto_mapped());
    let refl = reflect(&spirv).unwrap();
    let mut bindings: Vec<u32> = refl.descriptors.iter().map(|d| d.binding).collect();
    bindings.sort_unstable();
    bindings.dedup();
    assert_eq!(bindings.len(), 3, "{:#?}", refl.descriptors);
    assert!(refl.descriptor_by_name("DynamicTransforms").is_some());
    let mut locs: Vec<u32> = refl.inputs.iter().map(|v| v.location).collect();
    locs.sort_unstable();
    assert_eq!(locs, vec![0, 1]);
    assert_eq!(refl.output_by_name("fragColor").unwrap().location, 0);

    // Each flag alone only relaxes its own rule.
    let only_bindings = CompileOptions { auto_map_bindings: true, ..Default::default() };
    assert!(compile_glsl(src, ShaderStage::Fragment, "f", &only_bindings, None).is_err());
}

#[test]
fn legacy_versions_and_compatibility_profile_are_rejected() {
    let v120 = "#version 120\nvoid main() { gl_FragColor = vec4(1.0); }\n";
    let err = compile_glsl(v120, ShaderStage::Fragment, "f", &CompileOptions::default(), None).unwrap_err();
    assert!(err.errors[0].message.contains("version 140"), "{err}");

    // glslang only *logs* this error and would still emit SPIR-V; sb-compile fails it.
    let compat = "#version 400 compatibility\nvoid main() { gl_FragColor = vec4(1.0); }\n";
    let err = compile_glsl(compat, ShaderStage::Fragment, "f", &CompileOptions::default(), None).unwrap_err();
    assert_eq!(err.errors.len(), 1, "{err}");
    assert!(err.errors[0].message.contains("compatibility profile"), "{err}");
}

#[test]
fn includes_are_never_resolved() {
    // glslang's C interface would read files from disk without include callbacks.
    let path = std::env::temp_dir().join(format!("sb-compile-include-{}.glsl", std::process::id()));
    std::fs::write(&path, "layout(location = 0) out vec4 c;\n").unwrap();
    let src = format!(
        "#version 450\n#extension GL_GOOGLE_include_directive : enable\n#include \"{}\"\nvoid main() {{ c = vec4(1.0); }}\n",
        path.display()
    );
    let result = compile_glsl(&src, ShaderStage::Fragment, "f", &CompileOptions::default(), None);
    let _ = std::fs::remove_file(&path);
    let err = result.unwrap_err();
    assert!(err.errors[0].message.contains("include"), "{err}");
    assert_eq!(err.errors[0].line, Some(3));
}

#[test]
fn hostile_inputs_never_panic() {
    let cases = [
        String::new(),
        "\0".to_string(),
        "#version".to_string(),
        "#version 45é0\nvoid main(){}".to_string(),
        "#version 450\n".to_string(),
        "#version 450\nvoid main() {".to_string(),
        format!("#version 450\nvoid main() {{ float x = {}1.0{}; }}", "(".repeat(300), ")".repeat(300)),
        "#version 450\n#define A(x) A(x)\nvoid main() { A(1); }".to_string(),
        "#version 450\nvoid main() { float x = 1.0 / 0.0; }".to_string(),
        "#version 450\n\u{feff}\u{1F600} void main() {}".to_string(),
        "#version 450\n#line 10 7\nvoid main() { undefinedX; }".to_string(),
        "#version 99999\nvoid main(){}".to_string(),
        "#version 450 es\nvoid main(){}".to_string(),
        "#version 310 es\nvoid main(){}".to_string(),
        // Semantic oddities glslang must reject (or accept) without asserting.
        "#version 450\nfloat f(); float f(){ return f(); }\nvoid main(){ f(); }".to_string(),
        "#version 450\nfloat a[1000000000];\nvoid main(){ a[5] = 1.0; }".to_string(),
        "#version 450\nlayout(binding=0) uniform atomic_uint a;\nvoid main(){ atomicCounterIncrement(a); }".to_string(),
        "#version 450\nsubroutine float S(); subroutine uniform S s;\nvoid main(){}".to_string(),
        "#version 450\nstruct S { S s; };\nvoid main(){}".to_string(),
        "#version 450\nvoid main(){ int x = 1; switch(x){ case 1: break; case 1: break; } }".to_string(),
        "#version 450\nlayout(local_size_x = 0) in;\nvoid main(){}".to_string(),
        "#version 450\nlayout(location=0) out vec4 c;\nvoid main(){ c = vec4(float(0xFFFFFFFFFFFFFFFFFFFF)); }".to_string(),
        format!("#version 450\nfloat {};\nvoid main(){{}}", "a".repeat(1 << 20)),
        "#version 450\n#extension GL_EXT_buffer_reference : require\nlayout(buffer_reference) buffer B { B next; float v; };\n\
         layout(push_constant) uniform P { B b; };\nlayout(location=0) out vec4 c;\nvoid main(){ c = vec4(b.next.next.v); }"
            .to_string(),
    ];
    for src in &cases {
        for stage in [ShaderStage::Vertex, ShaderStage::Fragment, ShaderStage::Compute] {
            match compile_glsl(src, stage, "hostile", &CompileOptions::default(), None) {
                Ok(spirv) => {
                    reflect(&spirv).unwrap();
                }
                Err(e) => assert!(!e.errors.is_empty()),
            }
        }
    }
}

#[test]
fn line_directive_with_source_number_keeps_location_text() {
    let src = "#version 450\n#line 10 7\nvoid main() { undefinedX; }\n";
    let err = compile_glsl(src, ShaderStage::Fragment, "f", &CompileOptions::default(), None).unwrap_err();
    let e = &err.errors[0];
    assert_eq!(e.line, None, "{e:?}");
    assert!(e.message.starts_with("7:10"), "{}", e.message);
}

#[test]
fn spirv_targets() {
    for (vulkan, spirv) in [
        (VulkanTarget::Vulkan1_0, SpirvTarget::Spirv1_0),
        (VulkanTarget::Vulkan1_1, SpirvTarget::Spirv1_3),
        (VulkanTarget::Vulkan1_1, SpirvTarget::Spirv1_4),
        (VulkanTarget::Vulkan1_2, SpirvTarget::Spirv1_5),
        (VulkanTarget::Vulkan1_3, SpirvTarget::Spirv1_6),
    ] {
        let opts = CompileOptions { vulkan, spirv, ..Default::default() };
        let words = compile_with(FRAG, ShaderStage::Fragment, &opts);
        assert_eq!(words[1], spirv.header_word());
        let refl = reflect(&words).unwrap();
        assert_eq!(refl.spirv_version, spirv.version());
        assert_eq!(refl.descriptor_by_name("gtexture").unwrap().binding, 0);
    }
}

#[test]
fn debug_names_are_kept_by_default_and_strippable() {
    let named = compile_ok(FRAG, ShaderStage::Fragment);
    let refl = reflect(&named).unwrap();
    assert_eq!(refl.descriptors[0].name, "gtexture");
    assert_eq!(refl.inputs[0].name, "texcoord");
    assert_eq!(refl.outputs[0].name, "sb_FragData_0");

    let opts = CompileOptions { keep_debug_names: false, ..Default::default() };
    let stripped = compile_with(FRAG, ShaderStage::Fragment, &opts);
    assert!(stripped.len() < named.len());
    let refl = reflect(&stripped).unwrap();
    assert_eq!(refl.descriptors[0].name, "");
    assert_eq!((refl.descriptors[0].set, refl.descriptors[0].binding), (1, 0));
    assert_eq!(refl.inputs[0].name, "");
}

#[test]
fn debug_info_records_the_source_name() {
    let opts = CompileOptions { debug_info: true, ..Default::default() };
    let spirv = compile_glsl(FRAG, ShaderStage::Fragment, "world0/gbuffers_basic.fsh", &opts, None).unwrap();
    assert_valid(&spirv, VulkanTarget::Vulkan1_2);
    let bytes = module::words_to_bytes(&spirv);
    let needle = b"world0/gbuffers_basic.fsh";
    assert!(bytes.windows(needle.len()).any(|w| w == needle), "OpString with the file name missing");
    if let Some(dis) = disassemble(&spirv) {
        assert!(dis.contains("OpLine"), "{dis}");
    }
}

#[test]
fn optimize_preserves_the_interface() {
    let opts = CompileOptions { optimize: true, ..Default::default() };
    let out = compile_glsl_detailed(VERT, ShaderStage::Vertex, "v", &opts, None).unwrap();
    assert_valid(&out.spirv, VulkanTarget::Vulkan1_2);
    let refl = reflect(&out.spirv).unwrap();
    if sb_compile::find_tool("spirv-opt").is_none() {
        assert!(out.warnings.iter().any(|w| w.message.contains("unoptimized")));
    }
    assert_eq!(refl.inputs.len(), 2);
    assert_eq!(refl.outputs.len(), 1);
    assert_eq!(refl.descriptor(0, 0).unwrap().block_type_name.as_deref(), Some("sb_Frame"));
}

#[test]
fn compilation_is_deterministic() {
    let a = compile_ok(COMP, ShaderStage::Compute);
    let b = compile_ok(COMP, ShaderStage::Compute);
    assert_eq!(a, b);
}

/// `compile_glsl` must be callable from many threads at once (sb-pipeline compiles
/// every program stage on rayon workers).
#[test]
fn compiles_64_shaders_in_parallel() {
    let jobs: Vec<(usize, String, ShaderStage)> = (0..64)
        .map(|i| match i % 4 {
            0 => (i, FRAG.replace("texcoord);", &format!("texcoord) * {i}.0;")), ShaderStage::Fragment),
            1 => (i, VERT.replace("1.0)", &format!("{i}.0)")), ShaderStage::Vertex),
            2 => (i, COMP.replace("0.5", &format!("{i}.5")), ShaderStage::Compute),
            _ => (i, GEOM.replace("i < 3", &format!("i < {}", 1 + i % 3)), ShaderStage::Geometry),
        })
        .collect();
    let results: Vec<(usize, Vec<u32>)> = jobs
        .par_iter()
        .map(|(i, src, stage)| {
            let spirv = compile_glsl(src, *stage, &format!("job{i}"), &CompileOptions::default(), None)
                .unwrap_or_else(|e| panic!("job {i}: {e}"));
            let refl = reflect(&spirv).unwrap();
            assert_eq!(refl.stage, *stage);
            (*i, spirv)
        })
        .collect();
    assert_eq!(results.len(), 64);
    // Same results as a sequential compile.
    for (i, spirv) in results.iter().step_by(7) {
        let (_, src, stage) = &jobs[*i];
        let again = compile_glsl(src, *stage, "seq", &CompileOptions::default(), None).unwrap();
        assert_eq!(&again, spirv, "job {i}");
    }
    // Failures in parallel are reported, not mixed up between threads.
    let failures: Vec<_> = (0..32)
        .into_par_iter()
        .map(|i| {
            let src = format!("#version 450\nvoid main() {{\n{}\n    undefined_{i};\n}}\n", "\n".repeat(i));
            let e = compile_glsl(&src, ShaderStage::Fragment, "bad", &CompileOptions::default(), None).unwrap_err();
            (i, e)
        })
        .collect();
    for (i, e) in failures {
        assert_eq!(e.errors[0].line, Some(4 + i as u32), "{e}");
        assert!(e.errors[0].message.contains(&format!("undefined_{i}")));
    }
}

/// glslang recurses once per expression-tree level in native code; sb-compile
/// runs it on a large dedicated stack, so deep (but sane) expressions compile
/// even when the caller's own stack is tiny (e.g. a JNI thread)...
#[test]
fn deep_expressions_compile_on_small_caller_stacks() {
    let terms = " + y".repeat(10_000);
    let src = format!(
        "#version 450\nlayout(location = 0) out vec4 c;\nvoid main() {{ float y = c.x; float x = y{terms}; c = vec4(x); }}\n"
    );
    let spirv = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || compile_glsl(&src, ShaderStage::Fragment, "deep", &CompileOptions::default(), None))
        .unwrap()
        .join()
        .unwrap()
        .unwrap();
    assert_valid(&spirv, VulkanTarget::Vulkan1_2);
}

/// Regression: the complexity guard counted decimal points and argument commas,
/// so a large constant lookup table (flat in glslang's tree) was rejected as
/// "too complex". Tables of constructors and initializer lists must compile.
#[test]
fn large_constant_tables_are_not_too_complex() {
    let floats = (0..20_000).map(|i| format!("{}.5", i % 97)).collect::<Vec<_>>().join(", ");
    let vecs = (0..12_000).map(|i| format!("vec2({i}.0, -1.5e-3)")).collect::<Vec<_>>().join(", ");
    let src = format!(
        "#version 450\n\
         const float table[20000] = float[20000]({floats});\n\
         const vec2 offsets[12000] = {{ {vecs} }};\n\
         layout(location = 0) out vec4 c;\n\
         void main() {{ int i = int(gl_FragCoord.x); c = vec4(table[i], offsets[i], 1.0); }}\n"
    );
    let spirv = compile_glsl(&src, ShaderStage::Fragment, "lut", &CompileOptions::default(), None)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_valid(&spirv, VulkanTarget::Vulkan1_2);
}

/// Regression: glslang's preprocessor has no limits, so macro bombs used to run
/// it out of memory (2^30 tokens) or into quadratic time (`M(M(M(...)))` 4,000
/// deep took 3.5 GB). They are now rejected up front, with the line of the use.
#[test]
fn macro_bombs_are_rejected_before_glslang_runs() {
    let start = std::time::Instant::now();
    let mut src = String::from("#version 450\nlayout(location = 0) out vec4 c;\n#define M0 c.x\n");
    for i in 1..=30 {
        src.push_str(&format!("#define M{i} M{p} + M{p}\n", p = i - 1));
    }
    src.push_str("void main() {\n    c = vec4(M30);\n}\n");
    let line_map: Vec<Option<SourceLocation>> =
        (1..=40).map(|l| Some(SourceLocation::new("lib/bomb.glsl", l))).collect();
    let err = compile_glsl(&src, ShaderStage::Fragment, "bomb", &CompileOptions::default(), Some(&line_map)).unwrap_err();
    assert!(err.errors[0].message.contains("macro expansion too large"), "{err}");
    assert_eq!(err.errors[0].line, Some(35));
    assert_eq!(err.errors[0].original, Some(SourceLocation::new("lib/bomb.glsl", 35)));

    let nested = format!(
        "#version 450\nlayout(location = 0) out vec4 c;\n#define M(x) x\nvoid main() {{ c = vec4({}1.0{}); }}\n",
        "M(".repeat(4000),
        ")".repeat(4000)
    );
    let err = compile_glsl(&nested, ShaderStage::Fragment, "nested", &CompileOptions::default(), None).unwrap_err();
    assert!(err.errors[0].message.contains("nested too deeply"), "{err}");

    let doubling = format!(
        "#version 450\nlayout(location = 0) out vec4 c;\n#define D(x) (x + x)\nvoid main() {{ c = vec4({}c.x{}); }}\n",
        "D(".repeat(40),
        ")".repeat(40)
    );
    let err = compile_glsl(&doubling, ShaderStage::Fragment, "doubling", &CompileOptions::default(), None).unwrap_err();
    assert!(err.errors[0].message.contains("too large"), "{err}");
    assert!(start.elapsed() < std::time::Duration::from_secs(20), "{:?}", start.elapsed());

    // Ordinary macros still work.
    let ok = "#version 450\nlayout(location = 0) out vec4 c;\n#define SQ(x) ((x) * (x))\n#define GAMMA 2.2\n\
              void main() { c = vec4(pow(SQ(c.x), 1.0 / GAMMA)); }\n";
    compile_ok(ok, ShaderStage::Fragment);
}

/// Regression: glslang takes exponential time on struct types that reuse each
/// other (`struct S2 { S1 a; S1 b; }`): 24 levels took 7 s, 40 never finished.
/// Such types are rejected after preprocessing, with the line of the type.
#[test]
fn exponentially_nested_structs_are_rejected() {
    let start = std::time::Instant::now();
    let mut src = String::from("#version 450\nstruct S0 { float v; };\n");
    for i in 1..=40 {
        src.push_str(&format!("struct S{i} {{ S{p} a; S{p} b; }};\n", p = i - 1));
    }
    src.push_str("layout(location = 0) out vec4 c;\nvoid main() { S40 s; c = vec4(1.0); }\n");
    let err = compile_glsl(&src, ShaderStage::Fragment, "dag", &CompileOptions::default(), None).unwrap_err();
    assert!(err.errors[0].message.contains("'S16' : type too large to compile safely"), "{err}");
    assert_eq!(err.errors[0].line, Some(18));
    assert!(start.elapsed() < std::time::Duration::from_secs(10), "{:?}", start.elapsed());

    // Realistic nesting compiles.
    let ok = "#version 450\nstruct Light { vec3 pos; float radius; };\nstruct Scene { Light lights[16]; Light sun; };\n\
              layout(set = 0, binding = 0, std140) uniform U { Scene scene; };\nlayout(location = 0) out vec4 c;\n\
              void main() { c = vec4(scene.sun.pos + scene.lights[3].pos, scene.sun.radius); }\n";
    compile_ok(ok, ShaderStage::Fragment);
}

/// Chains just under the complexity limit compile on the dedicated stack: the
/// limit leaves a safety margin, it does not merely move the crash.
#[test]
fn chains_at_the_complexity_limit_compile() {
    let sum = " + y".repeat(24_500);
    let commas = ", y".repeat(12_200);
    for (label, expr) in [("sum", format!("y{sum}")), ("commas", format!("(y{commas})"))] {
        let src = format!(
            "#version 450\nlayout(location = 0) out vec4 c;\nvoid main() {{ float y = c.x; float x = {expr}; c = vec4(x); }}\n"
        );
        let spirv = compile_glsl(&src, ShaderStage::Fragment, label, &CompileOptions::default(), None)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_valid(&spirv, VulkanTarget::Vulkan1_2);
    }
}

/// ...and pathological ones are rejected with an error instead of overflowing
/// the stack and aborting the process, including when the depth only appears
/// after macro expansion.
#[test]
fn pathological_expressions_are_rejected_not_aborted() {
    let terms = " + y".repeat(60_000);
    let src = format!(
        "#version 450\nlayout(location = 0) out vec4 c;\nvoid main() {{\n float y = c.x;\n float x = y{terms};\n c = vec4(x);\n}}\n"
    );
    let err = compile_glsl(&src, ShaderStage::Fragment, "deep", &CompileOptions::default(), None).unwrap_err();
    assert!(err.errors[0].message.contains("too complex"), "{err}");
    assert_eq!(err.errors[0].line, Some(5));

    // Comma operators chain left-deep (unlike argument lists).
    let commas = ", y".repeat(60_000);
    let src = format!("#version 450\nlayout(location = 0) out vec4 c;\nvoid main() {{\n float y = c.x;\n float x = (y{commas});\n c = vec4(x);\n}}\n");
    let err = compile_glsl(&src, ShaderStage::Fragment, "commas", &CompileOptions::default(), None).unwrap_err();
    assert!(err.errors[0].message.contains("too complex"), "{err}");

    let mut src = String::from("#version 450\nlayout(location = 0) out vec4 c;\n#define M0 y\n");
    for i in 1..=17 {
        src.push_str(&format!("#define M{i} M{p} + M{p}\n", p = i - 1));
    }
    src.push_str("void main() { float y = c.x; float x = M17; c = vec4(x); }\n");
    let err = compile_glsl(&src, ShaderStage::Fragment, "macro", &CompileOptions::default(), None).unwrap_err();
    assert!(err.errors[0].message.contains("too complex"), "{err}");
}
