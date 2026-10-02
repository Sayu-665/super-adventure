//! `spirv-val` validation, `spirv-dis` disassembly and module utilities on real
//! glslang output.

mod common;

use common::{compile_ok, have_spirv_val};
use sb_compile::{ValidationResult, VulkanTarget, disassemble, module, reflect, validate, validate_with};
use sb_core::ShaderStage;
use std::path::Path;

const FRAG: &str = r#"#version 450
layout(set = 1, binding = 2) uniform sampler2D colortex2;
layout(location = 0) in vec2 texcoord;
layout(location = 0) out vec4 sb_FragData_0;
void main() { sb_FragData_0 = texture(colortex2, texcoord); }
"#;

/// Remove every instruction with `opcode` from a module.
fn without_opcode(words: &[u32], opcode: u32) -> Vec<u32> {
    let mut out = words[..module::HEADER_WORDS].to_vec();
    let mut i = module::HEADER_WORDS;
    while i < words.len() {
        let n = (words[i] >> 16) as usize;
        if words[i] & 0xffff != opcode {
            out.extend_from_slice(&words[i..i + n]);
        }
        i += n;
    }
    out
}

#[test]
fn valid_module_is_valid() {
    let spirv = compile_ok(FRAG, ShaderStage::Fragment);
    let r = validate(&spirv, VulkanTarget::Vulkan1_2);
    if have_spirv_val() {
        assert_eq!(r, ValidationResult::Valid);
    } else {
        assert!(r.is_skipped());
    }
}

#[test]
fn broken_modules_are_invalid() {
    if !have_spirv_val() {
        eprintln!("spirv-val not installed; skipping");
        return;
    }
    let spirv = compile_ok(FRAG, ShaderStage::Fragment);
    // No OpMemoryModel (opcode 14).
    let broken = without_opcode(&spirv, 14);
    match validate(&broken, VulkanTarget::Vulkan1_2) {
        ValidationResult::Invalid(msg) => assert!(!msg.is_empty()),
        other => panic!("expected Invalid, got {other:?}"),
    }
    // Truncated stream.
    let truncated = &spirv[..spirv.len() - 1];
    assert!(validate(truncated, VulkanTarget::Vulkan1_2).is_invalid());
    // SPIR-V 1.5 is not consumable by Vulkan 1.0.
    match validate(&spirv, VulkanTarget::Vulkan1_0) {
        ValidationResult::Invalid(msg) => assert!(msg.to_lowercase().contains("version"), "{msg}"),
        other => panic!("expected Invalid, got {other:?}"),
    }
    // Not SPIR-V at all: rejected before running the tool.
    assert!(validate(&[1, 2, 3, 4, 5, 6], VulkanTarget::Vulkan1_2).is_invalid());
}

#[test]
fn missing_validator_is_skipped() {
    let spirv = compile_ok(FRAG, ShaderStage::Fragment);
    let r = validate_with(Path::new("/definitely/not/spirv-val"), &spirv, VulkanTarget::Vulkan1_2);
    assert!(r.is_skipped(), "{r:?}");
}

#[test]
fn disassembly() {
    let spirv = compile_ok(FRAG, ShaderStage::Fragment);
    match disassemble(&spirv) {
        Some(text) => {
            assert!(text.contains("OpEntryPoint Fragment %main \"main\""), "{text}");
            assert!(text.contains("OpDecorate %colortex2 Binding 2"), "{text}");
            assert!(text.contains("OpDecorate %colortex2 DescriptorSet 1"), "{text}");
        }
        None => assert!(sb_compile::find_tool("spirv-dis").is_none()),
    }
}

#[test]
fn module_utilities_on_real_output() {
    let spirv = compile_ok(FRAG, ShaderStage::Fragment);
    let header = module::header(&spirv).unwrap();
    assert_eq!(header.version_pair(), (1, 5));
    assert!(header.bound > 0);
    let bytes = module::words_to_bytes(&spirv);
    assert_eq!(module::words_from_bytes(&bytes).unwrap(), spirv);

    let stripped = module::strip_debug_info(&spirv).unwrap();
    assert!(stripped.len() < spirv.len());
    assert!(!validate(&stripped, VulkanTarget::Vulkan1_2).is_invalid());
    let refl = reflect(&stripped).unwrap();
    assert_eq!(refl.descriptors[0].name, "");
    assert_eq!(refl.descriptors[0].binding, 2);
    // Idempotent.
    assert_eq!(module::strip_debug_info(&stripped).unwrap(), stripped);
}

