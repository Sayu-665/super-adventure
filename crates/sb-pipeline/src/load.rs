//! Loading the pack-wide inputs of a compile (spec steps 2–5): `shaders.properties` (read
//! twice, as Iris does), options, feature flags, dimension folders, id maps and the
//! options GUI model.

use crate::macros::{FeatureMacroScope, canonical_feature, color_space_macros, feature_macros, is_feature_supported};
use indexmap::IndexMap;
use sb_core::model::{CompileEnvironment, IdMaps, OptionsModel};
use sb_core::{Diagnostic, Diagnostics, SourceLocation};
use sb_pack::options::{DiscoveredOptions, OptionValues};
use sb_pack::shaders_properties::{self, ShadersProperties};
use sb_pack::{ShaderPack, properties};
use std::collections::BTreeSet;

/// One program folder to compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderInfo {
    /// `""` (pack root), `world0`, ...
    pub folder: String,
    /// Dimension ids it serves (`*` wildcard).
    pub dimension_ids: Vec<String>,
}

/// Everything loaded before programs are processed.
#[derive(Debug, Clone)]
pub struct PackLoad {
    pub options: DiscoveredOptions,
    pub values: OptionValues,
    pub props: ShadersProperties,
    pub id_maps: IdMaps,
    pub options_model: OptionsModel,
    /// Folders to compile, in order (root first).
    pub folders: Vec<FolderInfo>,
    /// Macros for GLSL preprocessing.
    pub glsl_macros: IndexMap<String, Option<String>>,
    /// Supported flags the pack declares (canonical names, declaration order).
    pub features_enabled: Vec<String>,
    /// Required flags ShaderBridge does not support (as written).
    pub features_unsupported: Vec<String>,
    /// Canonical names of every declared flag (required or optional), supported or not.
    pub features_declared: BTreeSet<String>,
    /// Programs disabled by the detected current profile (`!program.<path>`).
    pub profile_disabled: Vec<String>,
    pub diagnostics: Diagnostics,
}

impl PackLoad {
    /// Whether the pack declared `flag` (canonical) in `iris.features.*`.
    pub fn feature_active(&self, flag: &str) -> bool {
        self.features_declared.contains(flag)
    }
}

/// Java `\R` line splitting (CR LF counts once).
fn java_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if matches!(c, '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            out.push(&text[start..i]);
            let mut next = i + c.len_utf8();
            if c == '\r' && it.peek().is_some_and(|&(_, d)| d == '\n') {
                it.next();
                next += 1;
            }
            start = next;
        }
    }
    out.push(&text[start..]);
    out
}

/// Original 1-based line of every line of `sb_preprocess::preprocess_properties` output:
/// Iris's properties preprocessor drops blank lines, so output line `i` is the `i`-th
/// non-blank input line.
pub fn properties_line_map(original: &str) -> Vec<u32> {
    let text = original.strip_prefix('\u{FEFF}').unwrap_or(original);
    java_lines(text)
        .into_iter()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim_matches(|c: char| c <= ' ');
            !t.chars().all(|c| c.is_whitespace() || matches!(c, '\u{1C}'..='\u{1F}'))
        })
        .map(|(i, _)| u32::try_from(i + 1).unwrap_or(u32::MAX))
        .collect()
}

/// Preprocess a `.properties` file and parse it (entries keep original line numbers).
pub fn preprocess_and_parse(
    text: &str,
    file: &str,
    macros: &IndexMap<String, Option<String>>,
    diags: &mut Diagnostics,
) -> (String, Vec<properties::PropEntry>) {
    let (pre, d) = sb_preprocess::preprocess_properties(text, file, macros);
    diags.extend(d);
    let mut entries = properties::parse_preprocessed(&pre);
    properties::remap_entry_lines(&mut entries, &properties_line_map(text));
    (pre, entries)
}

/// The world folders that exist with runnable programs: `world0`, `world-1`, `world1`, then
/// the folders `dimension.properties` names (sb-pack `ShaderPack::world_folders`, with an
/// explicit dimension map because the pack is borrowed).
fn world_folders(pack: &ShaderPack, dim_map: Option<&IndexMap<String, Vec<String>>>) -> Vec<String> {
    let mut candidates: Vec<String> = sb_pack::STANDARD_WORLD_FOLDERS.iter().map(|(f, _)| f.to_string()).collect();
    if let Some(map) = dim_map {
        for folder in map.keys() {
            if !candidates.contains(folder) {
                candidates.push(folder.clone());
            }
        }
    }
    candidates
        .into_iter()
        .filter(|f| {
            sb_pack::vfs::clean_path(f).is_some_and(|c| !c.is_empty()) && pack.program_set(f).has_runnable_programs()
        })
        .collect()
}

