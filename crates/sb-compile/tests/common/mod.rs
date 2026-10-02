//! Helpers shared by the integration tests.
#![allow(dead_code)]

use sb_compile::{CompileOptions, ValidationResult, VulkanTarget, compile_glsl, validate};
use sb_core::ShaderStage;

/// Compile with default options, panic with the parsed failure on error, and
/// check the module with `spirv-val` when it is installed.
pub fn compile_ok(src: &str, stage: ShaderStage) -> Vec<u32> {
    compile_with(src, stage, &CompileOptions::default())
}

/// [`compile_ok`] with explicit options.
pub fn compile_with(src: &str, stage: ShaderStage, opts: &CompileOptions) -> Vec<u32> {
    let spirv = compile_glsl(src, stage, "test", opts, None).unwrap_or_else(|e| panic!("{e}\n{}", e.log));
    assert_valid(&spirv, opts.vulkan);
    spirv
}

/// Assert `spirv-val` accepts the module (skipped when the tool is missing).
pub fn assert_valid(spirv: &[u32], vulkan: VulkanTarget) {
    match validate(spirv, vulkan) {
        ValidationResult::Valid | ValidationResult::Skipped(_) => {}
        ValidationResult::Invalid(msg) => panic!("spirv-val rejected the module:\n{msg}"),
    }
}

/// Whether `spirv-val` is installed.
pub fn have_spirv_val() -> bool {
    sb_compile::find_tool("spirv-val").is_some()
}
