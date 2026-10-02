//! Building the model's [`OptionsModel`] (everything a host needs for an options GUI).

use super::{DiscoveredOptions, OptionValues, detect_profile, resolve_profiles};
use crate::shaders_properties::ShadersProperties;
use indexmap::IndexMap;
use sb_core::model::{OptionsModel, ScreenEntry};
use sb_core::{Diagnostic, Diagnostics};
use std::collections::HashSet;

/// Assemble the options GUI model: options with current values, screens, sliders,
/// resolved profiles, the detected current profile and lang strings.
///
/// Reports screen entries naming unknown options or screens, and sliders naming
/// unknown options, as informational diagnostics.
pub fn build_options_model(
    props: &ShadersProperties,
    options: &DiscoveredOptions,
    values: &OptionValues,
    lang: IndexMap<String, String>,
) -> (OptionsModel, Diagnostics) {
    let mut diags = Diagnostics::new();
    let mut opts = options.clone();
    opts.apply_values(values);

    let (profiles, d) = resolve_profiles(&props.profiles, options);
    diags.extend(d);
    let current_profile = detect_profile(&profiles, options, values).map(|p| p.name.clone());

    let (main_screen, main_screen_columns, screens) = props.screens_model();
    let known: HashSet<&str> = options.names().collect();
    let mut check = |where_: &str, entries: &[ScreenEntry]| {
        for e in entries {
            match e {
                ScreenEntry::Option(name) if !known.contains(name.as_str()) => {
                    diags.push(Diagnostic::info(
                        "opt.screen-unknown-option",
                        format!("{where_} lists `{name}`, which is not an option of this pack"),
                    ))
                }
                ScreenEntry::Screen(name) if !screens.contains_key(name) => {
                    diags.push(Diagnostic::warning(
                        "opt.screen-unknown-screen",
                        format!("{where_} links to screen `{name}`, which is not defined"),
                    ))
                }
                _ => {}
            }
        }
    };
    check("screen", &main_screen);
    for (name, s) in &screens {
        check(&format!("screen.{name}"), &s.entries);
    }
    for s in &props.sliders {
        if !known.contains(s.as_str()) {
            diags.push(Diagnostic::info(
                "opt.slider-unknown-option",
                format!("sliders lists `{s}`, which is not an option of this pack"),
            ));
        }
    }

    let model = OptionsModel {
        options: opts.options,
        main_screen,
        main_screen_columns,
        screens,
        sliders: props.sliders.clone(),
        profile_disabled_programs: profiles
            .iter()
            .filter(|(_, p)| !p.disabled_programs.is_empty())
            .map(|(k, p)| (k.clone(), p.disabled_programs.clone()))
            .collect(),
        profiles: profiles.into_iter().map(|(k, p)| (k, p.options)).collect(),
        current_profile,
        lang,
    };
    (model, diags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::discover;
    use crate::{ShaderPack, properties, shaders_properties};

    #[test]
    fn builds_gui_model() {
        let pack = ShaderPack::from_files(
            "t",
            [
                (
                    "a.fsh",
                    "#define SHADOWS\n#ifdef SHADOWS\n#endif\n#define QUALITY 2 // [1 2 3]",
                ),
                (
                    "shaders.properties",
                    "screen=<profile> [MORE] SHADOWS GHOST [NOWHERE]\nscreen.MORE=QUALITY *\nscreen.MORE.columns=1\nsliders=QUALITY GHOST2\nprofile.LOW=!SHADOWS QUALITY=1\nprofile.HIGH=SHADOWS QUALITY=3",
                ),
                ("lang/en_us.lang", "option.SHADOWS=Shadows"),
            ],
        );
        let (o, _) = discover(&pack, &["a.fsh".to_string()]);
        let entries = pack.read_properties("shaders.properties").unwrap();
        let (props, _) = shaders_properties::parse(
            &entries,
            &properties::parse(&pack.read_latin1("shaders.properties").unwrap()),
        );
        let values = OptionValues::from_pairs([("SHADOWS", "false"), ("QUALITY", "1")]);
        let (m, d) = build_options_model(&props, &o, &values, pack.lang("en_us"));
        assert_eq!(m.options.len(), 2);
        assert_eq!(
            m.options
                .iter()
                .find(|x| x.name == "SHADOWS")
                .unwrap()
                .value,
            "false"
        );
        assert_eq!(m.main_screen.len(), 5);
        assert_eq!(m.screens["MORE"].columns, Some(1));
        assert_eq!(m.sliders, vec!["QUALITY", "GHOST2"]);
        assert_eq!(m.profiles["LOW"]["SHADOWS"], "false");
        assert_eq!(m.current_profile.as_deref(), Some("LOW"));
        assert_eq!(m.lang["option.SHADOWS"], "Shadows");
        let codes: Vec<&str> = d.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(
            codes,
            vec![
                "opt.screen-unknown-option",
                "opt.screen-unknown-screen",
                "opt.slider-unknown-option"
            ]
        );
    }
}