/// Folder → dimension ids (Iris rules; sb-pack `ShaderPack::dimension_assignments`).
pub fn dimension_assignments(
    pack: &ShaderPack,
    dim_map: Option<&IndexMap<String, Vec<String>>>,
) -> IndexMap<String, Vec<String>> {
    let existing = world_folders(pack, dim_map);
    match dim_map {
        Some(map) if !map.is_empty() => {
            map.iter().filter(|(f, _)| existing.contains(f)).map(|(f, ids)| (f.clone(), ids.clone())).collect()
        }
        _ => sb_pack::STANDARD_WORLD_FOLDERS
            .iter()
            .filter(|(f, _)| existing.iter().any(|e| e == f))
            .map(|(f, ids)| (f.to_string(), ids.iter().map(|s| s.to_string()).collect()))
            .collect(),
    }
}

/// Option-discovery start files (Iris `ShaderPackSourceNames`): every stage file of every
/// recognized program in the root and the assigned folders, plus the computes of
/// composite-style programs and `shadow`.
pub fn option_start_files(pack: &ShaderPack, assignments: &IndexMap<String, Vec<String>>) -> Vec<String> {
    use sb_core::program::GeometryProgram;
    let mut folders = vec![String::new()];
    folders.extend(assignments.keys().cloned());
    let mut files: Vec<String> = Vec::new();
    for set in folders.iter().map(|f| pack.program_set(f)) {
        for p in set.programs.values() {
            files.extend(p.stages.values().cloned());
            let computes_listed =
                !matches!(p.name, sb_core::ProgramName::Geometry { program } if program != GeometryProgram::Shadow);
            if computes_listed {
                files.extend(p.computes.values().cloned());
                files.extend(p.ignored_computes.values().cloned());
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

/// Load the pack-wide inputs.
pub fn load(
    pack: &ShaderPack,
    env: &CompileEnvironment,
    values: &OptionValues,
    language: &str,
    dimension_filter: Option<&[String]>,
) -> PackLoad {
    let mut diags = pack.vfs_diagnostics();
    let env_macros = crate::macros::standard_macros(env);

    // dimension.properties: environment macros only.
    let mut dim_text_pre: Option<String> = None;
    let dim_map = pack.read_latin1("dimension.properties").map(|text| {
        let (pre, entries) = preprocess_and_parse(&text, "dimension.properties", &env_macros, &mut diags);
        dim_text_pre = Some(pre);
        let mut map: IndexMap<String, Vec<String>> = IndexMap::new();
        for e in entries {
            match e.key.strip_prefix("dimension.") {
                Some(folder) if !folder.is_empty() => {
                    map.insert(folder.to_string(), e.value.split_whitespace().map(str::to_string).collect());
                }
                _ => diags.push(
                    Diagnostic::warning("idmap.unexpected-key", format!("unexpected key `{}` in dimension.properties", e.key))
                        .at(SourceLocation::new("dimension.properties", e.line)),
                ),
            }
        }
        map
    });
    let assignments = dimension_assignments(pack, dim_map.as_ref());

    // Options.
    let start = option_start_files(pack, &assignments);
    let (options, d) = sb_pack::options::discover(pack, &start);
    diags.extend(d);
    let mut opt_values = values.clone();
    for name in values.values.keys() {
        if options.option(name).is_none() {
            diags.push(Diagnostic::info("opt.unknown-value", format!("option `{name}` set by the user is not an option of this pack; ignored")));
            opt_values.remove(name);
        }
    }
    let option_macros = options.property_macros(&opt_values);

    // shaders.properties: functional keys from the preprocessed text (options + all
    // supported feature flags), GUI/feature keys from the raw text.
    let mut props_macros = env_macros.clone();
    props_macros.extend(feature_macros(&[], env, FeatureMacroScope::Properties));
    props_macros.extend(option_macros.clone());
    let props = match pack.read_latin1("shaders.properties") {
        Some(text) => {
            let (_, pre) = preprocess_and_parse(&text, shaders_properties::FILE, &props_macros, &mut diags);
            let raw = properties::parse(&text);
            let (p, d) = shaders_properties::parse(&pre, &raw);
            diags.extend(d);
            p
        }
        None => ShadersProperties::default(),
    };

    // Feature flags.
    let mut features_enabled = Vec::new();
    let mut features_unsupported = Vec::new();
    let mut features_declared = BTreeSet::new();
    for (flag, required) in
        props.features_required.iter().map(|f| (f, true)).chain(props.features_optional.iter().map(|f| (f, false)))
    {
        let canonical = canonical_feature(flag);
        if let Some(c) = canonical {
            features_declared.insert(c.to_string());
        }
        match canonical {
            Some(c) if is_feature_supported(c, env) => {
                if !features_enabled.iter().any(|e: &String| e == c) {
                    features_enabled.push(c.to_string());
                }
            }
            _ if required => {
                features_unsupported.push(flag.clone());
                diags.push(
                    Diagnostic::error(
                        "pack.feature-unsupported",
                        format!("the pack requires feature `{flag}`, which ShaderBridge does not support in this environment (Iris would refuse the pack)"),
                    )
                    .at(SourceLocation::new(shaders_properties::FILE, 1)),
                );
            }
            _ => diags.push(Diagnostic::info(
                "pack.feature-optional-unsupported",
                format!("optional feature `{flag}` is not supported; IRIS_FEATURE_{flag} is not defined"),
            )),
        }
    }
    if !props.buffer_objects.is_empty() && !features_declared.contains("SSBO") {
        diags.push(Diagnostic::warning(
            "pack.feature-missing",
            "the pack uses bufferObject.* without declaring the SSBO feature flag (Iris refuses such packs); continuing",
        ));
    }
    if !props.images.is_empty() && !features_declared.contains("CUSTOM_IMAGES") {
        diags.push(Diagnostic::warning(
            "pack.feature-missing",
            "the pack uses image.* without declaring the CUSTOM_IMAGES feature flag (Iris refuses such packs); continuing",
        ));
    }

    // GLSL macros: environment + optional feature flags as written + colour spaces.
    let mut glsl_macros = env_macros.clone();
    glsl_macros.extend(feature_macros(&props.features_optional, env, FeatureMacroScope::Glsl));
    if props.supports_color_correction == Some(true) {
        glsl_macros.extend(color_space_macros());
    }

    // Id maps: Iris preprocesses them with the final environment (GLSL macros) + options.
    let mut idmap_macros = glsl_macros.clone();
    idmap_macros.extend(option_macros);
    let mut read_idmap = |file: &str| -> Option<String> {
        let text = pack.read_latin1(file)?;
        let (pre, d) = sb_preprocess::preprocess_properties(&text, file, &idmap_macros);
        diags.extend(d);
        Some(sb_preprocess::iris_idmap_fixups(&pre))
    };
    let block = read_idmap("block.properties");
    let item = read_idmap("item.properties");
    let entity = read_idmap("entity.properties");
    let (id_maps, d) =
        sb_pack::idmap::build_id_maps(block.as_deref(), item.as_deref(), entity.as_deref(), dim_text_pre.as_deref());
    diags.extend(d);

    // Options GUI model.
    let (options_model, d) = sb_pack::options::build_options_model(&props, &options, &opt_values, pack.lang(language));
    diags.extend(d);
    let profile_disabled: Vec<String> = options_model
        .current_profile
        .as_ref()
        .and_then(|p| options_model.profile_disabled_programs.get(p))
        .cloned()
        .unwrap_or_default();

    // Folders: root (when it has programs) + assigned world folders.
    let mut all: Vec<FolderInfo> = Vec::new();
    let wildcard_taken = assignments.values().any(|ids| ids.iter().any(|i| i == "*" || i == "*:*"));
    if pack.program_set("").has_runnable_programs() {
        all.push(FolderInfo {
            folder: String::new(),
            dimension_ids: if wildcard_taken { Vec::new() } else { vec!["*".to_string()] },
        });
    }
    for (folder, ids) in &assignments {
        all.push(FolderInfo { folder: folder.clone(), dimension_ids: ids.clone() });
    }
    if all.is_empty() {
        diags.push(Diagnostic::error("pack.no-programs", "the pack has no runnable programs (no shaders/ program files found)"));
    }
    let folders = match dimension_filter {
        Some(filter) => {
            for f in filter {
                if !all.iter().any(|x| &x.folder == f) {
                    diags.push(Diagnostic::warning(
                        "pipeline.unknown-dimension",
                        format!("requested dimension folder `{f}` does not exist or has no programs"),
                    ));
                }
            }
            all.into_iter().filter(|x| filter.contains(&x.folder)).collect()
        }
        None => all,
    };

    PackLoad {
        options,
        values: opt_values,
        props,
        id_maps,
        options_model,
        folders,
        glsl_macros,
        features_enabled,
        features_unsupported,
        features_declared,
        profile_disabled,
        diagnostics: diags,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn line_map_skips_blank_lines() {
        let text = "a=1\n\n  \n# comment\nb=2\r\n\tc=3";
        assert_eq!(properties_line_map(text), vec![1, 4, 5, 6]);
    }

    /// Cross-crate item 7: a `\` continuation followed by a directive line ends the
    /// logical line (Iris hides backslashes from its preprocessor), so photon's
    /// `moon_phase_brightness` parses to the complete `if(...)` expression.
    #[test]
    fn continuation_before_directive_ends_value() {
        let text = "#ifdef MOON_PHASE_AFFECTS_BRIGHTNESS\nuniform.float.moon_phase_brightness = if( \\\n\tmoonPhase == 0, 1.0,   \\\n\tmoonPhase == 1, 0.875, \\\n\t1.0 \\\n) \\\n#else\nuniform.float.moon_phase_brightness = 1.0\n#endif\nuniform.float.after = 2.0\n";
        let mut macros = IndexMap::new();
        macros.insert("MOON_PHASE_AFFECTS_BRIGHTNESS".to_string(), None);
        let mut d = Diagnostics::new();
        let (_, entries) = preprocess_and_parse(text, "shaders.properties", &macros, &mut d);
        assert!(!d.has_errors(), "{d:?}");
        let (props, _) = shaders_properties::parse(&entries, &properties::parse(text));
        let cu: Vec<_> = props.custom_uniforms.iter().map(|c| (c.name.clone(), c.expression.split_whitespace().collect::<Vec<_>>().join(" "))).collect();
        assert_eq!(
            cu,
            vec![
                ("moon_phase_brightness".to_string(), "if( moonPhase == 0, 1.0, moonPhase == 1, 0.875, 1.0 )".to_string()),
                ("after".to_string(), "2.0".to_string()),
            ]
        );
        // Entries keep their original line numbers.
        assert_eq!(entries[0].line, 2);
        let compiled = sb_expr::CustomUniforms::compile(
            &props.custom_uniforms,
            &|n: &str| sb_uniforms::custom_uniform_input_type(n),
            &crate::macros::expression_constants(),
        );
        assert!(!compiled.1.has_errors(), "{:?}", compiled.1);

        // Option off: the #else branch defines the constant.
        let (_, entries) = preprocess_and_parse(text, "shaders.properties", &IndexMap::new(), &mut d);
        let (props, _) = shaders_properties::parse(&entries, &properties::parse(text));
        assert_eq!(props.custom_uniforms[0].expression.trim(), "1.0");
    }

    #[test]
    fn folders_and_features() {
        let pack = ShaderPack::from_files(
            "p",
            [
                ("shaders.properties", "iris.features.required=SSBO MAGIC\niris.features.optional=CUSTOM_IMAGES\nsun=false\n"),
                ("world0/composite.fsh", "void main(){}"),
                ("world-1/composite.fsh", "void main(){}"),
                ("lang/en_us.lang", "option.X=Ex"),
            ],
        );
        let env = CompileEnvironment::default();
        let l = load(&pack, &env, &OptionValues::new(), "en_us", None);
        let folders: Vec<_> = l.folders.iter().map(|f| (f.folder.as_str(), f.dimension_ids.clone())).collect();
        assert_eq!(
            folders,
            vec![
                ("world0", vec!["minecraft:overworld".to_string(), "*".to_string()]),
                ("world-1", vec!["minecraft:the_nether".to_string()]),
            ]
        );
        assert_eq!(l.features_enabled, vec!["SSBO".to_string(), "CUSTOM_IMAGES".to_string()]);
        assert_eq!(l.features_unsupported, vec!["MAGIC".to_string()]);
        assert!(l.diagnostics.iter().any(|d| d.code == "pack.feature-unsupported" && d.is_error()));
        assert!(l.glsl_macros.contains_key("IRIS_FEATURE_CUSTOM_IMAGES"));
        assert!(!l.glsl_macros.contains_key("IRIS_FEATURE_SSBO"));
        assert!(!l.props.settings().sun);
        let only: Vec<String> = vec!["world-1".into()];
        let l = load(&pack, &env, &OptionValues::new(), "en_us", Some(&only));
        assert_eq!(l.folders.len(), 1);
        assert_eq!(l.folders[0].folder, "world-1");
    }

    #[test]
    fn dimension_properties_and_root() {
        let pack = ShaderPack::from_files(
            "p",
            [
                ("dimension.properties", "dimension.custom=*\ndimension.world0=minecraft:overworld\n"),
                ("composite.fsh", "void main(){}"),
                ("custom/final.fsh", "void main(){}"),
                ("world0/final.fsh", "void main(){}"),
                ("world1/final.fsh", "void main(){}"),
            ],
        );
        let l = load(&pack, &CompileEnvironment::default(), &OptionValues::new(), "en_us", None);
        let folders: Vec<_> = l.folders.iter().map(|f| (f.folder.clone(), f.dimension_ids.clone())).collect();
        // world1 is not named by dimension.properties: ignored (Iris).
        assert_eq!(
            folders,
            vec![
                (String::new(), vec![]),
                ("custom".to_string(), vec!["*".to_string()]),
                ("world0".to_string(), vec!["minecraft:overworld".to_string()]),
            ]
        );
        assert_eq!(l.id_maps.dimensions.get("custom"), Some(&vec!["*".to_string()]));
    }
}
