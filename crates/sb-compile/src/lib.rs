//! # sb-compile
//!
//! The SPIR-V back end of ShaderBridge (see `docs/ARCHITECTURE.md`, §3):
//!
//! * [`compile_glsl`] compiles the Vulkan GLSL 4.50 emitted by `sb-transform` to
//!   SPIR-V with glslang (Vulkan 1.2 / SPIR-V 1.5 by default, Vulkan GLSL rules,
//!   entry point `main`, debug names kept, no auto-mapping unless asked).
//!   Failures carry parsed glslang messages mapped back to the original pack
//!   files through the transformer's line map ([`CompileFailure::to_diagnostics`]).
//! * [`reflect`] extracts descriptors, block layouts, push constants, the stage
//!   interface (with interpolation qualifiers, interface blocks expanded per
//!   member) and the compute work-group size from a SPIR-V module, using spirq
//!   plus a word-level scan for what spirq does not expose.
//! * [`validate`] runs `spirv-val` (when installed) and [`disassemble`] runs
//!   `spirv-dis`.
//!
//! Every function is thread-safe; [`compile_glsl`] is designed to be called from
//! many rayon workers at once. Each compilation runs glslang on its own
//! short-lived thread with a 64 MiB stack (8 MiB if that cannot be reserved),
//! so callers with small stacks (JNI threads) are safe and glslang's per-thread
//! pool memory is released after every compile. Inputs that would exhaust that
//! stack, memory or time inside glslang (deep expressions, macro bombs,
//! exponentially nested structs) are rejected with an error instead of
//! aborting or hanging the process; [`reflect`] likewise bounds the types it
//! expands, and external tools are killed after 60 s.
//!
//! ## Limitations
//!
//! * glslang-sys is built without SPIRV-Tools: [`CompileOptions::optimize`] and
//!   [`validate`] / [`disassemble`] use the `spirv-opt` / `spirv-val` /
//!   `spirv-dis` executables when installed, and degrade gracefully otherwise.
//! * glslang usually stops at the first error of a shader, so a failure tends to
//!   carry one error.
//! * glslang's preprocessor has no macro-expansion limit; sources are expected
//!   to be preprocessed already (as `sb-preprocess` does). Sources that still
//!   contain `#define`s are screened first: macros that would expand to more
//!   than 4 million tokens or nest invocations more than 64 deep are rejected
//!   (the estimate does not model token pasting or the rescanning of a
//!   parameter that names a function-like macro).
//! * Arrays of interface blocks / structs (other than the implicit per-vertex
//!   array) are reflected as one `struct` variable.
//! * glslang line numbers are taken as lines of the GLSL handed in: a `#line N`
//!   directive without a source-string number in that GLSL would shift them.
//!
//! ```
//! use sb_compile::{compile_glsl, reflect, CompileOptions};
//! use sb_core::ShaderStage;
//!
//! let src = "#version 450
//! layout(set = 1, binding = 3) uniform sampler2D colortex0;
//! layout(location = 0) in vec2 texcoord;
//! layout(location = 0) out vec4 sb_FragData_0;
//! void main() { sb_FragData_0 = texture(colortex0, texcoord); }
//! ";
//! let spirv = compile_glsl(src, ShaderStage::Fragment, "composite.fsh", &CompileOptions::default(), None).unwrap();
//! let refl = reflect(&spirv).unwrap();
//! assert_eq!(refl.descriptor_by_name("colortex0").unwrap().binding, 3);
//! assert_eq!(refl.inputs[0].name, "texcoord");
//! ```

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

mod compile;
mod ffi;
mod guard;
pub mod log;
mod macros;
pub mod module;
mod reflect;
mod target;
mod tools;

pub use compile::{
    CompileFailure, CompileMessage, CompileOptions, CompileOutput, DIAG_CODE, compile_glsl, compile_glsl_detailed,
};
pub use reflect::{
    Access, BufferMember, Descriptor, DescriptorKind, ExecutionModeInfo, ImageDim, InterfaceVar, PushConstantBlock,
    Reflection, reflect,
};
pub use target::{SpirvTarget, VulkanTarget};
pub use tools::{ValidationResult, disassemble, find_tool, validate, validate_with};
