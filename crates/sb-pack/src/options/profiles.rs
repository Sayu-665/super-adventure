//! `profile.<NAME>` definitions (Iris `ProfileSet`).
//!
//! Tokens of a profile, in order (later tokens override earlier ones):
//!
//! * `!program.[dim/]name` disables a program while the profile is active;
//! * `profile.OTHER` includes another profile;
//! * `!OPT` sets `OPT=false`;
//! * `OPT=value` or `OPT:value` sets a value;
//! * `OPT` sets a boolean option to `true` (other names are ignored with a warning).

use super::{DiscoveredOptions, OptionValues, effective, is_boolean_option};
use indexmap::IndexMap;
use sb_core::{Diagnostic, Diagnostics};
use serde::{Deserialize, Serialize};

/// A resolved profile.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// Option name -> value (nested profiles expanded).
    pub options: IndexMap<String, String>,
    /// Programs disabled by `!program.<path>` (e.g. `composite2`, `world0/deferred`).
    pub disabled_programs: Vec<String>,
}

impl Profile {
    /// Whether the profile disables `program` of program folder `folder` (`""` = pack
    /// root). Like `program.<path>.enabled`, `!program.<path>` names the program's path
    /// relative to the shaders root (`composite2` for the root, `world0/composite2` for
    /// a world folder); Iris compares it exactly.
    pub fn disables(&self, folder: &str, program: &str) -> bool {
        let path = crate::shaders_properties::program_path(folder, program);
        self.disabled_programs.contains(&path)
    }

    /// Whether the current values match this profile (Iris `Profile.matches`): every
    /// listed boolean / value option has the listed value. Unknown names are ignored.
    pub fn matches(&self, options: &DiscoveredOptions, values: &OptionValues) -> bool {
        self.options
            .iter()
            .all(|(name, value)| match options.option(name) {
                Some(o) => effective(o, values.get(name)) == *value,
                None => true,
            })
    }
}

/// Resolve raw `profile.<NAME>` token lists (from
/// [`crate::shaders_properties::ShadersProperties::profiles`]).
pub fn resolve_profiles(
    defs: &IndexMap<String, Vec<String>>,
    options: &DiscoveredOptions,
) -> (IndexMap<String, Profile>, Diagnostics) {
    let mut diags = Diagnostics::new();
    let mut out = IndexMap::new();
    for name in defs.keys() {
        let mut stack = vec![name.clone()];
        let mut profile = Profile {
            name: name.clone(),
            ..Default::default()
        };
        expand(name, defs, options, &mut stack, &mut profile, &mut diags);
        out.insert(name.clone(), profile);
    }
    (out, diags)
}

fn expand(
    name: &str,
    defs: &IndexMap<String, Vec<String>>,
    options: &DiscoveredOptions,
    stack: &mut Vec<String>,
    profile: &mut Profile,
    diags: &mut Diagnostics,
) {
    let Some(tokens) = defs.get(name) else { return };
    for token in tokens {
        if let Some(program) = token.strip_prefix("!program.") {
            if !profile.disabled_programs.iter().any(|p| p == program) {
                profile.disabled_programs.push(program.to_string());
            }
        } else if let Some(dep) = token.strip_prefix("profile.") {
            if stack.iter().any(|s| s == dep) {
                diags.push(Diagnostic::warning(
                    "opt.profile-cycle",
                    format!(
                        "profile `{}` includes `{dep}` recursively ({})",
                        profile.name,
                        stack.join(" -> ")
                    ),
                ));
                continue;
            }
            if !defs.contains_key(dep) {
                diags.push(Diagnostic::warning(
                    "opt.profile-missing",
                    format!("profile `{name}` includes unknown profile `{dep}`"),
                ));
                continue;
            }
            stack.push(dep.to_string());
            expand(dep, defs, options, stack, profile, diags);
            stack.pop();
        } else if let Some(opt) = token.strip_prefix('!') {
            profile.options.insert(opt.to_string(), "false".to_string());
        } else if let Some((k, v)) = token.split_once('=') {
            profile.options.insert(k.to_string(), v.to_string());
        } else if let Some((k, v)) = token.split_once(':') {
            profile.options.insert(k.to_string(), v.to_string());
        } else if options.option(token).is_some_and(is_boolean_option) {
            profile.options.insert(token.clone(), "true".to_string());
        } else {
            diags.push(Diagnostic::warning(
                "opt.profile-token",
                format!("profile `{name}`: `{token}` is not a boolean option; ignored"),
            ));
        }
    }
}

