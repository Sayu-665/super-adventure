//! # sb-transform
//!
//! Translates preprocessed OptiFine/Iris shader-pack GLSL (any version, compatibility
//! or core profile, NVIDIA-lenient) into strict Vulkan GLSL 4.60 that glslang compiles
//! for Vulkan 1.2 (see `docs/ARCHITECTURE.md` §4–§8).
//!
//! * [`profiles`]: draw profiles, the data describing how a host feeds geometry.
//! * [`analyze`]: phase A, parse one stage and collect [`StageInfo`].
//! * [`transform_program`]: phases B and C, rewrite every stage of a program, link the
//!   stage interfaces and emit Vulkan GLSL with a line map back to the pack sources.
//!
//! Typical use (see the corpus test for a complete example):
//!
//! 1. [`analyze`] every stage of every program of a dimension;
//! 2. build the pack layout from [`StageInfo::uniform_decls`] plus each used profile's
//!    [`DrawProfile::referenced_builtins`], and the binding table from
//!    [`AnalyzedStage::resources`] (canonicalized with
//!    `sb_uniforms::canonicalize_with_kind`) plus [`register_profile_resources`];
//! 3. [`transform_program`] each program with its draw profile.
//!
//! Every generated identifier starts with `sb_`; pack identifiers with that prefix are
//! renamed to `sbu_` (the preprocessor's `sb_kw_` escapes are kept).

mod analyze;
mod ast;
mod compat;
mod consteval;
mod depth;
mod emit;
mod fixes;
mod link;
mod names;
mod pack;
mod parse;
mod print;
mod program;
pub mod profiles;
mod rewrite;
mod scope;
mod stack;
mod text;
mod transform;

pub use analyze::{
    AnalyzedStage, BlockInfo, IRIS_ATTRIBUTES, InterfaceVar, LooseUniform, OpaqueUniform, StageInfo, analyze,
};
pub use compat::register_profile_resources;
pub use pack::{PackBuilder, PackData};
pub use profiles::{
    DrawProfile, FULLSCREEN_PROFILE, ProfileBlock, ProfileGlobal, ProfileInput, ProfileSampler, Semantics,
    builtin_profiles, default_profile_for, parse_profile, profile,
};
pub use transform::{
    DEFAULT_VERTEX_SHADER, PackContext, TransformOptions, TransformedProgram, TransformedStage, transform_program,
};