#[test]
fn reflect_rejects_corrupted_real_modules_without_panicking() {
    // A fragment shader with a sampler, and a module exercising the paths that
    // walk types themselves: interface blocks, struct varyings, storage images,
    // uniform/storage blocks with nested structs and arrays.
    let rich = r#"#version 450
struct Light { vec3 pos; float r; mat3 m; };
layout(set = 0, binding = 0, std140) uniform U { Light lights[4]; vec4 tail; };
layout(set = 0, binding = 1, std430) buffer S { uint count; Light dyn[]; } ssbo;
layout(set = 2, binding = 0, r32ui) uniform uimage2D counters;
layout(location = 0) in vec3 pos;
layout(location = 0) out Varyings { vec2 uv; flat int id; mat2 rot; } vs;
layout(location = 5) out Light lightOut;
void main() {
    vs.uv = pos.xy; vs.id = int(ssbo.count); vs.rot = mat2(lights[1].r);
    lightOut = ssbo.dyn[0];
    imageAtomicAdd(counters, ivec2(0), 1u);
    gl_Position = vec4(pos + lights[2].pos, tail.x);
}
"#;
    for spirv in [compile_ok(FRAG, ShaderStage::Fragment), compile_ok(rich, ShaderStage::Vertex)] {
        assert!(reflect(&spirv).is_ok());
        // Flip every word in turn (after the header) and make sure reflection never panics.
        for i in module::HEADER_WORDS..spirv.len() {
            for value in [0u32, 1, 2, u32::MAX, 0x0001_0000, spirv[i] ^ 0x8000_0000, spirv[i].wrapping_add(1)] {
                let mut m = spirv.clone();
                m[i] = value;
                let _ = reflect(&m);
            }
        }
        // Truncations.
        for len in 0..spirv.len() {
            let _ = reflect(&spirv[..len]);
        }
    }
}

/// Crafted modules whose types spirq would expand exponentially or clone
/// recursively are rejected quickly instead of exhausting memory or the stack.
#[test]
fn reflect_bounds_type_expansion() {
    fn op(words: &mut Vec<u32>, opcode: u32, operands: &[u32]) {
        words.push(((operands.len() as u32 + 1) << 16) | opcode);
        words.extend_from_slice(operands);
    }
    // Fragment entry point, then `levels` struct (or array) types, each using the
    // previous one twice, and a variable of the last.
    let build = |levels: u32, dag: bool| {
        let mut w = vec![module::MAGIC, 0x0001_0500, 0, 0, 0];
        op(&mut w, 17, &[1]); // OpCapability Shader
        op(&mut w, 14, &[0, 1]); // OpMemoryModel Logical GLSL450
        op(&mut w, 15, &[4, 1, u32::from_le_bytes(*b"main"), 0]); // OpEntryPoint Fragment %1 "main"
        op(&mut w, 16, &[1, 7]); // OpExecutionMode %1 OriginUpperLeft
        op(&mut w, 19, &[2]); // OpTypeVoid
        op(&mut w, 33, &[3, 2]); // OpTypeFunction
        op(&mut w, 22, &[4, 32]); // OpTypeFloat 32
        op(&mut w, 21, &[5, 32, 0]); // OpTypeInt 32 0
        op(&mut w, 43, &[5, 6, 2]); // OpConstant 2
        let mut prev = 4;
        for id in 10..10 + levels {
            if dag {
                op(&mut w, 30, &[id, prev, prev]); // OpTypeStruct
            } else {
                op(&mut w, 28, &[id, prev, 6]); // OpTypeArray
            }
            prev = id;
        }
        let ptr = 10 + levels;
        op(&mut w, 32, &[ptr, 6, prev]); // OpTypePointer Private
        op(&mut w, 59, &[ptr, ptr + 1, 6]); // OpVariable Private
        op(&mut w, 54, &[2, 1, 0, 3]); // OpFunction
        op(&mut w, 248, &[ptr + 2]); // OpLabel
        op(&mut w, 253, &[]); // OpReturn
        op(&mut w, 56, &[]); // OpFunctionEnd
        w[3] = ptr + 3;
        w
    };
    let start = std::time::Instant::now();
    assert!(reflect(&build(10, true)).is_ok());
    assert!(reflect(&build(40, true)).unwrap_err().contains("too large"));
    assert!(reflect(&build(30, false)).is_ok());
    assert!(reflect(&build(100_000, false)).unwrap_err().contains("deeper"));
    assert!(start.elapsed() < std::time::Duration::from_secs(10), "{:?}", start.elapsed());
}
