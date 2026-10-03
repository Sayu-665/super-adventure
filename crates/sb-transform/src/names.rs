//! Name tables: GLSL builtins, legacy texture functions, Vulkan keywords, extensions.

/// Core-profile per-draw names (OptiFine 1.17+ core shaders) that the draw profile's
/// semantics implement.
pub const CORE_PROFILE_NAMES: &[(&str, &str)] = &[
    ("modelViewMatrix", "sb_ModelView"),
    ("modelViewMatrixInverse", "sb_ModelViewInverse"),
    ("projectionMatrix", "sb_Projection"),
    ("projectionMatrixInverse", "sb_ProjectionInverse"),
    ("normalMatrix", "sb_NormalMatrix"),
    ("textureMatrix", "sb_TextureMatrix"),
    ("chunkOffset", "sb_ChunkOffset"),
    ("modelOffset", "sb_ChunkOffset"),
];

/// Whether `name` is a core-profile per-draw name handled by the profile semantics.
pub fn is_core_profile_name(name: &str) -> bool {
    CORE_PROFILE_NAMES.iter().any(|(n, _)| *n == name)
}

/// The generated global replacing a core-profile name.
pub fn core_profile_replacement(name: &str) -> Option<&'static str> {
    CORE_PROFILE_NAMES.iter().find(|(n, _)| *n == name).map(|(_, r)| *r)
}

/// Every built-in function of GLSL 4.60 (plus Vulkan `subpassLoad`). A pack function
/// with one of these names may clash with a built-in signature and is renamed.
pub const BUILTIN_FUNCTIONS: &[&str] = &[
    "radians", "degrees", "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh",
    "pow", "exp", "log", "exp2", "log2", "sqrt", "inversesqrt", "abs", "sign", "floor", "trunc", "round", "roundEven",
    "ceil", "fract", "mod", "modf", "min", "max", "clamp", "mix", "step", "smoothstep", "isnan", "isinf",
    "floatBitsToInt", "floatBitsToUint", "intBitsToFloat", "uintBitsToFloat", "fma", "frexp", "ldexp",
    "packUnorm2x16", "packSnorm2x16", "packUnorm4x8", "packSnorm4x8", "unpackUnorm2x16", "unpackSnorm2x16",
    "unpackUnorm4x8", "unpackSnorm4x8", "packHalf2x16", "unpackHalf2x16", "packDouble2x32", "unpackDouble2x32",
    "length", "distance", "dot", "cross", "normalize", "faceforward", "reflect", "refract", "matrixCompMult",
    "outerProduct", "transpose", "determinant", "inverse", "lessThan", "lessThanEqual", "greaterThan",
    "greaterThanEqual", "equal", "notEqual", "any", "all", "not", "uaddCarry", "usubBorrow", "umulExtended",
    "imulExtended", "bitfieldExtract", "bitfieldInsert", "bitfieldReverse", "bitCount", "findLSB", "findMSB",
    "textureSize", "textureQueryLod", "textureQueryLevels", "textureSamples", "texture", "textureProj", "textureLod",
    "textureOffset", "texelFetch", "texelFetchOffset", "textureProjOffset", "textureLodOffset", "textureProjLod",
    "textureProjLodOffset", "textureGrad", "textureGradOffset", "textureProjGrad", "textureProjGradOffset",
    "textureGather", "textureGatherOffset", "textureGatherOffsets", "atomicCounterIncrement",
    "atomicCounterDecrement", "atomicCounter", "atomicCounterAdd", "atomicCounterSubtract", "atomicCounterMin",
    "atomicCounterMax", "atomicCounterAnd", "atomicCounterOr", "atomicCounterXor", "atomicCounterExchange",
    "atomicCounterCompSwap", "atomicAdd", "atomicMin", "atomicMax", "atomicAnd", "atomicOr", "atomicXor",
    "atomicExchange", "atomicCompSwap", "imageSize", "imageSamples", "imageLoad", "imageStore", "imageAtomicAdd",
    "imageAtomicMin", "imageAtomicMax", "imageAtomicAnd", "imageAtomicOr", "imageAtomicXor", "imageAtomicExchange",
    "imageAtomicCompSwap", "EmitStreamVertex", "EndStreamPrimitive", "EmitVertex", "EndPrimitive", "dFdx", "dFdy",
    "dFdxFine", "dFdyFine", "dFdxCoarse", "dFdyCoarse", "fwidth", "fwidthFine", "fwidthCoarse",
    "interpolateAtCentroid", "interpolateAtSample", "interpolateAtOffset", "noise1", "noise2", "noise3", "noise4",
    "barrier", "memoryBarrier", "memoryBarrierAtomicCounter", "memoryBarrierBuffer", "memoryBarrierShared",
    "memoryBarrierImage", "groupMemoryBarrier", "subpassLoad", "anyInvocation", "allInvocations",
    "allInvocationsEqual",
];

