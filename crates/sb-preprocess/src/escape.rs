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
//!
//! ## Context-sensitive words
//!
//! The words of [`CONTEXTUAL_RESERVED`] (`flat`, `layout`, `switch`, `double`,
//! ...) are keywords from some GLSL version on, but lenient compilers accept
//! them as identifiers in older versions (NVIDIA all of them, glslang most),
//! while glsl-lang rejects them as identifiers at every version and the
//! translated `#version 450` output makes them keywords anyway. Unlike the
//! words above, packs also use them as keywords below that version
//! (`flat varying` under `GL_EXT_gpu_shader4`, `layout(location = 0)` under
//! `GL_ARB_explicit_attrib_location`, a leniently accepted `switch`), so the
//! version gate alone cannot tell the two uses apart. Each occurrence is
//! therefore classified from the nearest significant tokens before and after
//! it (whitespace, comments and directives skipped; after macro expansion) as
//! an identifier use, a keyword use or ambiguous (see [`KeywordRole`]), and a
//! word is escaped in a translation unit only when
//! * the source version is below [`ContextualWord::since`] and none of its
//!   [`ContextualWord::extensions`] is enabled, and
//! * at least one occurrence is certainly an identifier and none is certainly
//!   a keyword (a word lexes the same way everywhere in a unit, so a pack that
//!   declares `float case` cannot also contain a `case` label).
//!
//! All occurrences of the word are then escaped, ambiguous ones included
//! (`float case = 1.0; x = case - 1.0;`). Everything else is left alone: a
//! wrong escape would break valid code, a missed one only keeps the parse
//! error the identifier causes anyway.

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

/// How a [`ContextualWord`] appears when it is used as a keyword. This decides
/// which neighbouring tokens (whitespace and comments skipped) prove an
/// identifier use, prove a keyword use, or leave the occurrence ambiguous.
/// For every role, a preceding `.` (field selection) proves an identifier use;
/// evidence for both uses at once leaves the occurrence ambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeywordRole {
    /// Interpolation, memory or precision qualifier, or the `precision`
    /// statement keyword: always followed by another qualifier or a type name.
    /// Followed by an identifier: keyword; by a punctuator: identifier.
    /// Preceded by an operator other than `(` and `,`: identifier.
    Qualifier,
    /// `layout(...)`. Preceded by an operator other than `(` and `,`, or
    /// followed by a punctuator other than `(`: identifier (`layout(` itself
    /// is ambiguous: it could be a call of a function named `layout`).
    Layout,
    /// The `switch (...)` statement keyword. Preceded by an operator, or
    /// followed by a punctuator other than `(`: identifier.
    Switch,
    /// The `case <constant-expression>:` label. Followed by an identifier or a
    /// number: keyword. Preceded by an operator, or followed by a punctuator
    /// that cannot start an expression (anything but `(`, `+`, `-`, `~`, `!`
    /// and `:`): identifier.
    Case,
    /// The `default:` label. Followed by `:` (and not preceded by an operator
    /// such as the `?` of a conditional): keyword. Preceded by an operator, or
    /// followed by any other punctuator: identifier.
    Default,
    /// A type name (`double`). Followed by an identifier: keyword. Followed by
    /// `(` (constructor) or `[` (array type): ambiguous. Followed by `,`/`)`
    /// (unnamed parameter) or `;` (empty declaration): ambiguous, unless
    /// preceded by an operator other than `(` and `,` (after an operator a type
    /// name can only start a constructor): identifier. Any other punctuator:
    /// identifier.
    Type,
}

/// A word that is a keyword from GLSL version [`since`](Self::since) on (or
/// when one of [`extensions`](Self::extensions) is enabled), that lenient
/// compilers accept as an identifier before that, and that is escaped only
/// where it is used as an identifier (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextualWord {
    /// The word.
    pub word: &'static str,
    /// First desktop GLSL version in which the word is a keyword; identifier
    /// uses are escaped only in sources with a lower `#version`.
    pub since: u32,
    /// Extensions that make the word a keyword in older versions; while one
    /// of them is enabled (any behaviour but `disable`), the word is never escaped.
    pub extensions: &'static [&'static str],
    /// How the word appears as a keyword.
    pub role: KeywordRole,
}

