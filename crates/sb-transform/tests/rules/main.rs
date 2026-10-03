//! Rule-by-rule tests of the transformer (ARCHITECTURE §4–§8, spec-sb-transform §3–§4).
//!
//! Every test translates small GLSL programs through the public API (preprocess →
//! analyze → pack layout → transform), asserts on the emitted text, and compiles every
//! emitted stage with glslang for Vulkan 1.2 (`sb_compile`), validating the SPIR-V with
//! `spirv-val` when it is installed. Reflection is used to check interfaces, bindings,
//! push constants and outputs.

mod harness;

mod analysis;
mod builtins;
mod depth;
mod fixes;
mod interface;
mod outputs;
mod profiles;
mod resources;
mod shadow;
mod probe;
