//! Draw profiles (ARCHITECTURE §6): how a host draw path feeds geometry and per-draw
//! state to a translated program.
//!
//! A profile lists the host's vertex attributes, uniform blocks and samplers, and GLSL
//! expressions that implement the OptiFine/Iris compatibility semantics (`gl_Vertex`,
//! `gl_Color`, `gl_MultiTexCoord*`, `mc_Entity`, matrices, ...). The built-in profiles
//! live in `profiles/*.toml` (see `profiles/README.md` for the schema) and are embedded
//! with `include_str!`; hosts can register more with [`parse_profile`].

use std::collections::BTreeSet;
use std::sync::LazyLock;

use indexmap::IndexMap;
use sb_core::GeometryProgram;
use serde::{Deserialize, Serialize};

/// Name of the profile used by composite-style (fullscreen) passes.
pub const FULLSCREEN_PROFILE: &str = "fullscreen";

/// A host vertex attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileInput {
    /// Attribute name; must equal the host vertex-format element name (renderpearl
    /// matches attributes by name).
    pub name: String,
    /// GLSL type the attribute is declared with (`vec3`, `uvec3`, `ivec2`, ...).
    #[serde(rename = "type")]
    pub ty: String,
    /// Attribute location.
    pub location: u32,
    /// Per-instance attribute (informational).
    #[serde(default)]
    pub instanced: bool,
}

/// A host std140 uniform block, declared verbatim with an instance name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileBlock {
    /// Block type name (hosts such as Mojang's renderpearl match blocks by this name).
    pub name: String,
    /// Instance name used by the profile's expressions (`sb_h*`).
    pub instance: String,
    /// Member declarations, e.g. `mat4 ModelViewMat; ivec2 TextureSize;`.
    pub members: String,
}

/// A host sampler and the pack sampler names (canonical, see sb-uniforms) it satisfies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSampler {
    /// Sampler name as the host binds it (`Sampler0`, `uLightMap`, ...).
    pub name: String,
    /// GLSL sampler type (default `sampler2D`).
    #[serde(rename = "type", default = "default_sampler_type")]
    pub ty: String,
    /// Canonical pack sampler names this host sampler provides (`gtexture`, `lightmap`).
    #[serde(default)]
    pub provides: Vec<String>,
}

fn default_sampler_type() -> String {
    "sampler2D".to_string()
}

/// GLSL expressions implementing the compatibility semantics, evaluated in the vertex
/// prologue. Missing entries fall back to [`default_semantics`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Semantics {
    /// `vec4` `gl_Vertex` / `vaPosition` (camera-relative position for world geometry).
    pub position: Option<String>,
    /// `vec4` `gl_Color` / `vaColor`.
    pub color: Option<String>,
    /// `vec4` `gl_MultiTexCoord0` / `vaUV0`.
    pub uv0: Option<String>,
    /// `vec4` `gl_MultiTexCoord1/2` / `vaUV2` (0..240 range; x = block, y = sky).
    pub lightmap: Option<String>,
    /// `vec3` `gl_Normal` / `vaNormal`.
    pub normal: Option<String>,
    /// `vec4` `mc_Entity` (x = block id or -1, y = render type).
    pub entity: Option<String>,
    /// `vec4` `mc_midTexCoord` / `gl_MultiTexCoord3`.
    pub mid_tex_coord: Option<String>,
    /// `vec4` `at_tangent`.
    pub tangent: Option<String>,
    /// `vec4` `at_midBlock`.
    pub mid_block: Option<String>,
    /// `vec3` `at_velocity`.
    pub velocity: Option<String>,
    /// `ivec2` `vaUV1` (overlay coordinates).
    pub overlay: Option<String>,
    /// `mat4` `gl_ModelViewMatrix` / `modelViewMatrix`.
    pub model_view: Option<String>,
    /// `mat4` `gl_ProjectionMatrix` / `projectionMatrix` (GL-style forward Z).
    pub projection: Option<String>,
    /// `mat4` `gl_TextureMatrix[0]` / `textureMatrix`.
    pub texture_matrix: Option<String>,
    /// `mat4` `gl_TextureMatrix[1]` and `[2]`.
    pub lightmap_matrix: Option<String>,
    /// `mat3` `gl_NormalMatrix` / `normalMatrix` (default `mat3(transpose(inverse(model_view)))`).
    pub normal_matrix: Option<String>,
    /// `vec3` `chunkOffset` / `modelOffset`.
    pub chunk_offset: Option<String>,
}