const fn contextual(
    word: &'static str,
    since: u32,
    extensions: &'static [&'static str],
    role: KeywordRole,
) -> ContextualWord {
    ContextualWord {
        word,
        since,
        extensions,
        role,
    }
}

/// `GL_EXT_gpu_shader4` introduces the `flat`/`noperspective` (and NVIDIA's
/// `smooth`) interpolation qualifiers for GLSL 1.20.
const GPU_SHADER4: &[&str] = &["GL_EXT_gpu_shader4"];
const NOPERSPECTIVE_EXTENSIONS: &[&str] = &[
    "GL_EXT_gpu_shader4",
    "GL_NV_shader_noperspective_interpolation",
];
/// Extensions that introduce the memory qualifiers before GLSL 4.20 (glslang
/// only honours the ARB image extension; the others are kept for leniency).
const MEMORY_QUALIFIER_EXTENSIONS: &[&str] = &[
    "GL_ARB_shader_image_load_store",
    "GL_EXT_shader_image_load_store",
    "GL_ARB_shader_storage_buffer_object",
];
/// Extensions that introduce `layout` qualifiers before GLSL 1.40 (glslang
/// honours the first two).
const LAYOUT_EXTENSIONS: &[&str] = &[
    "GL_ARB_explicit_attrib_location",
    "GL_ARB_shading_language_420pack",
    "GL_ARB_uniform_buffer_object",
    "GL_ARB_separate_shader_objects",
    "GL_ARB_explicit_uniform_location",
    "GL_ARB_fragment_coord_conventions",
    "GL_ARB_conservative_depth",
    "GL_ARB_enhanced_layouts",
];
const FP64_EXTENSIONS: &[&str] = &["GL_ARB_gpu_shader_fp64", "GL_ARB_vertex_attrib_64bit"];

/// Context-sensitive reserved words (see the module documentation). The
/// versions are those at which glslang's scanner turns each word into a
/// keyword for desktop GLSL (glslang reports `volatile`, `switch`, `default`
/// and `double` as reserved words before that and treats `case` as a keyword
/// at every version, but NVIDIA accepts them as identifiers).
pub const CONTEXTUAL_RESERVED: &[ContextualWord] = &[
    contextual("smooth", 130, GPU_SHADER4, KeywordRole::Qualifier),
    contextual("flat", 130, GPU_SHADER4, KeywordRole::Qualifier),
    contextual(
        "noperspective",
        130,
        NOPERSPECTIVE_EXTENSIONS,
        KeywordRole::Qualifier,
    ),
    contextual(
        "coherent",
        420,
        MEMORY_QUALIFIER_EXTENSIONS,
        KeywordRole::Qualifier,
    ),
    contextual(
        "volatile",
        420,
        MEMORY_QUALIFIER_EXTENSIONS,
        KeywordRole::Qualifier,
    ),
    contextual(
        "restrict",
        420,
        MEMORY_QUALIFIER_EXTENSIONS,
        KeywordRole::Qualifier,
    ),
    contextual(
        "readonly",
        420,
        MEMORY_QUALIFIER_EXTENSIONS,
        KeywordRole::Qualifier,
    ),
    contextual(
        "writeonly",
        420,
        MEMORY_QUALIFIER_EXTENSIONS,
        KeywordRole::Qualifier,
    ),
    contextual("layout", 140, LAYOUT_EXTENSIONS, KeywordRole::Layout),
    contextual("precision", 130, &[], KeywordRole::Qualifier),
    contextual("lowp", 130, &[], KeywordRole::Qualifier),
    contextual("mediump", 130, &[], KeywordRole::Qualifier),
    contextual("highp", 130, &[], KeywordRole::Qualifier),
    contextual("switch", 130, &[], KeywordRole::Switch),
    contextual("case", 130, &[], KeywordRole::Case),
    contextual("default", 130, &[], KeywordRole::Default),
    contextual("double", 400, FP64_EXTENSIONS, KeywordRole::Type),
];

/// A significant token next to an escape candidate (whitespace and comments
/// are skipped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Neighbour<'a> {
    /// Start or end of the translation unit.
    Missing,
    /// An identifier or keyword.
    Ident,
    /// A preprocessing number.
    Number,
    /// A punctuator.
    Punct(&'a str),
    /// A string literal or a stray character.
    Other,
}

/// How one occurrence of a contextual word is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Usage {
    Identifier,
    Keyword,
    Ambiguous,
}

