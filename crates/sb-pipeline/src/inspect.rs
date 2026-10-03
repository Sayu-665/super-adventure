//! [`inspect`]: a quick summary of a pack without translating anything.

use crate::load;
use sb_core::Diagnostics;
use sb_core::model::{CompileEnvironment, DhStrategy};
use sb_core::program::{GeometryGroup, GeometryProgram};
use sb_pack::{OptionValues, ShaderPack};
use serde::{Deserialize, Serialize};

/// One program of a folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramSummary {
    /// Name within the folder (`gbuffers_terrain`, `composite3_a`, `composite3.csh`).
    pub name: String,
    /// Stage file extensions (`vsh`, `fsh`, ..., `csh`); a synthesized vertex shader is
    /// listed as `vsh*`.
    pub stages: Vec<String>,
}

/// One program folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderSummary {
    /// `""` (pack root), `world0`, ...
    pub folder: String,
    /// Dimension ids the folder serves (`*` wildcard).
    pub dimension_ids: Vec<String>,
    /// Enabled programs (default option values).
    pub programs: Vec<ProgramSummary>,
    /// Programs disabled by `program.*.enabled` or the current profile.
    pub disabled: Vec<String>,
    /// Distant Horizons strategy with DH present.
    pub dh_strategy: DhStrategy,
}

/// What [`inspect`] reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackSummary {
    /// Pack name.
    pub name: String,
    /// Program folders that would be compiled.
    pub folders: Vec<FolderSummary>,
    /// Discovered options.
    pub option_count: usize,
    /// `profile.*` definitions.
    pub profile_count: usize,
    /// `screen.*` sub-screens.
    pub screen_count: usize,
    /// Profile matching the default values, if any.
    pub current_profile: Option<String>,
    /// `iris.features.required` as written.
    pub features_required: Vec<String>,
    /// `iris.features.optional` as written.
    pub features_optional: Vec<String>,
    /// Required flags ShaderBridge does not support.
    pub features_unsupported: Vec<String>,
    /// `lang/*.lang` codes.
    pub languages: Vec<String>,
    /// `uniform.*` / `variable.*` definitions.
    pub custom_uniform_count: usize,
    /// Effective `texture.*` / `customTexture.*` entries.
    pub custom_texture_count: usize,
    /// The pack has a `block.properties`.
    pub has_block_properties: bool,
    /// Loading diagnostics (properties, options, id maps).
    pub diagnostics: Diagnostics,
}

/// Summarize a pack: programs per folder, options, profiles, feature flags and the DH
/// strategy (with default option values and the default environment, DH enabled).
pub fn inspect(pack: &ShaderPack) -> PackSummary {
    let env = CompileEnvironment::default();
    let l = load::load(pack, &env, &OptionValues::new(), "en_us", None);
    let mut folders = Vec::new();
    for info in &l.folders {
        let plan = crate::resolve::resolve_folder(pack, &info.folder, &l, &env);
        let programs = plan
            .units
            .iter()
            .map(|u| ProgramSummary {
                name: u.name.clone(),
                stages: u
                    .stages
                    .iter()
                    .map(|(s, src)| match src {
                        crate::resolve::StageSource::SynthesizedVertex { .. } => format!("{}*", s.pack_extension()),
                        _ => s.pack_extension().to_string(),
                    })
                    .collect(),
            })
            .collect();
        let native = plan.geometry.keys().any(|g| matches!(g, GeometryProgram::DhTerrain | GeometryProgram::DhWater));
        let has_gbuffers = plan.geometry.keys().any(|g| g.group() == GeometryGroup::Gbuffers);
        let dh_strategy = if native {
            DhStrategy::Native
        } else if has_gbuffers {
            DhStrategy::Synthesized
        } else {
            DhStrategy::Disabled
        };
        folders.push(FolderSummary {
            folder: info.folder.clone(),
            dimension_ids: info.dimension_ids.clone(),
            programs,
            disabled: plan.disabled.clone(),
            dh_strategy,
        });
    }
    PackSummary {
        name: pack.name().to_string(),
        folders,
        option_count: l.options.options.len(),
        profile_count: l.options_model.profiles.len(),
        screen_count: l.options_model.screens.len(),
        current_profile: l.options_model.current_profile.clone(),
        features_required: l.props.features_required.clone(),
        features_optional: l.props.features_optional.clone(),
        features_unsupported: l.features_unsupported.clone(),
        languages: pack.languages(),
        custom_uniform_count: l.props.custom_uniforms.len(),
        custom_texture_count: l.props.texture_directives().len(),
        has_block_properties: pack.exists("block.properties"),
        diagnostics: l.diagnostics,
    }
}
