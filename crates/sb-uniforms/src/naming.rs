//! Names ShaderBridge derives from pack names when one name needs several uniform
//! members or bindings (ARCHITECTURE §5.1, §5.2).
//!
//! When a pack declares the same uniform name with different types, or the same
//! sampler name with incompatible kinds, the first declaration keeps the plain name and
//! every other one gets a **conflict variant** named
//! `sb_as_<suffix>_<name>` (for example `sb_as_float_worldTime`,
//! `sb_as_shadow_shadowtex1`, `sb_as_uint_colortex2`, `sb_as_vec3_8_lights`).
//!
//! The scheme is chosen so that derived names
//!
//! * never contain `__`, which GLSL reserves ("identifiers containing two consecutive
//!   underscores are reserved"; glslang warns about them), unless the pack's own name
//!   already does. Underscores at the edges of the pack name are folded into the
//!   separator;
//! * cannot collide with pack identifiers: the translator renames every pack
//!   identifier that starts with `sb_` (except uniform names, which are the interface)
//!   to `sbu_*`, and ShaderBridge generates no other identifier starting with
//!   `sb_as_`. The rare remaining collisions (a pack *uniform* literally named
//!   `sb_as_...`, or two variants that spell the same) are resolved by the callers with
//!   [`uniquified_name`].
//!
//! Use [`is_derived_name`] to recognise every name [`conflict_name`] and
//! [`uniquified_name`] can produce for a pack name.

/// Prefix of every conflict-variant name (see the [module docs](self)). ShaderBridge
/// generates no other identifier with this prefix.
pub const CONFLICT_PREFIX: &str = "sb_as_";

/// Join two identifier parts with exactly one `_`, folding underscores at the join so
/// that the result never gets `__` from the join itself.
fn join(a: &str, b: &str) -> String {
    let a = a.trim_end_matches('_');
    let b = b.trim_start_matches('_');
    format!("{a}_{b}")
}

/// Collapse runs of `_` and strip them at the edges (`tex_composite__a_` →
/// `tex_composite_a`).
fn squeeze(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for part in s.split('_').filter(|p| !p.is_empty()) {
        if !out.is_empty() {
            out.push('_');
        }
        out.push_str(part);
    }
    out
}

/// The conflict-variant name of pack name `name` for a declaration described by
/// `suffix` (a type such as `float` or `vec3_8`, or a resource kind such as `shadow`,
/// `uint`, `3d_int`, `atlas`): `sb_as_<suffix>_<name>`.
///
/// `suffix` is normalised (runs of `_` collapsed, edges trimmed; an empty suffix
/// becomes `x`), and leading underscores of `name` are folded into the separator, so
/// the result contains `__` only if `name` itself does.
///
/// ```
/// use sb_uniforms::naming::conflict_name;
/// assert_eq!(conflict_name("worldTime", "float"), "sb_as_float_worldTime");
/// assert_eq!(conflict_name("shadowtex1", "shadow"), "sb_as_shadow_shadowtex1");
/// assert_eq!(conflict_name("_tmp", "vec3_8"), "sb_as_vec3_8_tmp");
/// ```
pub fn conflict_name(name: &str, suffix: &str) -> String {
    let suffix = squeeze(suffix);
    let suffix = if suffix.is_empty() { "x".to_string() } else { suffix };
    join(&format!("{CONFLICT_PREFIX}{suffix}"), name)
}

/// `base` made unique with counter `n` (2, 3, ...): `<base>_<n>`, with trailing
/// underscores of `base` folded into the separator.
///
/// ```
/// use sb_uniforms::naming::uniquified_name;
/// assert_eq!(uniquified_name("sb_as_uint_colortex2", 2), "sb_as_uint_colortex2_2");
/// assert_eq!(uniquified_name("tmp_", 3), "tmp_3");
/// ```
pub fn uniquified_name(base: &str, n: u32) -> String {
    join(base, &n.to_string())
}

/// If `candidate` ends with a [`uniquified_name`] counter (`_<digits>`), the part
/// before it.
fn strip_uniquifier(candidate: &str) -> Option<&str> {
    let digits = candidate.len() - candidate.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let stem = candidate[..candidate.len() - digits].strip_suffix('_')?;
    // `uniquified_name` folds trailing underscores of the base, so a genuine stem never
    // ends with one (it is empty for an all-underscore base).
    (!stem.ends_with('_')).then_some(stem)
}