/// Built-in functions glslang declares for extensions (a pack function with the same
/// name clashes with them even when the extension is not enabled).
pub fn is_extension_builtin(name: &str) -> bool {
    matches!(name, "min3" | "max3" | "mid3" | "cubeFaceIndexAMD" | "cubeFaceCoordAMD" | "timeAMD")
}

/// The `GL_KHR_shader_subgroup_vote` equivalent of a GLSL 4.60 / `GL_ARB_shader_group_vote`
/// vote function (`anyInvocation[ARB]` -> `subgroupAny`, ...). glslang lowers the
/// former to `SPV_KHR_subgroup_vote`, which Vulkan accepts only with the
/// `VK_EXT_shader_subgroup_vote` extension; the subgroup forms are core Vulkan 1.1
/// operations. Both act on an implementation-defined group of invocations, so the
/// subgroup is a valid choice of group.
pub fn group_vote_function(name: &str) -> Option<&'static str> {
    match name.strip_suffix("ARB").unwrap_or(name) {
        "anyInvocation" => Some("subgroupAny"),
        "allInvocations" => Some("subgroupAll"),
        "allInvocationsEqual" => Some("subgroupAllEqual"),
        _ => None,
    }
}

/// Whether `name` is a GLSL 4.60 built-in function.
pub fn is_builtin_function(name: &str) -> bool {
    BUILTIN_FUNCTIONS.contains(&name)
}

/// Vulkan GLSL (`GL_KHR_vulkan_glsl`) type keywords that legacy code may use as
/// identifiers (glslang rejects them as names when targeting Vulkan).
pub fn is_vulkan_type_keyword(name: &str) -> bool {
    if matches!(
        name,
        "sampler" | "samplerShadow" | "subpassInput" | "subpassInputMS" | "isubpassInput" | "isubpassInputMS" | "usubpassInput" | "usubpassInputMS"
    ) {
        return true;
    }
    let rest = name.strip_prefix('i').or_else(|| name.strip_prefix('u')).unwrap_or(name);
    matches!(
        rest,
        "texture1D"
            | "texture1DArray"
            | "texture2D"
            | "texture2DArray"
            | "texture2DMS"
            | "texture2DMSArray"
            | "texture2DRect"
            | "texture3D"
            | "textureBuffer"
            | "textureCube"
            | "textureCubeArray"
    )
}

/// Identifiers glslang rejects at `#version 460` that older GLSL versions (or lenient
/// drivers) accept as names. The preprocessor escapes most of them; whatever remains
/// is escaped by the transformer.
pub fn is_reserved_at_460(name: &str) -> bool {
    matches!(
        name,
        "shared"
            | "filter"
            | "input"
            | "output"
            | "common"
            | "half"
            | "fixed"
            | "active"
            | "superp"
            | "volatile"
            | "noperspective"
            | "centroid"
            | "invariant"
            | "smooth"
            | "flat"
            | "sample"
            | "buffer"
            | "precise"
            | "patch"
            | "subroutine"
            | "coherent"
            | "restrict"
            | "readonly"
            | "writeonly"
            | "resource"
            | "partition"
    ) || is_vulkan_type_keyword(name)
}

/// A legacy sampling function and its core replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyTexture {
    /// Core function name (`texture`, `textureLod`, `texelFetch`, ...).
    pub core: &'static str,
    /// `shadow*` functions: the result is splatted to `vec4`.
    pub shadow: bool,
}

/// Map a legacy sampling function (`texture2D`, `texture2DLodEXT`, `shadow2DProj`,
/// `texelFetch2D`, `textureSize2D`, ...) to its core equivalent.
pub fn legacy_texture(name: &str) -> Option<LegacyTexture> {
    let base = name.strip_suffix("ARB").or_else(|| name.strip_suffix("EXT")).unwrap_or(name);
    const DIMS: &[&str] = &["1DArray", "2DArray", "2DRect", "Cube", "1D", "2D", "3D", "Buffer"];
    fn strip_dim(rest: &str) -> Option<&str> {
        DIMS.iter().find_map(|d| rest.strip_prefix(d))
    }
    if let Some(rest) = base.strip_prefix("texelFetch") {
        let suffix = strip_dim(rest)?;
        return match suffix {
            "" => Some(LegacyTexture { core: "texelFetch", shadow: false }),
            "Offset" => Some(LegacyTexture { core: "texelFetchOffset", shadow: false }),
            _ => None,
        };
    }
    if let Some(rest) = base.strip_prefix("textureSize") {
        return strip_dim(rest)?.is_empty().then_some(LegacyTexture { core: "textureSize", shadow: false });
    }
    let (rest, shadow) = match base.strip_prefix("texture") {
        Some(r) => (r, false),
        None => (base.strip_prefix("shadow")?, true),
    };
    let suffix = strip_dim(rest)?;
    if !shadow && rest.starts_with("Buffer") {
        return None;
    }
    let core = match suffix {
        "" => "texture",
        "Lod" => "textureLod",
        "Proj" => "textureProj",
        "ProjLod" => "textureProjLod",
        "Grad" => "textureGrad",
        "ProjGrad" => "textureProjGrad",
        "Offset" => "textureOffset",
        "LodOffset" => "textureLodOffset",
        "ProjOffset" => "textureProjOffset",
        "ProjLodOffset" => "textureProjLodOffset",
        "GradOffset" => "textureGradOffset",
        "ProjGradOffset" => "textureProjGradOffset",
        _ => return None,
    };
    Some(LegacyTexture { core, shadow })
}