/// Every semantic key with its GLSL type, in prologue order.
pub const SEMANTIC_KEYS: &[(&str, &str)] = &[
    ("model_view", "mat4"),
    ("projection", "mat4"),
    ("texture_matrix", "mat4"),
    ("lightmap_matrix", "mat4"),
    ("normal_matrix", "mat3"),
    ("chunk_offset", "vec3"),
    ("position", "vec4"),
    ("color", "vec4"),
    ("uv0", "vec4"),
    ("lightmap", "vec4"),
    ("normal", "vec3"),
    ("entity", "vec4"),
    ("mid_tex_coord", "vec4"),
    ("tangent", "vec4"),
    ("mid_block", "vec4"),
    ("velocity", "vec3"),
    ("overlay", "ivec2"),
];

impl Semantics {
    /// The expression for `key` (one of [`SEMANTIC_KEYS`]), if this table defines it.
    pub fn get(&self, key: &str) -> Option<&str> {
        let v = match key {
            "position" => &self.position,
            "color" => &self.color,
            "uv0" => &self.uv0,
            "lightmap" => &self.lightmap,
            "normal" => &self.normal,
            "entity" => &self.entity,
            "mid_tex_coord" => &self.mid_tex_coord,
            "tangent" => &self.tangent,
            "mid_block" => &self.mid_block,
            "velocity" => &self.velocity,
            "overlay" => &self.overlay,
            "model_view" => &self.model_view,
            "projection" => &self.projection,
            "texture_matrix" => &self.texture_matrix,
            "lightmap_matrix" => &self.lightmap_matrix,
            "normal_matrix" => &self.normal_matrix,
            "chunk_offset" => &self.chunk_offset,
            _ => return None,
        };
        v.as_deref()
    }
}

/// An extra pack-visible variable the profile defines, initialized in the vertex
/// prologue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileGlobal {
    /// Variable name (packs may reference it, e.g. `dhMaterialId`, `mc_chunkFade`).
    pub name: String,
    /// GLSL type.
    #[serde(rename = "type")]
    pub ty: String,
    /// Stage the global is defined in (only `vertex` is supported).
    #[serde(default = "default_global_stage")]
    pub stage: String,
    /// Initializer expression, evaluated in the vertex prologue.
    pub init: String,
    /// Forward the value to every later stage that references the name.
    #[serde(default)]
    pub varying: bool,
}

fn default_global_stage() -> String {
    "vertex".to_string()
}

/// A draw profile (see the [module docs](self)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawProfile {
    /// Unique id (`vanilla_terrain`, `dh_terrain`, `fullscreen`, ...).
    pub name: String,
    /// Free text.
    pub description: String,
    /// Used for fullscreen passes: no vertex inputs, a triangle generated from
    /// `gl_VertexIndex`.
    pub fullscreen: bool,
    /// Host vertex attributes.
    pub inputs: Vec<ProfileInput>,
    /// Host uniform blocks.
    pub blocks: Vec<ProfileBlock>,
    /// Host samplers.
    pub samplers: Vec<ProfileSampler>,
    /// Compatibility semantics.
    pub semantics: Semantics,
    /// Extra pack-visible globals.
    pub globals: Vec<ProfileGlobal>,
    /// Helper code injected before the pack's code in vertex-like stages.
    pub code_vertex: String,
    /// Helper code injected before the pack's code in the fragment stage.
    pub code_fragment: String,
    /// Positions are world-space (camera-relative) world geometry: in the shadow pass the
    /// `model_view`/`projection` semantics become `shadowModelView`/`shadowProjection`
    /// (Iris shadow programs see `gl_ModelViewMatrix == shadowModelView`).
    pub world_space: bool,
    /// Push-constant members (`layout(push_constant) uniform sb_hPush { ... }`, accessed
    /// unqualified); empty for none.
    pub push_constants: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCode {
    #[serde(default)]
    vertex: String,
    #[serde(default)]
    fragment: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    #[allow(dead_code)]
    host: Option<String>,
    #[serde(default)]
    fullscreen: bool,
    #[serde(default)]
    world_space: bool,
    #[serde(default)]
    push_constants: Option<String>,
    #[serde(default)]
    inputs: Vec<ProfileInput>,
    #[serde(default)]
    blocks: Vec<ProfileBlock>,
    #[serde(default)]
    samplers: Vec<ProfileSampler>,
    #[serde(default)]
    semantics: Semantics,
    #[serde(default)]
    globals: Vec<ProfileGlobal>,
    code: Option<RawCode>,
}