/// Whether `candidate` is `name` itself or a name that [`conflict_name`] and
/// [`uniquified_name`] can derive from it: `name`, `name_<n>`,
/// `sb_as_<suffix>_<name>` or `sb_as_<suffix>_<name>_<n>`.
///
/// Different pack names can derive the same spelling in contrived cases (`foo_2` is
/// both a pack name and the uniquified `foo`), so callers must also compare what the
/// candidate refers to.
///
/// ```
/// use sb_uniforms::naming::is_derived_name;
/// assert!(is_derived_name("colortex2", "colortex2"));
/// assert!(is_derived_name("sb_as_uint_colortex2", "colortex2"));
/// assert!(is_derived_name("sb_as_uint_colortex2_2", "colortex2"));
/// assert!(is_derived_name("colortex2_3", "colortex2"));
/// assert!(!is_derived_name("sb_as_uint_colortex21", "colortex2"));
/// assert!(!is_derived_name("colortex2_a", "colortex2"));
/// ```
pub fn is_derived_name(candidate: &str, name: &str) -> bool {
    if candidate == name {
        return true;
    }
    let variant = |stem: &str, uniquified: bool| -> bool {
        let Some(rest) = stem.strip_prefix(CONFLICT_PREFIX) else {
            return false;
        };
        let tail = name.trim_start_matches('_');
        let tail = if uniquified { tail.trim_end_matches('_') } else { tail };
        if tail.is_empty() {
            // `name` is all underscores: `sb_as_<suffix>_`, uniquified `sb_as_<suffix>_<n>`.
            let suffix = if uniquified { Some(rest) } else { rest.strip_suffix('_') };
            return suffix.is_some_and(|s| !s.is_empty());
        }
        rest.strip_suffix(tail)
            .and_then(|s| s.strip_suffix('_'))
            .is_some_and(|suffix| !suffix.is_empty())
    };
    if variant(candidate, false) {
        return true;
    }
    match strip_uniquifier(candidate) {
        Some(stem) => stem == name.trim_end_matches('_') || variant(stem, true),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_names_never_contain_double_underscores() {
        for name in ["foo", "_foo", "foo_", "_foo_", "__", "_", "a_b"] {
            for suffix in ["float", "vec3_8", "3d_int", "tex_composite__a_", "", "_"] {
                let v = conflict_name(name, suffix);
                assert!(v.starts_with(CONFLICT_PREFIX), "{v}");
                assert!(!v.contains("__"), "{name} {suffix} -> {v}");
                assert!(is_derived_name(&v, name), "{v} from {name}");
                for n in [2, 3, 10] {
                    let u = uniquified_name(&v, n);
                    assert!(!u.contains("__"), "{u}");
                    assert!(is_derived_name(&u, name), "{u} from {name}");
                }
            }
            let u = uniquified_name(name, 2);
            assert!(!u.contains("__"), "{u}");
            assert!(is_derived_name(&u, name), "{u} from {name}");
        }
    }

    #[test]
    fn spellings() {
        assert_eq!(conflict_name("worldTime", "float"), "sb_as_float_worldTime");
        assert_eq!(conflict_name("lights", "vec3_8"), "sb_as_vec3_8_lights");
        assert_eq!(conflict_name("foo_", "uint"), "sb_as_uint_foo_");
        assert_eq!(conflict_name("_", "uint"), "sb_as_uint_");
        assert_eq!(conflict_name("x", "tex_composite__a_"), "sb_as_tex_composite_a_x");
        assert_eq!(conflict_name("x", ""), "sb_as_x_x");
        assert_eq!(uniquified_name("foo", 2), "foo_2");
        assert_eq!(uniquified_name("foo_", 2), "foo_2");
        assert_eq!(uniquified_name("sb_as_uint_", 2), "sb_as_uint_2");
    }

    #[test]
    fn unrelated_names_are_not_derived() {
        for (candidate, name) in [
            ("colortex21", "colortex2"),
            ("colortex2_a", "colortex2"),
            ("colortex2_", "colortex2"),
            ("colortex2__2", "colortex2"),
            ("xcolortex2", "colortex2"),
            ("sb_as_colortex2", "colortex2"),
            ("sb_as__colortex2", "colortex2"),
            ("sb_as_uint_xcolortex2", "colortex2"),
            ("sb_as_uint_colortex2_x", "colortex2"),
            ("sb_tex_uint_colortex2", "colortex2"),
            ("sbu_as_uint_colortex2", "colortex2"),
            ("sb_as_uint_foo", "foo_bar"),
            ("2", "x"),
            ("_2", "x"),
        ] {
            assert!(!is_derived_name(candidate, name), "{candidate} from {name}");
        }
    }

    #[test]
    fn uniquifier_parsing() {
        assert_eq!(strip_uniquifier("foo_2"), Some("foo"));
        assert_eq!(strip_uniquifier("foo_12"), Some("foo"));
        assert_eq!(strip_uniquifier("foo2"), None);
        assert_eq!(strip_uniquifier("foo_"), None);
        assert_eq!(strip_uniquifier("_2"), Some(""));
        assert_eq!(strip_uniquifier("foo__2"), None);
        assert_eq!(strip_uniquifier("2"), None);
    }
}