/// Texture read functions whose first argument is the sampler (for depth-read
/// rewriting and sampler-use analysis).
pub fn is_texture_read(name: &str) -> bool {
    matches!(
        name,
        "texture"
            | "textureProj"
            | "textureLod"
            | "textureOffset"
            | "texelFetch"
            | "texelFetchOffset"
            | "textureProjOffset"
            | "textureLodOffset"
            | "textureProjLod"
            | "textureProjLodOffset"
            | "textureGrad"
            | "textureGradOffset"
            | "textureProjGrad"
            | "textureProjGradOffset"
            | "textureGather"
            | "textureGatherOffset"
            | "textureGatherOffsets"
    )
}

/// Extensions whose functionality is core in GLSL 4.50/4.60 (dropped silently).
pub const CORE_EXTENSIONS: &[&str] = &[
    "GL_ARB_shader_image_load_store",
    "GL_EXT_shader_image_load_store",
    "GL_ARB_explicit_attrib_location",
    "GL_ARB_explicit_uniform_location",
    "GL_ARB_shading_language_packing",
    "GL_ARB_texture_gather",
    "GL_ARB_shader_texture_lod",
    "GL_EXT_shader_texture_lod",
    "GL_ARB_texture_query_levels",
    "GL_ARB_texture_query_lod",
    "GL_ARB_shader_storage_buffer_object",
    "GL_EXT_gpu_shader4",
    "GL_ARB_gpu_shader5",
    "GL_EXT_gpu_shader5",
    "GL_ARB_shader_bit_encoding",
    "GL_ARB_separate_shader_objects",
    "GL_ARB_compute_shader",
    "GL_ARB_shading_language_420pack",
    "GL_ARB_enhanced_layouts",
    "GL_ARB_texture_rectangle",
    "GL_ARB_uniform_buffer_object",
    "GL_ARB_conservative_depth",
    "GL_ARB_derivative_control",
    "GL_ARB_texture_cube_map_array",
    "GL_ARB_shader_atomic_counters",
    "GL_ARB_shader_atomic_counter_ops",
    "GL_ARB_sample_shading",
    "GL_ARB_gpu_shader_fp64",
    "GL_ARB_arrays_of_arrays",
    "GL_ARB_texture_multisample",
    "GL_ARB_shader_precision",
    "GL_ARB_tessellation_shader",
    "GL_ARB_geometry_shader4",
    "GL_EXT_geometry_shader4",
    "GL_EXT_geometry_shader",
    "GL_ARB_fragment_coord_conventions",
    "GL_ARB_texture_buffer_object",
    "GL_EXT_texture_buffer_object",
    "GL_EXT_texture_array",
    "GL_ARB_draw_instanced",
    "GL_EXT_draw_instanced",
    "GL_ARB_shader_group_vote",
    "GL_ARB_shading_language_include",
    "GL_GOOGLE_include_directive",
    "GL_GOOGLE_cpp_style_line_directive",
    "GL_ARB_fragment_layer_viewport",
    "GL_ARB_compute_variable_group_size",
    "GL_ARB_texture_storage",
    "GL_ARB_vertex_attrib_64bit",
    "GL_ARB_viewport_array",
    "GL_ARB_cull_distance",
    "GL_ARB_shader_image_size",
    "GL_ARB_shader_texture_image_samples",
    "GL_ARB_shader_subroutine",
    "GL_ARB_get_program_binary",
    "GL_ARB_texture_non_power_of_two",
    "GL_EXT_texture_integer",
    "GL_ARB_shading_language_100",
    "GL_ARB_fragment_shader",
    "GL_ARB_vertex_shader",
    "GL_ARB_draw_buffers",
    "GL_ATI_draw_buffers",
    "GL_EXT_frag_depth",
];

