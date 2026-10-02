//! Reserved-word escaping.
//!
//! Lenient (NVIDIA) drivers accept a number of GLSL reserved words as ordinary
//! identifiers. Strict compilers (glslang, Mesa) and our parser (glsl-lang)
//! reject them, and translated shaders are compiled as `#version 450` for
//! Vulkan, where several more words are keywords. Such identifiers are renamed
//! to `sb_kw_<name>` in the preprocessed output.
//!
//! The lists were checked against `glslangValidator` 15.1 (identifier use in a
//! `#version 450` Vulkan shader is an error for every word below) and
//! glsl-lang 0.8 (the `reserved_words_glsl_lang` test asserts which of them
//! glsl-lang rejects: all except the Vulkan-only `sampler`/`samplerShadow`).
//! `packed` is deliberately *not* escaped: both glslang (450) and glsl-lang
//! accept it as an identifier, and it is a valid `layout(packed)` qualifier.
//! Identifiers inside `layout(...)` are never escaped.

use crate::ExtensionDirective;

/// Prefix of escaped identifiers.
pub const ESCAPE_PREFIX: &str = "sb_kw_";

/// Words that are reserved (or Vulkan GLSL keywords) at every version and are
/// never valid as a pack's own keyword usage.
pub const ALWAYS_RESERVED: &[&str] = &[
    "input",
    "output",
    "filter",
    "common",
    "partition",
    "active",
    "superp",
    "namespace",
    "using",
    "cast",
    "sizeof",
    "external",
    "interface",
    "template",
    "this",
    "class",
    "union",
    "enum",
    "typedef",
    "goto",
    "inline",
    "noinline",
    "public",
    "static",
    "extern",
    "asm",
    "unsigned",
    "fvec2",
    "fvec3",
    "fvec4",
    "hvec2",
    "hvec3",
    "hvec4",
    "half",
    "fixed",
    "long",
    "short",
    "resource",
    // Vulkan GLSL (GL_KHR_vulkan_glsl) type keywords that are not legacy built-in
    // function names: glslang rejects them as identifiers when targeting Vulkan
    // (spectrum uses `sampler` as a parameter name).
    "sampler",
    "samplerShadow",
];

/// Keywords introduced in GLSL 4.00: escaped when the source version is below
/// 400 and the extension that introduces the keyword is not enabled.
pub const RESERVED_BELOW_400: &[(&str, &[&str])] = &[
    (
        "sample",
        &[
            "GL_ARB_gpu_shader5",
            "GL_OES_shader_multisample_interpolation",
        ],
    ),
    ("subroutine", &["GL_ARB_shader_subroutine"]),
    (
        "patch",
        &[
            "GL_ARB_tessellation_shader",
            "GL_EXT_tessellation_shader",
            "GL_OES_tessellation_shader",
        ],
    ),
    (
        "precise",
        &[
            "GL_ARB_gpu_shader5",
            "GL_EXT_gpu_shader5",
            "GL_OES_gpu_shader5",
        ],
    ),
];

/// Keywords introduced in GLSL 4.30: escaped when the version is below 430 and
/// neither `GL_ARB_shader_storage_buffer_object` nor `GL_ARB_compute_shader` is enabled.
pub const RESERVED_BELOW_430: &[&str] = &["buffer", "shared"];

const SSBO_EXTENSIONS: &[&str] = &[
    "GL_ARB_shader_storage_buffer_object",
    "GL_ARB_compute_shader",
];

/// Classification of an escape candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReservedClass {
    Always,
    Below400(usize),
    Below430,
}

/// Every candidate word with its class.
pub(crate) fn candidates() -> impl Iterator<Item = (&'static str, ReservedClass)> {
    ALWAYS_RESERVED
        .iter()
        .map(|w| (*w, ReservedClass::Always))
        .chain(
            RESERVED_BELOW_400
                .iter()
                .enumerate()
                .map(|(i, (w, _))| (*w, ReservedClass::Below400(i))),
        )
        .chain(
            RESERVED_BELOW_430
                .iter()
                .map(|w| (*w, ReservedClass::Below430)),
        )
}

fn enabled(exts: &[ExtensionDirective], names: &[&str]) -> bool {
    exts.iter()
        .any(|e| e.behavior != "disable" && names.contains(&e.name.as_str()))
}

/// Whether a word of the given class must be escaped for this version and extension set.
pub(crate) fn must_escape(class: ReservedClass, version: u32, exts: &[ExtensionDirective]) -> bool {
    match class {
        ReservedClass::Always => true,
        ReservedClass::Below400(i) => {
            version < 400 && !enabled(exts, RESERVED_BELOW_400.get(i).map_or(&[], |e| e.1))
        }
        ReservedClass::Below430 => version < 430 && !enabled(exts, SSBO_EXTENSIONS),
    }
}

/// Whether `word` would be escaped in a shader with the given version and extensions.
///
/// ```
/// use sb_preprocess::would_escape;
/// assert!(would_escape("input", 460, &[]));
/// assert!(would_escape("sample", 330, &[]));
/// assert!(!would_escape("sample", 400, &[]));
/// assert!(!would_escape("shared", 430, &[]));
/// assert!(!would_escape("color", 120, &[]));
/// ```
pub fn would_escape(word: &str, version: u32, extensions: &[ExtensionDirective]) -> bool {
    candidates().any(|(w, c)| w == word && must_escape(c, version, extensions))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext(name: &str, behavior: &str) -> ExtensionDirective {
        ExtensionDirective {
            name: name.into(),
            behavior: behavior.into(),
        }
    }

    #[test]
    fn version_gates() {
        for w in ["sample", "subroutine", "patch", "precise"] {
            assert!(would_escape(w, 330, &[]), "{w}");
            assert!(would_escape(w, 120, &[]), "{w}");
            assert!(!would_escape(w, 400, &[]), "{w}");
            assert!(!would_escape(w, 460, &[]), "{w}");
        }
        for w in ["buffer", "shared"] {
            assert!(would_escape(w, 420, &[]), "{w}");
            assert!(!would_escape(w, 430, &[]), "{w}");
        }
        for w in ALWAYS_RESERVED {
            assert!(would_escape(w, 460, &[]), "{w}");
            assert!(would_escape(w, 110, &[]), "{w}");
        }
    }

    #[test]
    fn extension_gates() {
        assert!(!would_escape(
            "shared",
            330,
            &[ext("GL_ARB_compute_shader", "enable")]
        ));
        assert!(!would_escape(
            "buffer",
            330,
            &[ext("GL_ARB_shader_storage_buffer_object", "require")]
        ));
        assert!(would_escape(
            "buffer",
            330,
            &[ext("GL_ARB_shader_storage_buffer_object", "disable")]
        ));
        assert!(!would_escape(
            "precise",
            330,
            &[ext("GL_ARB_gpu_shader5", "enable")]
        ));
        assert!(!would_escape(
            "patch",
            330,
            &[ext("GL_ARB_tessellation_shader", "warn")]
        ));
        assert!(would_escape(
            "subroutine",
            330,
            &[ext("GL_ARB_gpu_shader5", "enable")]
        ));
    }

    #[test]
    fn lists_are_disjoint() {
        let mut all: Vec<&str> = candidates().map(|(w, _)| w).collect();
        let n = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), n);
    }
}