/// Punctuators after which an operand must follow, so a statement keyword
/// (`switch`, `case`, `default`) or a qualifier cannot. `(` and `,` are
/// excluded for roles that can start a parameter declaration.
fn expects_operand(p: &str) -> bool {
    matches!(
        p,
        "(" | "["
            | ","
            | "?"
            | "="
            | "+"
            | "-"
            | "*"
            | "/"
            | "%"
            | "<"
            | ">"
            | "<="
            | ">="
            | "=="
            | "!="
            | "&&"
            | "||"
            | "^^"
            | "!"
            | "~"
            | "&"
            | "|"
            | "^"
            | "<<"
            | ">>"
            | "+="
            | "-="
            | "*="
            | "/="
            | "%="
            | "<<="
            | ">>="
            | "&="
            | "|="
            | "^="
    )
}

/// Classifies one occurrence of a contextual word from its neighbours.
pub(crate) fn classify_usage(role: KeywordRole, prev: Neighbour<'_>, next: Neighbour<'_>) -> Usage {
    use KeywordRole as R;
    use Neighbour as N;
    // An operator after which an operand must follow, other than the `(` and
    // `,` that can also start a parameter declaration.
    let prev_operator = matches!(prev, N::Punct(p) if p != "(" && p != "," && expects_operand(p));
    // What the preceding token proves: only identifiers follow a field selection,
    // statement keywords never follow an operator, and qualifiers only follow
    // `(` and `,` among the operators. A type name can follow any operator (as
    // a constructor), so for it the operator is weighed with the next token.
    let prev_identifier = match prev {
        N::Punct(".") => true,
        N::Punct(p) => match role {
            R::Switch | R::Case | R::Default => expects_operand(p),
            R::Qualifier | R::Layout => prev_operator,
            R::Type => false,
        },
        _ => false,
    };
    let from_next = match (role, next) {
        (_, N::Missing | N::Other) => Usage::Ambiguous,
        (R::Qualifier | R::Type | R::Case, N::Ident) => Usage::Keyword,
        (R::Case, N::Number) => Usage::Keyword,
        (_, N::Ident | N::Number) => Usage::Ambiguous,
        (R::Qualifier, N::Punct(_)) => Usage::Identifier,
        (R::Layout | R::Switch, N::Punct(p)) => {
            if p == "(" {
                Usage::Ambiguous
            } else {
                Usage::Identifier
            }
        }
        (R::Case, N::Punct(p)) => {
            if matches!(p, "(" | "+" | "-" | "~" | "!" | ":") {
                Usage::Ambiguous
            } else {
                Usage::Identifier
            }
        }
        (R::Default, N::Punct(p)) => {
            if p != ":" {
                Usage::Identifier
            } else if prev_identifier {
                Usage::Ambiguous
            } else {
                Usage::Keyword
            }
        }
        (R::Type, N::Punct(p)) => {
            if matches!(p, "(" | "[") || (matches!(p, "," | ")" | ";") && !prev_operator) {
                Usage::Ambiguous
            } else {
                Usage::Identifier
            }
        }
    };
    match (prev_identifier, from_next) {
        // Contradicting evidence (invalid code either way): leave it alone.
        (true, Usage::Keyword) => Usage::Ambiguous,
        (true, _) => Usage::Identifier,
        (false, u) => u,
    }
}

/// Classification of an escape candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReservedClass {
    Always,
    Below400(usize),
    Below430,
    /// Index into [`CONTEXTUAL_RESERVED`].
    Contextual(usize),
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
        .chain(
            CONTEXTUAL_RESERVED
                .iter()
                .enumerate()
                .map(|(i, w)| (w.word, ReservedClass::Contextual(i))),
        )
}

fn enabled(exts: &[ExtensionDirective], names: &[&str]) -> bool {
    exts.iter()
        .any(|e| e.behavior != "disable" && names.contains(&e.name.as_str()))
}

/// Whether a word of the given class must be escaped for this version and
/// extension set (for a contextual word: where it is used as an identifier).
pub(crate) fn must_escape(class: ReservedClass, version: u32, exts: &[ExtensionDirective]) -> bool {
    match class {
        ReservedClass::Always => true,
        ReservedClass::Below400(i) => {
            version < 400 && !enabled(exts, RESERVED_BELOW_400.get(i).map_or(&[], |e| e.1))
        }
        ReservedClass::Below430 => version < 430 && !enabled(exts, SSBO_EXTENSIONS),
        ReservedClass::Contextual(i) => CONTEXTUAL_RESERVED
            .get(i)
            .is_some_and(|w| version < w.since && !enabled(exts, w.extensions)),
    }
}