fn is_identifier(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphabetic() || f == '_')
        && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Parse and validate a draw profile from TOML (the schema of `profiles/README.md`).
///
/// Validation: identifiers are valid GLSL names, input locations are unique, input
/// and global types are valid value types, sampler types are sampler types, and
/// globals live in the vertex stage.
pub fn parse_profile(toml_text: &str) -> Result<DrawProfile, String> {
    let raw: RawProfile = toml::from_str(toml_text).map_err(|e| e.to_string())?;
    let code = raw.code.unwrap_or(RawCode { vertex: String::new(), fragment: String::new() });
    let p = DrawProfile {
        name: raw.name,
        description: raw.description,
        fullscreen: raw.fullscreen,
        inputs: raw.inputs,
        blocks: raw.blocks,
        samplers: raw.samplers,
        semantics: raw.semantics,
        globals: raw.globals,
        code_vertex: code.vertex,
        code_fragment: code.fragment,
        world_space: raw.world_space,
        push_constants: raw.push_constants.unwrap_or_default(),
    };
    validate(&p)?;
    Ok(p)
}

fn validate(p: &DrawProfile) -> Result<(), String> {
    if p.name.is_empty() {
        return Err("profile name is empty".into());
    }
    let mut locations = BTreeSet::new();
    let mut names = BTreeSet::new();
    for i in &p.inputs {
        if !is_identifier(&i.name) || i.name.starts_with("gl_") {
            return Err(format!("input `{}` is not a valid GLSL identifier", i.name));
        }
        if sb_core::GlslType::parse(&i.ty).is_none() {
            return Err(format!("input `{}` has unsupported type `{}`", i.name, i.ty));
        }
        if !locations.insert(i.location) {
            return Err(format!("input location {} is used twice", i.location));
        }
        if !names.insert(i.name.clone()) {
            return Err(format!("input `{}` is declared twice", i.name));
        }
    }
    for b in &p.blocks {
        if !is_identifier(&b.name) || !is_identifier(&b.instance) {
            return Err(format!("block `{}` / instance `{}` is not a valid identifier", b.name, b.instance));
        }
        if !names.insert(b.instance.clone()) {
            return Err(format!("block instance `{}` is declared twice", b.instance));
        }
    }
    for s in &p.samplers {
        if !is_identifier(&s.name) {
            return Err(format!("sampler `{}` is not a valid identifier", s.name));
        }
        if sb_uniforms::sampler_kind(&s.ty).is_none() {
            return Err(format!("sampler `{}` has unsupported type `{}`", s.name, s.ty));
        }
        if !names.insert(s.name.clone()) {
            return Err(format!("sampler `{}` is declared twice", s.name));
        }
    }
    for g in &p.globals {
        if !is_identifier(&g.name) || g.name.starts_with("gl_") {
            return Err(format!("global `{}` is not a valid identifier", g.name));
        }
        if sb_core::GlslType::parse(&g.ty).is_none() {
            return Err(format!("global `{}` has unsupported type `{}`", g.name, g.ty));
        }
        if g.stage != "vertex" {
            return Err(format!("global `{}`: only stage = \"vertex\" is supported", g.name));
        }
        if !names.insert(g.name.clone()) {
            return Err(format!("global `{}` is declared twice", g.name));
        }
    }
    Ok(())
}