/// Extensions glslang supports when targeting Vulkan that add functionality beyond
/// GLSL 4.60; kept (with `require` downgraded to `enable`).
pub fn is_kept_extension(name: &str) -> bool {
    name.starts_with("GL_KHR_shader_subgroup_")
        || name.starts_with("GL_EXT_shader_explicit_arithmetic_types")
        || name.starts_with("GL_EXT_shader_subgroup_extended_types")
        || matches!(
            name,
            "GL_KHR_memory_scope_semantics"
                | "GL_EXT_shader_16bit_storage"
                | "GL_EXT_shader_8bit_storage"
                | "GL_EXT_shader_image_load_formatted"
                | "GL_EXT_nonuniform_qualifier"
                | "GL_EXT_samplerless_texture_functions"
                | "GL_EXT_control_flow_attributes"
                | "GL_EXT_shader_atomic_float"
                | "GL_EXT_shader_atomic_float2"
                | "GL_EXT_shader_atomic_int64"
                | "GL_ARB_shader_ballot"
                | "GL_ARB_shader_draw_parameters"
                | "GL_ARB_gpu_shader_int64"
                | "GL_ARB_shader_clock"
                | "GL_EXT_shader_realtime_clock"
                | "GL_EXT_scalar_block_layout"
                | "GL_EXT_demote_to_helper_invocation"
                | "GL_EXT_debug_printf"
                | "GL_EXT_fragment_shader_barycentric"
                | "GL_ARB_shader_viewport_layer_array"
                | "GL_ARB_post_depth_coverage"
                | "GL_EXT_post_depth_coverage"
                | "GL_ARB_shader_stencil_export"
                | "GL_AMD_gpu_shader_half_float"
                | "GL_AMD_gpu_shader_int16"
                | "GL_EXT_shader_image_int64"
                | "GL_EXT_maximal_reconvergence"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_vote_functions_map_to_subgroup_vote() {
        assert_eq!(group_vote_function("anyInvocation"), Some("subgroupAny"));
        assert_eq!(group_vote_function("anyInvocationARB"), Some("subgroupAny"));
        assert_eq!(group_vote_function("allInvocations"), Some("subgroupAll"));
        assert_eq!(group_vote_function("allInvocationsEqualARB"), Some("subgroupAllEqual"));
        assert_eq!(group_vote_function("subgroupAny"), None);
        assert_eq!(group_vote_function("ARB"), None);
    }

    #[test]
    fn legacy_texture_names() {
        let c = |n: &str| legacy_texture(n).map(|l| (l.core, l.shadow));
        assert_eq!(c("texture2D"), Some(("texture", false)));
        assert_eq!(c("texture2DLod"), Some(("textureLod", false)));
        assert_eq!(c("texture2DLodEXT"), Some(("textureLod", false)));
        assert_eq!(c("texture2DGradARB"), Some(("textureGrad", false)));
        assert_eq!(c("textureCubeLod"), Some(("textureLod", false)));
        assert_eq!(c("texture2DRectProj"), Some(("textureProj", false)));
        assert_eq!(c("texture3DProjLod"), Some(("textureProjLod", false)));
        assert_eq!(c("shadow2D"), Some(("texture", true)));
        assert_eq!(c("shadow2DProjLod"), Some(("textureProjLod", true)));
        assert_eq!(c("shadow2DGradARB"), Some(("textureGrad", true)));
        assert_eq!(c("texelFetch2D"), Some(("texelFetch", false)));
        assert_eq!(c("texelFetch2DOffset"), Some(("texelFetchOffset", false)));
        assert_eq!(c("textureSize2D"), Some(("textureSize", false)));
        assert_eq!(c("texture"), None);
        assert_eq!(c("textureLod"), None);
        assert_eq!(c("texture2DFoo"), None);
        assert_eq!(c("textureGather"), None);
        assert_eq!(c("shadowMap"), None);
    }

    #[test]
    fn keyword_tables() {
        assert!(is_vulkan_type_keyword("texture2D"));
        assert!(is_vulkan_type_keyword("utexture3D"));
        assert!(is_vulkan_type_keyword("sampler"));
        assert!(!is_vulkan_type_keyword("texture"));
        assert!(is_reserved_at_460("flat"));
        assert!(!is_reserved_at_460("color"));
        assert!(is_builtin_function("fma") && !is_builtin_function("saturate"));
        assert!(is_kept_extension("GL_KHR_shader_subgroup_ballot"));
        assert!(!is_kept_extension("GL_ARB_gpu_shader5"));
        assert_eq!(core_profile_replacement("modelOffset"), Some("sb_ChunkOffset"));
    }
}