/// Whether `word` would be escaped in a shader with the given version and
/// extensions. For the words of [`CONTEXTUAL_RESERVED`] this answers whether
/// the version and extensions allow escaping its identifier uses; whether a
/// translation unit is actually escaped also depends on how the word is used
/// there (see the module documentation), and keyword uses never are.
///
/// ```
/// use sb_preprocess::would_escape;
/// assert!(would_escape("input", 460, &[]));
/// assert!(would_escape("sample", 330, &[]));
/// assert!(!would_escape("sample", 400, &[]));
/// assert!(!would_escape("shared", 430, &[]));
/// assert!(!would_escape("color", 120, &[]));
/// assert!(would_escape("flat", 120, &[]));
/// assert!(!would_escape("flat", 130, &[]));
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
    fn contextual_version_and_extension_gates() {
        for w in CONTEXTUAL_RESERVED {
            assert!(would_escape(w.word, w.since - 1, &[]), "{}", w.word);
            assert!(would_escape(w.word, 110, &[]), "{}", w.word);
            assert!(!would_escape(w.word, w.since, &[]), "{}", w.word);
            assert!(!would_escape(w.word, 460, &[]), "{}", w.word);
            for e in w.extensions {
                assert!(
                    !would_escape(w.word, 110, &[ext(e, "enable")]),
                    "{} {e}",
                    w.word
                );
                assert!(
                    would_escape(w.word, 110, &[ext(e, "disable")]),
                    "{} {e}",
                    w.word
                );
            }
        }
        let gpu4 = [ext("GL_EXT_gpu_shader4", "enable")];
        for w in ["smooth", "flat", "noperspective"] {
            assert!(!would_escape(w, 120, &gpu4), "{w}");
        }
        assert!(would_escape("switch", 120, &gpu4));
        let image = [ext("GL_ARB_shader_image_load_store", "require")];
        for w in ["coherent", "volatile", "restrict", "readonly", "writeonly"] {
            assert!(would_escape(w, 410, &[]), "{w}");
            assert!(!would_escape(w, 330, &image), "{w}");
        }
        assert!(!would_escape(
            "layout",
            130,
            &[ext("GL_ARB_explicit_attrib_location", "enable")]
        ));
        assert!(would_escape("layout", 130, &[]));
        assert!(!would_escape(
            "double",
            330,
            &[ext("GL_ARB_gpu_shader_fp64", "enable")]
        ));
        assert!(would_escape("double", 330, &[]));
    }

    fn role(word: &str) -> KeywordRole {
        CONTEXTUAL_RESERVED
            .iter()
            .find(|w| w.word == word)
            .map(|w| w.role)
            .expect("contextual word")
    }

    #[test]
    fn contextual_usage_classification() {
        use KeywordRole as R;
        use Neighbour::{Ident, Missing, Number, Other, Punct};
        use Usage::{Ambiguous, Identifier, Keyword};
        let c = classify_usage;
        // Qualifiers: `flat varying`, `readonly image2D`, `precision highp float`.
        for r in [R::Qualifier] {
            assert_eq!(c(r, Missing, Ident), Keyword);
            assert_eq!(c(r, Punct("("), Ident), Keyword);
            assert_eq!(c(r, Punct(","), Ident), Keyword);
            assert_eq!(c(r, Punct(";"), Ident), Keyword);
            for p in [";", "=", ",", ")", "(", ".", "[", "*", "+=", "++"] {
                assert_eq!(c(r, Ident, Punct(p)), Identifier, "{p}");
            }
            assert_eq!(c(r, Punct("."), Missing), Identifier);
            assert_eq!(c(r, Punct("="), Missing), Identifier);
            assert_eq!(c(r, Punct("("), Missing), Ambiguous);
            assert_eq!(c(r, Ident, Number), Ambiguous);
            assert_eq!(c(r, Ident, Other), Ambiguous);
            assert_eq!(c(r, Ident, Missing), Ambiguous);
            // Contradicting evidence is left alone.
            assert_eq!(c(r, Punct("="), Ident), Ambiguous);
        }
        assert_eq!(role("flat"), R::Qualifier);
        assert_eq!(role("highp"), R::Qualifier);
        // layout( is ambiguous (a function could be named `layout`).
        assert_eq!(c(R::Layout, Punct(";"), Punct("(")), Ambiguous);
        assert_eq!(c(R::Layout, Ident, Punct("(")), Ambiguous);
        assert_eq!(c(R::Layout, Ident, Punct("=")), Identifier);
        assert_eq!(c(R::Layout, Punct("."), Punct("(")), Identifier);
        assert_eq!(c(R::Layout, Punct("("), Punct("(")), Ambiguous);
        assert_eq!(c(R::Layout, Punct("*"), Punct("(")), Identifier);
        assert_eq!(c(R::Layout, Ident, Ident), Ambiguous);
        // switch
        assert_eq!(c(R::Switch, Punct("{"), Punct("(")), Ambiguous);
        assert_eq!(c(R::Switch, Punct(")"), Punct("(")), Ambiguous);
        assert_eq!(c(R::Switch, Ident, Punct("(")), Ambiguous);
        assert_eq!(c(R::Switch, Punct("="), Punct("(")), Identifier);
        assert_eq!(c(R::Switch, Punct("("), Punct(")")), Identifier);
        assert_eq!(c(R::Switch, Ident, Punct(";")), Identifier);
        // case
        for p in ["(", "-", "+", "~", "!", ":"] {
            assert_eq!(c(R::Case, Punct(":"), Punct(p)), Ambiguous, "{p}");
            assert_eq!(c(R::Case, Punct("="), Punct(p)), Identifier, "{p}");
        }
        assert_eq!(c(R::Case, Punct("{"), Number), Keyword);
        assert_eq!(c(R::Case, Punct(";"), Ident), Keyword);
        assert_eq!(c(R::Case, Punct("?"), Punct(":")), Identifier);
        for p in ["=", ";", ",", ")", "]", ".", "[", "*", "<", "?", "++", "-="] {
            assert_eq!(c(R::Case, Ident, Punct(p)), Identifier, "{p}");
        }
        // default
        assert_eq!(c(R::Default, Punct(";"), Punct(":")), Keyword);
        assert_eq!(c(R::Default, Punct("{"), Punct(":")), Keyword);
        assert_eq!(c(R::Default, Missing, Punct(":")), Keyword);
        assert_eq!(c(R::Default, Punct("?"), Punct(":")), Identifier);
        assert_eq!(c(R::Default, Punct("("), Punct(":")), Identifier);
        assert_eq!(c(R::Default, Ident, Punct("=")), Identifier);
        assert_eq!(c(R::Default, Ident, Punct(".")), Identifier);
        assert_eq!(c(R::Default, Ident, Ident), Ambiguous);
        // double
        assert_eq!(c(R::Type, Punct(";"), Ident), Keyword);
        assert_eq!(c(R::Type, Punct("="), Ident), Keyword);
        for p in ["(", "[", ",", ")", ";"] {
            assert_eq!(c(R::Type, Ident, Punct(p)), Ambiguous, "{p}");
            assert_eq!(c(R::Type, Punct("("), Punct(p)), Ambiguous, "{p}");
            assert_eq!(c(R::Type, Punct(","), Punct(p)), Ambiguous, "{p}");
            assert_eq!(c(R::Type, Punct(";"), Punct(p)), Ambiguous, "{p}");
        }
        // After an operator a type name can only start a constructor.
        for p in ["(", "["] {
            assert_eq!(c(R::Type, Punct("="), Punct(p)), Ambiguous, "{p}");
            assert_eq!(c(R::Type, Punct("*"), Punct(p)), Ambiguous, "{p}");
        }
        for p in [",", ")", ";"] {
            assert_eq!(c(R::Type, Punct("="), Punct(p)), Identifier, "{p}");
            assert_eq!(c(R::Type, Punct("?"), Punct(p)), Identifier, "{p}");
            assert_eq!(c(R::Type, Punct("+="), Punct(p)), Identifier, "{p}");
        }
        for p in ["=", "*", ".", "+=", "?", ":"] {
            assert_eq!(c(R::Type, Ident, Punct(p)), Identifier, "{p}");
        }
        assert_eq!(c(R::Type, Punct("."), Punct("(")), Identifier);
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