struct Embedded {
    defaults: DrawProfile,
    profiles: Vec<DrawProfile>,
}

const EMBEDDED_FILES: &[(&str, &str)] = &[
    ("fullscreen.toml", include_str!("../profiles/fullscreen.toml")),
    ("vanilla_terrain.toml", include_str!("../profiles/vanilla_terrain.toml")),
    ("vanilla_terrain_basic.toml", include_str!("../profiles/vanilla_terrain_basic.toml")),
    ("vanilla_entity.toml", include_str!("../profiles/vanilla_entity.toml")),
    ("vanilla_particle.toml", include_str!("../profiles/vanilla_particle.toml")),
    ("vanilla_lines.toml", include_str!("../profiles/vanilla_lines.toml")),
    ("vanilla_position.toml", include_str!("../profiles/vanilla_position.toml")),
    ("vanilla_position_color.toml", include_str!("../profiles/vanilla_position_color.toml")),
    ("vanilla_position_tex.toml", include_str!("../profiles/vanilla_position_tex.toml")),
    ("vanilla_position_tex_color.toml", include_str!("../profiles/vanilla_position_tex_color.toml")),
    ("dh_terrain.toml", include_str!("../profiles/dh_terrain.toml")),
    ("dh_generic.toml", include_str!("../profiles/dh_generic.toml")),
];

const DEFAULTS_FILE: &str = include_str!("../profiles/defaults.toml");

static EMBEDDED: LazyLock<Embedded> = LazyLock::new(|| {
    // The embedded files are covered by tests; a broken file yields an empty profile
    // rather than a panic.
    let parse = |file: &str, text: &str| {
        parse_profile(text).unwrap_or_else(|e| DrawProfile {
            name: format!("invalid:{file}"),
            description: format!("embedded profile {file} failed to parse: {e}"),
            fullscreen: false,
            inputs: Vec::new(),
            blocks: Vec::new(),
            samplers: Vec::new(),
            semantics: Semantics::default(),
            globals: Vec::new(),
            code_vertex: String::new(),
            code_fragment: String::new(),
            world_space: false,
            push_constants: String::new(),
        })
    };
    Embedded {
        defaults: parse("defaults.toml", DEFAULTS_FILE),
        profiles: EMBEDDED_FILES.iter().map(|(f, t)| parse(f, t)).collect(),
    }
});

/// Every embedded draw profile (not including the `defaults` table).
pub fn builtin_profiles() -> &'static [DrawProfile] {
    &EMBEDDED.profiles
}

/// The embedded profile named `name`.
pub fn profile(name: &str) -> Option<&'static DrawProfile> {
    EMBEDDED.profiles.iter().find(|p| p.name == name)
}

/// The default semantics (`profiles/defaults.toml`) used for keys a profile leaves out.
pub fn default_semantics() -> &'static Semantics {
    &EMBEDDED.defaults.semantics
}

/// Default draw profile of a geometry program (composite-style programs use
/// [`FULLSCREEN_PROFILE`]).
pub fn default_profile_for(program: GeometryProgram) -> &'static str {
    use GeometryProgram as G;
    match program {
        G::Terrain
        | G::TerrainSolid
        | G::TerrainCutout
        | G::Water
        | G::DamagedBlock
        | G::Shadow
        | G::ShadowSolid
        | G::ShadowCutout
        | G::ShadowWater => "vanilla_terrain",
        G::Block
        | G::BlockTranslucent
        | G::Entities
        | G::EntitiesTranslucent
        | G::EntitiesGlowing
        | G::Hand
        | G::HandWater
        | G::Item
        | G::ArmorGlint
        | G::SpiderEyes
        | G::Lightning
        | G::ShadowEntities
        | G::ShadowBlock
        | G::ShadowLightning => "vanilla_entity",
        G::Particles | G::ParticlesTranslucent | G::Weather => "vanilla_particle",
        G::SkyBasic => "vanilla_position",
        G::SkyTextured => "vanilla_position_tex",
        G::Clouds | G::Textured | G::TexturedLit | G::BeaconBeam => "vanilla_position_tex_color",
        G::Basic | G::Line => "vanilla_lines",
        G::DhTerrain | G::DhWater | G::DhShadow => "dh_terrain",
        G::DhGeneric => "dh_generic",
    }
}