/// The profile matching the current values (Iris scans profiles with more settings
/// first; ties keep declaration order). `None` = "Custom".
pub fn detect_profile<'a>(
    profiles: &'a IndexMap<String, Profile>,
    options: &DiscoveredOptions,
    values: &OptionValues,
) -> Option<&'a Profile> {
    let mut sorted: Vec<&Profile> = profiles.values().collect();
    sorted.sort_by_key(|p| std::cmp::Reverse(p.options.len()));
    sorted.into_iter().find(|p| p.matches(options, values))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ShaderPack;
    use crate::options::discover;
    use pretty_assertions::assert_eq;

    fn setup() -> (DiscoveredOptions, IndexMap<String, Vec<String>>) {
        let p = ShaderPack::from_files(
            "t",
            [(
                "a.fsh",
                "#define SHADOWS\n#ifdef SHADOWS\n#endif\n//#define BLOOM\n#ifdef BLOOM\n#endif\n#define QUALITY 2 // [1 2 3]",
            )],
        );
        let (o, _) = discover(&p, &["a.fsh".to_string()]);
        let mut defs = IndexMap::new();
        let toks = |s: &str| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
        defs.insert(
            "LOW".to_string(),
            toks("!SHADOWS QUALITY=1 !program.composite2 !program.world0/deferred"),
        );
        defs.insert("MEDIUM".to_string(), toks("profile.LOW SHADOWS QUALITY:2"));
        defs.insert(
            "HIGH".to_string(),
            toks("profile.MEDIUM BLOOM QUALITY=3 NOT_BOOL"),
        );
        defs.insert("LOOP".to_string(), toks("profile.LOOP2"));
        defs.insert(
            "LOOP2".to_string(),
            toks("profile.LOOP BLOOM profile.GHOST"),
        );
        (o, defs)
    }

    #[test]
    fn resolution_with_nesting_and_program_toggles() {
        let (o, defs) = setup();
        let (profiles, diags) = resolve_profiles(&defs, &o);
        let low = &profiles["LOW"];
        assert_eq!(
            low.options.get("SHADOWS").map(String::as_str),
            Some("false")
        );
        assert_eq!(low.options.get("QUALITY").map(String::as_str), Some("1"));
        assert_eq!(low.disabled_programs, vec!["composite2", "world0/deferred"]);
        assert!(low.disables("", "composite2"));
        assert!(!low.disables("world0", "composite2"), "exact path match");
        assert!(low.disables("world0", "deferred"));
        assert!(!low.disables("", "deferred"));
        let medium = &profiles["MEDIUM"];
        assert_eq!(
            medium.options.get("SHADOWS").map(String::as_str),
            Some("true")
        );
        assert_eq!(medium.options.get("QUALITY").map(String::as_str), Some("2"));
        assert_eq!(medium.disabled_programs.len(), 2, "inherited from LOW");
        let high = &profiles["HIGH"];
        assert_eq!(high.options.get("BLOOM").map(String::as_str), Some("true"));
        assert_eq!(high.options.get("QUALITY").map(String::as_str), Some("3"));
        let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(
            codes,
            vec![
                "opt.profile-token",
                "opt.profile-cycle",
                "opt.profile-missing",
                "opt.profile-cycle",
                "opt.profile-missing"
            ]
        );
        assert_eq!(
            profiles["LOOP"].options.get("BLOOM").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn detection_prefers_more_specific_profiles() {
        let (o, defs) = setup();
        let (profiles, _) = resolve_profiles(&defs, &o);
        // Defaults: SHADOWS on, BLOOM off, QUALITY 2 -> MEDIUM.
        let current = detect_profile(&profiles, &o, &OptionValues::new()).map(|p| p.name.as_str());
        assert_eq!(current, Some("MEDIUM"));
        let v = OptionValues::from_pairs([("BLOOM", "true"), ("QUALITY", "3")]);
        assert_eq!(
            detect_profile(&profiles, &o, &v).map(|p| p.name.as_str()),
            Some("HIGH")
        );
        let v = OptionValues::from_pairs([("QUALITY", "3")]);
        assert_eq!(
            detect_profile(&profiles, &o, &v).map(|p| p.name.as_str()),
            None
        );
    }
}
