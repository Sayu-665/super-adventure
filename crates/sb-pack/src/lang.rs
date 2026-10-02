//! Shader pack language files (`shaders/lang/<code>.lang`).
//!
//! Lang files use `.properties` syntax but are UTF-8 encoded (Iris reads them with a
//! UTF-8 reader). Keys follow the OptiFine conventions: `option.<NAME>`,
//! `option.<NAME>.comment`, `value.<NAME>.<value>`, `prefix.<NAME>`, `suffix.<NAME>`,
//! `screen.<NAME>`, `screen.<NAME>.comment`, `profile.<NAME>`,
//! `profile.<NAME>.comment`, `profile.comment`.
//!
//! Use [`crate::ShaderPack::lang`] to load a language with `en_us` fallback.

use crate::properties;
use indexmap::IndexMap;

/// Parse the text of a `.lang` file into an ordered key -> text map.
pub fn parse_lang(text: &str) -> IndexMap<String, String> {
    properties::parse(text)
        .into_iter()
        .map(|e| (e.key, e.value))
        .collect()
}

/// Display name of an option (`option.<NAME>`), if translated.
pub fn option_name<'a>(lang: &'a IndexMap<String, String>, option: &str) -> Option<&'a str> {
    lang.get(&format!("option.{option}")).map(String::as_str)
}

/// Tooltip of an option (`option.<NAME>.comment`), if translated.
pub fn option_comment<'a>(lang: &'a IndexMap<String, String>, option: &str) -> Option<&'a str> {
    lang.get(&format!("option.{option}.comment"))
        .map(String::as_str)
}

/// Display text of an option value (`value.<NAME>.<value>`), if translated.
pub fn value_name<'a>(
    lang: &'a IndexMap<String, String>,
    option: &str,
    value: &str,
) -> Option<&'a str> {
    lang.get(&format!("value.{option}.{value}"))
        .map(String::as_str)
}

/// Display name of a screen (`screen.<NAME>`), if translated.
pub fn screen_name<'a>(lang: &'a IndexMap<String, String>, screen: &str) -> Option<&'a str> {
    lang.get(&format!("screen.{screen}")).map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lang_entries() {
        let text = "# Comment\noption.SHADOWS=Shadows\noption.SHADOWS.comment=Enables \\\n    shadows.\nvalue.QUALITY.0=Low\nscreen.LIGHTING=Lighting \u{2600}\n";
        let lang = parse_lang(text);
        assert_eq!(option_name(&lang, "SHADOWS"), Some("Shadows"));
        assert_eq!(option_comment(&lang, "SHADOWS"), Some("Enables shadows."));
        assert_eq!(value_name(&lang, "QUALITY", "0"), Some("Low"));
        assert_eq!(screen_name(&lang, "LIGHTING"), Some("Lighting \u{2600}"));
        assert_eq!(option_name(&lang, "MISSING"), None);
    }

    #[test]
    fn color_codes_and_escapes_survive() {
        let lang = parse_lang("option.A=\u{a7}6Gold \\u00e9");
        assert_eq!(lang["option.A"], "\u{a7}6Gold \u{e9}");
    }
}