/// Fallback expression for semantics neither the profile nor `defaults.toml` define.
fn hardcoded_default(key: &str) -> Option<&'static str> {
    Some(match key {
        "velocity" => "vec3(0.0)",
        "normal_matrix" => "mat3(transpose(inverse(sb_ModelView)))",
        "position" => "vec4(0.0, 0.0, 0.0, 1.0)",
        "color" => "vec4(1.0)",
        "uv0" | "mid_tex_coord" => "vec4(0.0, 0.0, 0.0, 1.0)",
        "lightmap" => "vec4(240.0, 240.0, 0.0, 1.0)",
        "normal" => "vec3(0.0, 1.0, 0.0)",
        "entity" => "vec4(-1.0, 0.0, 0.0, 0.0)",
        "tangent" => "vec4(1.0, 0.0, 0.0, 1.0)",
        "mid_block" => "vec4(0.0)",
        "overlay" => "ivec2(0, 10)",
        "model_view" | "projection" | "texture_matrix" | "lightmap_matrix" => "mat4(1.0)",
        "chunk_offset" => "vec3(0.0)",
        _ => return None,
    })
}

impl DrawProfile {
    /// The expression implementing semantic `key`: the profile's own, else the
    /// `defaults.toml` one, else a built-in fallback. `normal_matrix` defaults to
    /// `mat3(transpose(inverse(sb_ModelView)))` (`sb_ModelView` is the prologue global
    /// holding the model-view semantic).
    pub fn semantic(&self, key: &str) -> &str {
        if let Some(e) = self.semantics.get(key) {
            return e;
        }
        if key != "normal_matrix"
            && let Some(e) = default_semantics().get(key)
        {
            return e;
        }
        hardcoded_default(key).unwrap_or("0")
    }

    /// Whether the profile itself defines semantic `key`.
    pub fn defines_semantic(&self, key: &str) -> bool {
        self.semantics.get(key).is_some()
    }

    /// Names the profile declares (inputs, block instances and type names, samplers,
    /// globals and helper functions); pack identifiers with these names are renamed.
    pub fn declared_names(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for i in &self.inputs {
            out.insert(i.name.clone());
        }
        for b in &self.blocks {
            out.insert(b.name.clone());
            out.insert(b.instance.clone());
        }
        for s in &self.samplers {
            out.insert(s.name.clone());
        }
        for g in &self.globals {
            out.insert(g.name.clone());
        }
        for code in [&self.code_vertex, &self.code_fragment] {
            out.extend(crate::text::defined_functions(code));
        }
        out
    }

    /// The builtin-uniform names (sb-uniforms registry) that the profile's semantics
    /// (including defaults it inherits), globals and helper code reference.
    /// `sb-pipeline` adds them to the pack layout.
    pub fn referenced_builtins(&self) -> Vec<String> {
        let declared = self.declared_names();
        let mut out = BTreeSet::new();
        let mut scan = |text: &str| {
            for id in crate::text::identifiers(text) {
                if !declared.contains(id) && sb_uniforms::is_builtin(id) {
                    out.insert(id.to_string());
                }
            }
        };
        for (key, _) in SEMANTIC_KEYS {
            scan(self.semantic(key));
        }
        for g in &self.globals {
            scan(&g.init);
        }
        scan(&self.code_vertex);
        scan(&self.code_fragment);
        out.into_iter().collect()
    }

    /// The profile's sampler that provides the canonical pack sampler `canonical`.
    pub fn sampler_providing(&self, canonical: &str) -> Option<&ProfileSampler> {
        self.samplers.iter().find(|s| s.provides.iter().any(|p| p == canonical))
    }

    /// Semantics as an ordered map (key -> effective expression), for diagnostics and
    /// tools.
    pub fn effective_semantics(&self) -> IndexMap<&'static str, &str> {
        SEMANTIC_KEYS.iter().map(|(k, _)| (*k, self.semantic(k))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_profile_parses() {
        for (file, text) in EMBEDDED_FILES {
            let p = parse_profile(text).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert_eq!(format!("{}.toml", p.name), *file);
        }
        parse_profile(DEFAULTS_FILE).expect("defaults.toml");
        assert_eq!(builtin_profiles().len(), EMBEDDED_FILES.len());
        assert!(builtin_profiles().iter().all(|p| !p.name.starts_with("invalid:")));
    }

    #[test]
    fn default_profiles_exist() {
        for p in GeometryProgram::ALL {
            let name = default_profile_for(*p);
            assert!(profile(name).is_some(), "{p:?} -> {name}");
        }
        assert!(profile(FULLSCREEN_PROFILE).unwrap().fullscreen);
        assert_eq!(default_profile_for(GeometryProgram::ShadowEntities), "vanilla_entity");
        assert_eq!(default_profile_for(GeometryProgram::DhWater), "dh_terrain");
        assert_eq!(default_profile_for(GeometryProgram::SkyTextured), "vanilla_position_tex");
    }

    #[test]
    fn semantics_fall_back_to_defaults() {
        let p = profile("vanilla_lines").unwrap();
        assert_eq!(p.semantic("position"), "vec4(Position + sb_hDynamic.ModelOffset, 1.0)");
        assert_eq!(p.semantic("uv0"), "vec4(0.0, 0.0, 0.0, 1.0)");
        assert_eq!(p.semantic("normal_matrix"), "mat3(transpose(inverse(sb_ModelView)))");
        assert_eq!(p.semantic("velocity"), "vec3(0.0)");
        let f = profile("fullscreen").unwrap();
        assert_eq!(f.semantic("normal_matrix"), "mat3(1.0)");
    }

    #[test]
    fn referenced_builtins_of_profiles() {
        let t = profile("vanilla_terrain").unwrap();
        assert_eq!(t.referenced_builtins(), vec!["gbufferProjection".to_string()]);
        let e = profile("vanilla_entity").unwrap();
        let r = e.referenced_builtins();
        assert!(r.contains(&"entityId".to_string()), "{r:?}");
        assert!(r.contains(&"projectionMatrix".to_string()), "{r:?}");
        assert!(!r.contains(&"entityColor".to_string()), "{r:?}");
        let d = profile("dh_terrain").unwrap();
        let r = d.referenced_builtins();
        assert!(r.contains(&"dhProjection".to_string()) && r.contains(&"gbufferModelView".to_string()), "{r:?}");
        assert!(profile("fullscreen").unwrap().referenced_builtins().is_empty());
    }

    #[test]
    fn parse_profile_validates() {
        assert!(parse_profile("name = \"x\"\n[[inputs]]\nname = \"a\"\ntype = \"vec9\"\nlocation = 0\n").is_err());
        assert!(
            parse_profile(
                "name = \"x\"\n[[inputs]]\nname = \"a\"\ntype = \"vec3\"\nlocation = 0\n[[inputs]]\nname = \"b\"\ntype = \"vec3\"\nlocation = 0\n"
            )
            .is_err()
        );
        assert!(parse_profile("name = \"x\"\n[semantics]\nposition_typo = \"vec4(1.0)\"\n").is_err());
        assert!(parse_profile("name = \"x\"\n[[samplers]]\nname = \"s\"\ntype = \"image2D\"\n").is_err());
        let p = parse_profile("name = \"x\"\n[[samplers]]\nname = \"s\"\nprovides = [\"gtexture\"]\n").unwrap();
        assert_eq!(p.samplers[0].ty, "sampler2D");
        assert_eq!(p.sampler_providing("gtexture").unwrap().name, "s");
        assert!(parse_profile("not toml [").is_err());
    }

    #[test]
    fn declared_names_include_helpers() {
        let d = profile("dh_terrain").unwrap().declared_names();
        for n in ["vPosition", "meta", "uLightMap", "sb_hDhShared", "dh_hasTexture", "sb_dhNormal", "dhMaterialId"] {
            assert!(d.contains(n), "{n}");
        }
    }
}
