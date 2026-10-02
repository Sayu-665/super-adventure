//! Opaque-uniform (sampler/image) name classification (ARCHITECTURE §5.2).
//!
//! [`canonicalize`] maps the name a program declares to the canonical binding name
//! used in every translated shader and to the [`ResourceRef`] the host must bind. The
//! rules follow Iris (`IrisSamplers`, `IrisImages`, `ProgramSamplers`):
//!
//! * Aliases: `tex`, `texture`, `u_MainSampler` → `gtexture`; `gcolor`, `gdepth`,
//!   `gnormal`, `composite`, `gaux1..4` → `colortex0..7`; `gdepthtex` → `depthtex0`;
//!   `shadowcolor` → `shadowcolor0`; `dhDepthTex` → `dhDepthTex0`; `shadow` /
//!   `watershadow` follow the OptiFine rule ([`ResourceContext::watershadow_declared`]).
//! * Visibility: a sampler that Iris does not bind in a program class reads GL texture
//!   unit 0, which holds the atlas (`gtexture`) in gbuffers/shadow/DH programs and
//!   `colortex0` in fullscreen/compute programs. Such names resolve to that unit-0
//!   resource. In particular gbuffers/shadow/DH programs can only read
//!   `colortex4..31` (and `gaux1..4`): `colortex0..3`, `gcolor`, `gdepth`, `gnormal` and
//!   `composite` resolve to `gtexture`. Likewise `gdepthtex` is a composite-only alias,
//!   `dhBlockAtlas` exists only in DH programs, and the world samplers (`gtexture`,
//!   `lightmap`, `normals`, `specular`, `iris_overlay`) resolve to `colortex0` in
//!   fullscreen programs.
//! * Custom textures:
//!   * An image (PNG or resource) `texture.<stage>.<name>` overrides the builtin sampler
//!     group of `<name>`, whatever the declared sampler type (Iris
//!     `CustomTextureSamplerInterceptor`).
//!   * A *raw* `texture.<stage>.<name>` (`<path> <TYPE> <format> <size...> <pixel
//!     format> <pixel type>`) does **not** override the group. Iris (`TextureTransformer`)
//!     renames only declarations of exactly `<name>` whose sampler type matches the raw
//!     texture type (`sampler3D`/`isampler3D`/`usampler3D` for `TEXTURE_3D`, ...). Other
//!     declarations of the name keep the builtin resource. This needs the declared type:
//!     use [`canonicalize_with_kind`] (see [`ResourceContext::with_raw_texture`]).
//!   * `customTexture.<name>` defines a new sampler and overrides the render-target
//!     names (`colortexN`, legacy aliases, `dhDepthTex*`) that Iris registers before
//!     custom textures.
//! * Custom images (`image.<name>`) and their sampler names resolve to
//!   [`ResourceRef::Image`].
//! * Anything else is [`ResourceRef::Unknown`]: the host binds the unit-0 resource
//!   (atlas in geometry programs, `colortex0` in fullscreen programs).
//!
//! # Known differences from Iris
//!
//! * Distant Horizons programs: Iris builds them with `hasTexture = false`, so
//!   `gtexture`/`tex`/`texture`/`u_MainSampler`/`gcolor`/`colortex0` and `iris_overlay`
//!   read a 1×1 **white** texture there. Iris sets `dhBlockAtlas` to unit 0, so other
//!   unbound names read the DH block atlas. `sb_core::model::ResourceRef` has no "white
//!   texture" value. These names therefore resolve to `gtexture` / [`ResourceRef::Atlas`]
//!   as in gbuffers programs. A host should bind a white texel for `Atlas` in native DH
//!   programs. Synthesized DH programs remap `gtexture` through their draw profile.
//! * `shadowcomp` programs: Iris binds only `noisetex`, shadow samplers, custom
//!   textures and images there (`ShadowCompositeRenderer`). ShaderBridge treats them as
//!   [`ProgramClass::Fullscreen`], so `colortexN`/`depthtexN` also resolve. This is a
//!   superset; packs relying on unit-0 garbage there do not exist in practice.
//! * Shadow-pass computes (`shadow.csh`) get the [`ProgramClass::Shadow`] set, which
//!   includes `depthtex0..2`; Iris does not bind depth textures there.

use sb_core::model::{ProgramKind, ResourceKind, ResourceRef};
use sb_core::program::{GeometryGroup, GeometryProgram};
use std::collections::BTreeMap;

/// Number of `colortex` buffers (Iris `MAX_COLOR_BUFFERS`).
pub const MAX_COLOR_TEX: u32 = 32;
/// Number of `shadowcolor` buffers (with `HIGHER_SHADOWCOLOR`).
pub const MAX_SHADOW_COLOR: u32 = 8;
/// `depthtex0..2`.
pub const MAX_DEPTH_TEX: u32 = 3;
/// Stage string of `customTexture.<name>` entries (as in `sb_core::model::CustomTexture::stage`).
pub const CUSTOM_STAGE: &str = "custom";
/// Legacy names of `colortex0..7`.
pub const LEGACY_COLOR_NAMES: [&str; 8] = [
    "gcolor",
    "gdepth",
    "gnormal",
    "composite",
    "gaux1",
    "gaux2",
    "gaux3",
    "gaux4",
];
/// Names of the albedo atlas sampler (GL texture unit 0 in world programs).
pub const ALBEDO_NAMES: [&str; 4] = ["tex", "texture", "gtexture", "u_MainSampler"];

/// Which sampler set a program sees.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ProgramClass {
    /// `gbuffers_*` programs.
    Gbuffers,
    /// `shadow*` programs and shadow-pass computes (`shadow.csh`, `shadow_a.csh`, ...),
    /// which Iris binds like gbuffers programs.
    Shadow,
    /// Composite-style fullscreen passes (`begin`, `shadowcomp`, `prepare`, `deferred`,
    /// `composite`, `final`).
    Fullscreen,
    /// Computes of composite-style passes and `setup` (same samplers as fullscreen).
    Compute,
    /// Distant Horizons programs (`dh_*`).
    Dh,
}

impl ProgramClass {
    /// Class of a geometry program (by its [`GeometryGroup`]).
    pub fn from_geometry(program: GeometryProgram) -> Self {
        match program.group() {
            GeometryGroup::Gbuffers => Self::Gbuffers,
            GeometryGroup::Shadow => Self::Shadow,
            GeometryGroup::DistantHorizons => Self::Dh,
        }
    }

    /// Class of a program of the model.
    pub fn from_program_kind(kind: &ProgramKind) -> Self {
        match kind {
            ProgramKind::Geometry { program } | ProgramKind::GeometryCompute { program, .. } => {
                Self::from_geometry(*program)
            }
            ProgramKind::Composite { .. } => Self::Fullscreen,
            ProgramKind::Compute { .. } => Self::Compute,
        }
    }

    /// World-geometry programs (gbuffers, shadow, DH): unit 0 is the atlas and only
    /// `colortex4+` are readable render targets.
    pub fn is_geometry(self) -> bool {
        matches!(self, Self::Gbuffers | Self::Shadow | Self::Dh)
    }

    /// The resource on GL texture unit 0, which unknown samplers read: the atlas
    /// (`gtexture`) in geometry programs, `colortex0` otherwise.
    pub fn default_sampler(self) -> Canonical {
        if self.is_geometry() {
            Canonical::new("gtexture", ResourceRef::Atlas)
        } else {
            Canonical::new("colortex0", ResourceRef::ColorTex(0))
        }
    }
}

/// Per-program information [`canonicalize`] needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceContext {
    /// The program's class.
    pub class: ProgramClass,
    /// The program uses a sampler named `watershadow` (then `watershadow` =
    /// `shadowtex0` and `shadow` = `shadowtex1`; otherwise `shadow` = `shadowtex0`).
    ///
    /// Iris decides this **per program** from the *active* uniforms
    /// (`glGetUniformLocation("watershadow") != -1`), so a declaration the program never
    /// reads does not count. Prefer the names the program references over the names
    /// it merely declares (e.g. through a shared header).
    pub watershadow_declared: bool,
    /// Image (PNG/resource/dynamic) custom textures visible to the program: sampler name
    /// → texture stage. These are the `texture.<stage>.<name>` entries of the program's
    /// texture stage and every `customTexture.<name>` (stage [`CUSTOM_STAGE`]). Stage strings match
    /// `sb_core::model::CustomTexture::stage`. In composite-style programs the caller must
    /// leave out the `texture.<stage>.colortexN` (and legacy-alias) overrides that Iris
    /// deactivates because the buffer was flipped at least once earlier in the frame
    /// (`flippedAtLeastOnceSnapshot`; gbuffers/shadow/DH programs never deactivate
    /// them). Raw `texture.<stage>.<name>` entries do not belong here and are never
    /// deactivated; see [`ResourceContext::raw_textures`].
    ///
    /// The map holds one entry per sampler name, and the last
    /// [`ResourceContext::with_custom_texture`] call wins. A pack that defines both
    /// `customTexture.X` and `texture.<this stage>.X` for the same `X` keeps only one of
    /// them. Iris's own behaviour there differs between gbuffers and composite programs,
    /// and no known pack does it.
    pub custom_textures: BTreeMap<String, String>,
    /// Raw `texture.<stage>.<name>` entries of the program's texture stage, keyed by
    /// `(sampler name, sampler dimension)` with the stage as the value. The dimension uses
    /// the [`ResourceKind`] spelling: `1d`, `2d`, `3d`, `2d_rect`; see
    /// [`raw_texture_dim`]. They apply only through [`canonicalize_with_kind`], and only
    /// to non-shadow samplers of exactly that name and dimension, as in Iris's
    /// `TextureTransformer`.
    pub raw_textures: BTreeMap<(String, String), String>,
    /// Custom images: image name, and each image's `samplerName`, → image name
    /// (`image.<name>=<samplerName> ...`).
    pub custom_images: BTreeMap<String, String>,
}

impl ResourceContext {
    /// A context without custom textures/images and without `watershadow`.
    pub fn new(class: ProgramClass) -> Self {
        Self {
            class,
            watershadow_declared: false,
            custom_textures: BTreeMap::new(),
            raw_textures: BTreeMap::new(),
            custom_images: BTreeMap::new(),
        }
    }

    /// The same context for another program class.
    pub fn for_class(&self, class: ProgramClass) -> Self {
        Self {
            class,
            ..self.clone()
        }
    }

    /// Set [`ResourceContext::watershadow_declared`].
    pub fn with_watershadow(mut self, declared: bool) -> Self {
        self.watershadow_declared = declared;
        self
    }

    /// Set [`ResourceContext::watershadow_declared`] from the names of the opaque
    /// uniforms the program declares.
    pub fn detect_watershadow<'a>(
        mut self,
        declared_names: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        self.watershadow_declared = declared_names.into_iter().any(|n| n == "watershadow");
        self
    }

    /// Add an image custom texture (`texture.<stage>.<sampler>=<png or resource>`, or
    /// `customTexture.<sampler>` with stage [`CUSTOM_STAGE`], raw or not). Use
    /// [`ResourceContext::with_raw_texture`] for raw `texture.<stage>.<sampler>` entries.
    pub fn with_custom_texture(
        mut self,
        stage: impl Into<String>,
        sampler: impl Into<String>,
    ) -> Self {
        self.custom_textures.insert(sampler.into(), stage.into());
        self
    }

    /// Add a raw `texture.<stage>.<sampler>=<path> <TYPE> ...` entry of the program's
    /// texture stage. `dim` is the [`ResourceKind`] dimension of the texture type
    /// (`1d`, `2d`, `3d`, `2d_rect`; see [`raw_texture_dim`]). Several dimensions of one
    /// sampler can coexist; the declaration's type selects one.
    pub fn with_raw_texture(
        mut self,
        stage: impl Into<String>,
        sampler: impl Into<String>,
        dim: impl Into<String>,
    ) -> Self {
        self.raw_textures
            .insert((sampler.into(), dim.into()), stage.into());
        self
    }

    /// Add a custom image and its optional sampler name (`none` is ignored).
    pub fn with_custom_image(
        mut self,
        image: impl Into<String>,
        sampler_name: Option<&str>,
    ) -> Self {
        let image = image.into();
        if let Some(s) = sampler_name.filter(|s| !s.is_empty() && *s != "none") {
            self.custom_images.insert(s.to_string(), image.clone());
        }
        self.custom_images.insert(image.clone(), image);
        self
    }
}

/// A canonicalized opaque uniform: the binding name used in translated shaders and the
/// resource the host binds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Canonical {
    /// Canonical GLSL name (key of the binding table).
    pub name: String,
    /// What the host binds.
    pub resource: ResourceRef,
}

impl Canonical {
    /// Construct from parts.
    pub fn new(name: impl Into<String>, resource: ResourceRef) -> Self {
        Self {
            name: name.into(),
            resource,
        }
    }
}

/// A builtin sampler/image group as Iris registers it.
struct Builtin {
    canonical: String,
    /// All names registered together (custom-texture overrides match any of them, in
    /// this order).
    names: Vec<String>,
    resource: ResourceRef,
    /// Registered before `customTexture.*`, so a custom texture of the same name wins.
    custom_overridable: bool,
}

enum Lookup {
    Visible(Builtin),
    /// A builtin name the class does not bind (reads GL unit 0).
    Invisible,
    NotBuiltin,
}

fn builtin(canonical: impl Into<String>, names: &[&str], resource: ResourceRef) -> Lookup {
    Lookup::Visible(Builtin {
        canonical: canonical.into(),
        names: names.iter().map(|s| (*s).to_string()).collect(),
        resource,
        custom_overridable: false,
    })
}

/// Parse `<prefix><index>` with a decimal index below `max` and no leading zeros.
pub(crate) fn indexed(name: &str, prefix: &str, max: u32) -> Option<u32> {
    let digits = name.strip_prefix(prefix)?;
    if digits.is_empty()
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
    {
        return None;
    }
    digits.parse::<u32>().ok().filter(|&i| i < max)
}

fn color_tex(index: u32, geometry: bool) -> Lookup {
    if geometry && index < 4 {
        return Lookup::Invisible;
    }
    let canonical = format!("colortex{index}");
    let mut names = vec![canonical.clone()];
    if let Some(legacy) = LEGACY_COLOR_NAMES.get(index as usize) {
        names.push((*legacy).to_string());
    }
    Lookup::Visible(Builtin {
        canonical,
        names,
        resource: ResourceRef::ColorTex(index),
        custom_overridable: true,
    })
}

fn lookup_builtin(name: &str, class: ProgramClass, watershadow: bool) -> Lookup {
    let geometry = class.is_geometry();
    let world = |canonical: &str, names: &[&str], r: ResourceRef| {
        if geometry {
            builtin(canonical, names, r)
        } else {
            Lookup::Invisible
        }
    };
    match name {
        // Iris builds Distant Horizons programs with hasTexture=false / hasOverlay=false:
        // the albedo and overlay samplers read a constant 1x1 white texture there.
        "tex" | "texture" | "gtexture" | "u_MainSampler" if class == ProgramClass::Dh => {
            return builtin("gtexture", &ALBEDO_NAMES, ResourceRef::White);
        }
        "iris_overlay" if class == ProgramClass::Dh => {
            return builtin("iris_overlay", &["iris_overlay"], ResourceRef::White);
        }
        "tex" | "texture" | "gtexture" | "u_MainSampler" => {
            return world("gtexture", &ALBEDO_NAMES, ResourceRef::Atlas);
        }
        "lightmap" => return world("lightmap", &["lightmap"], ResourceRef::Lightmap),
        "iris_overlay" => return world("iris_overlay", &["iris_overlay"], ResourceRef::Overlay),
        "normals" => return world("normals", &["normals"], ResourceRef::Normals),
        "specular" => return world("specular", &["specular"], ResourceRef::Specular),
        "noisetex" => return builtin("noisetex", &["noisetex"], ResourceRef::Noise),
        "gdepthtex" => {
            return if geometry {
                Lookup::Invisible
            } else {
                builtin(
                    "depthtex0",
                    &["gdepthtex", "depthtex0"],
                    ResourceRef::DepthTex(0),
                )
            };
        }
        "shadow" if watershadow => {
            return builtin(
                "shadowtex1",
                &["shadowtex1", "shadow"],
                ResourceRef::ShadowTex(1),
            );
        }
        "shadow" => {
            return builtin(
                "shadowtex0",
                &["shadowtex0", "shadow"],
                ResourceRef::ShadowTex(0),
            );
        }
        // Declaring `watershadow` is what enables the rule, whatever the caller says.
        "watershadow" => {
            return builtin(
                "shadowtex0",
                &["shadowtex0", "watershadow"],
                ResourceRef::ShadowTex(0),
            );
        }
        "shadowtex0HW" => {
            return builtin(
                "shadowtex0HW",
                &["shadowtex0HW"],
                ResourceRef::ShadowTexHw(0),
            );
        }
        "shadowtex1HW" => {
            return builtin(
                "shadowtex1HW",
                &["shadowtex1HW"],
                ResourceRef::ShadowTexHw(1),
            );
        }
        "shadowcolor" => {
            return builtin(
                "shadowcolor0",
                &["shadowcolor"],
                ResourceRef::ShadowColor(0),
            );
        }
        "dhBlockAtlas" => {
            return if class == ProgramClass::Dh {
                builtin("dhBlockAtlas", &["dhBlockAtlas"], ResourceRef::DhBlockAtlas)
            } else {
                Lookup::Invisible
            };
        }
        "dhDepthTex" | "dhDepthTex0" | "dhDepthTex1" => {
            let index = u32::from(name == "dhDepthTex1");
            let names: &[&str] = if index == 0 {
                &["dhDepthTex", "dhDepthTex0"]
            } else {
                &["dhDepthTex1"]
            };
            return Lookup::Visible(Builtin {
                canonical: format!("dhDepthTex{index}"),
                names: names.iter().map(|s| (*s).to_string()).collect(),
                resource: ResourceRef::DhDepthTex(index),
                // Registered with the render targets, before custom textures.
                custom_overridable: true,
            });
        }
        _ => {}
    }
    if let Some(i) = LEGACY_COLOR_NAMES.iter().position(|l| *l == name) {
        return color_tex(i as u32, geometry);
    }
    if let Some(i) = indexed(name, "colortex", MAX_COLOR_TEX) {
        return color_tex(i, geometry);
    }
    if let Some(i) = indexed(name, "depthtex", MAX_DEPTH_TEX) {
        let names: &[&str] = if i == 0 && !geometry {
            &["gdepthtex", "depthtex0"]
        } else {
            &[name]
        };
        return builtin(name, names, ResourceRef::DepthTex(i));
    }
    if let Some(i) = indexed(name, "shadowtex", 2) {
        let names: &[&str] = match (i, watershadow) {
            (0, true) => &["shadowtex0", "watershadow"],
            (0, false) => &["shadowtex0", "shadow"],
            (_, true) => &["shadowtex1", "shadow"],
            (_, false) => &["shadowtex1"],
        };
        return builtin(name, names, ResourceRef::ShadowTex(i));
    }
    if let Some(i) = indexed(name, "shadowcolorimg", MAX_SHADOW_COLOR) {
        return builtin(name, &[], ResourceRef::ShadowColorImage(i));
    }
    if let Some(i) = indexed(name, "shadowcolor", MAX_SHADOW_COLOR) {
        return builtin(name, &[name], ResourceRef::ShadowColor(i));
    }
    if let Some(i) = indexed(name, "colorimg", MAX_COLOR_TEX) {
        return builtin(name, &[], ResourceRef::ColorImage(i));
    }
    Lookup::NotBuiltin
}

/// Whether `name` is a builtin sampler or image name in any program class.
pub fn is_builtin_resource_name(name: &str) -> bool {
    [ProgramClass::Fullscreen, ProgramClass::Dh]
        .into_iter()
        .any(|c| matches!(lookup_builtin(name, c, false), Lookup::Visible(_)))
}

/// The id stored in [`ResourceRef::CustomTexture`] for an image texture or a
/// `customTexture.*`: `<stage>.<sampler>`, matching the `stage` and `sampler` fields of
/// `sb_core::model::CustomTexture`.
pub fn custom_texture_id(stage: &str, sampler: &str) -> String {
    format!("{stage}.{sampler}")
}

/// The id stored in [`ResourceRef::CustomTexture`] for a raw
/// `texture.<stage>.<sampler>` of dimension `dim`: `<stage>.<sampler>.<dim>`, e.g.
/// `deferred.colortex6.3d`. The host picks the `sb_core::model::CustomTexture` with that
/// stage and sampler whose `TextureSource::Raw` has the matching dimensions (`2d_rect`
/// is a `TEXTURE_RECTANGLE`).
pub fn raw_texture_id(stage: &str, sampler: &str, dim: &str) -> String {
    format!("{stage}.{sampler}.{dim}")
}

/// Split a [`custom_texture_id`] or [`raw_texture_id`] into `(stage, sampler)`.
pub fn split_custom_texture_id(id: &str) -> Option<(&str, &str)> {
    parse_custom_texture_id(id).map(|(stage, sampler, _)| (stage, sampler))
}

/// Split a [`custom_texture_id`] or [`raw_texture_id`] into `(stage, sampler, raw
/// dimension)`; the dimension is `None` for image textures and `customTexture.*`.
pub fn parse_custom_texture_id(id: &str) -> Option<(&str, &str, Option<&str>)> {
    let mut parts = id.split('.');
    let stage = parts.next().filter(|s| !s.is_empty())?;
    let sampler = parts.next().filter(|s| !s.is_empty())?;
    let dim = match parts.next() {
        None => None,
        Some(d) if !d.is_empty() => Some(d),
        Some(_) => return None,
    };
    parts.next().is_none().then_some((stage, sampler, dim))
}

/// [`ResourceKind`] dimension of a raw texture type (`TEXTURE_1D` → `1d`,
/// `TEXTURE_2D` → `2d`, `TEXTURE_3D` → `3d`, `TEXTURE_RECTANGLE` → `2d_rect`;
/// case-insensitive), or `None`.
pub fn raw_texture_dim(texture_type: &str) -> Option<&'static str> {
    const TYPES: [(&str, &str); 4] = [
        ("TEXTURE_1D", "1d"),
        ("TEXTURE_2D", "2d"),
        ("TEXTURE_3D", "3d"),
        ("TEXTURE_RECTANGLE", "2d_rect"),
    ];
    TYPES
        .iter()
        .find(|(t, _)| t.eq_ignore_ascii_case(texture_type.trim()))
        .map(|(_, d)| *d)
}

/// Binding name of a raw stage texture: `sb_tex_<stage>_<sampler>_<dim>`.
pub fn raw_texture_binding_name(stage: &str, sampler: &str, dim: &str) -> String {
    format!(
        "sb_tex_{}_{}_{}",
        sanitize(stage),
        sanitize(sampler),
        sanitize(dim)
    )
}

/// Binding name of a custom texture: the sampler name itself for a
/// `customTexture.<name>` that does not shadow any builtin name, otherwise
/// `sb_tex_<stage>_<sampler>` (stage textures override builtin names per stage, so
/// they need a stage-qualified name).
pub fn custom_texture_binding_name(stage: &str, sampler: &str) -> String {
    if stage == CUSTOM_STAGE && !is_builtin_resource_name(sampler) && is_identifier(sampler) {
        return sampler.to_string();
    }
    format!("sb_tex_{}_{}", sanitize(stage), sanitize(sampler))
}

fn custom_texture(stage: &str, sampler: &str) -> Canonical {
    Canonical::new(
        custom_texture_binding_name(stage, sampler),
        ResourceRef::CustomTexture(custom_texture_id(stage, sampler)),
    )
}

fn is_identifier(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphabetic() || f == '_')
        && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Custom images and `customTexture.*` entries registered under exactly `name`.
fn custom_resource(name: &str, ctx: &ResourceContext) -> Option<Canonical> {
    if let Some(image) = ctx.custom_images.get(name) {
        return Some(Canonical::new(name, ResourceRef::Image(image.clone())));
    }
    if ctx
        .custom_textures
        .get(name)
        .is_some_and(|s| s == CUSTOM_STAGE)
    {
        return Some(custom_texture(CUSTOM_STAGE, name));
    }
    None
}

/// Canonicalize the opaque uniform `name` declared with `kind` by a program (see the
/// [module docs](self)). This is [`canonicalize`] plus the raw
/// `texture.<stage>.<name>` rule: a non-shadow sampler of exactly `name` whose
/// dimension equals a raw texture's dimension (see [`ResourceContext::raw_textures`])
/// becomes that raw texture. Iris renames such declarations to the raw texture
/// (`TextureTransformer`); declarations of other types keep the builtin resource.
///
/// Translators should always use this function, because the raw rule needs the type.
pub fn canonicalize_with_kind(name: &str, kind: &ResourceKind, ctx: &ResourceContext) -> Canonical {
    if !ctx.raw_textures.is_empty()
        && let ResourceKind::Sampler {
            dim, shadow: false, ..
        } = kind
        && let Some(stage) = ctx.raw_textures.get(&(name.to_string(), dim.clone()))
    {
        return Canonical::new(
            raw_texture_binding_name(stage, name, dim),
            ResourceRef::CustomTexture(raw_texture_id(stage, name, dim)),
        );
    }
    canonicalize(name, ctx)
}

/// Canonicalize the opaque uniform `name` declared by a program (see the
/// [module docs](self)) without its type.
///
/// Raw `texture.<stage>.<name>` entries are ignored, because they apply only to
/// declarations of a matching sampler type; use [`canonicalize_with_kind`] when the
/// type is known. Apart from that the result does not depend on the declared type: an
/// image-only name (`colorimgN`, `shadowcolorimgN`, custom images) declared as a sampler
/// still maps to the image resource, which hosts can bind as a sampled texture.
pub fn canonicalize(name: &str, ctx: &ResourceContext) -> Canonical {
    match lookup_builtin(name, ctx.class, ctx.watershadow_declared) {
        Lookup::Visible(b) => {
            // `texture.<stage>.<name>` overrides any name of the group.
            for n in &b.names {
                if let Some(stage) = ctx.custom_textures.get(n).filter(|s| *s != CUSTOM_STAGE) {
                    return custom_texture(stage, n);
                }
            }
            // `customTexture.<name>` is registered after the render targets.
            if b.custom_overridable
                && ctx
                    .custom_textures
                    .get(name)
                    .is_some_and(|s| s == CUSTOM_STAGE)
            {
                return custom_texture(CUSTOM_STAGE, name);
            }
            Canonical {
                name: b.canonical,
                resource: b.resource,
            }
        }
        Lookup::Invisible => custom_resource(name, ctx).unwrap_or_else(|| {
            let unit0 = ctx.class.default_sampler();
            canonicalize(&unit0.name, ctx)
        }),
        Lookup::NotBuiltin => custom_resource(name, ctx)
            .unwrap_or_else(|| Canonical::new(name, ResourceRef::Unknown(name.to_string()))),
    }
}

/// Canonical form of a pack shader storage block (`layout(binding = N) buffer X`),
/// bound to `bufferObject.N`.
pub fn canonicalize_ssbo(block_name: &str, binding: u32) -> Canonical {
    Canonical::new(block_name, ResourceRef::Ssbo(binding))
}

/// Canonical form of a uniform block declared by the pack.
pub fn canonicalize_ubo(block_name: &str) -> Canonical {
    Canonical::new(
        block_name,
        ResourceRef::UniformBlock(block_name.to_string()),
    )
}

/// Dimensionality suffixes of GLSL sampler/image types and their [`ResourceKind`]
/// spelling, longest first so that prefixes do not shadow longer suffixes.
const DIMS: [(&str, &str); 11] = [
    ("2DMSArray", "2d_ms_array"),
    ("CubeArray", "cube_array"),
    ("1DArray", "1d_array"),
    ("2DArray", "2d_array"),
    ("2DRect", "2d_rect"),
    ("Buffer", "buffer"),
    ("2DMS", "2d_ms"),
    ("Cube", "cube"),
    ("1D", "1d"),
    ("2D", "2d"),
    ("3D", "3d"),
];

/// Split `[i|u]<base><rest>` into (sample type, rest). `image2D` is a float image even
/// though it starts with `i`.
fn split_type<'a>(glsl_type: &'a str, base: &str) -> Option<(&'static str, &'a str)> {
    if let Some(rest) = glsl_type.strip_prefix(base) {
        return Some(("float", rest));
    }
    if let Some(rest) = glsl_type
        .strip_prefix('i')
        .and_then(|t| t.strip_prefix(base))
    {
        return Some(("int", rest));
    }
    glsl_type
        .strip_prefix('u')
        .and_then(|t| t.strip_prefix(base))
        .map(|rest| ("uint", rest))
}

/// [`ResourceKind::Sampler`] of a GLSL sampler type (`sampler2D`, `usampler3D`,
/// `sampler2DShadow`, `samplerCubeArrayShadow`, ...), or `None` for other types.
///
/// Dimensions are `1d`, `2d`, `3d`, `cube`, `2d_rect`, `1d_array`, `2d_array`,
/// `cube_array`, `buffer`, `2d_ms` and `2d_ms_array`.
pub fn sampler_kind(glsl_type: &str) -> Option<ResourceKind> {
    let (sample_type, rest) = split_type(glsl_type, "sampler")?;
    let (rest, shadow) = match rest.strip_suffix("Shadow") {
        Some(r) => (r, true),
        None => (rest, false),
    };
    let dim = DIMS.iter().find(|(s, _)| *s == rest)?.1;
    if shadow
        && (sample_type != "float" || matches!(dim, "3d" | "buffer" | "2d_ms" | "2d_ms_array"))
    {
        return None;
    }
    Some(ResourceKind::Sampler {
        dim: dim.to_string(),
        shadow,
        sample_type: sample_type.to_string(),
    })
}

/// [`ResourceKind::StorageImage`] of a GLSL image type (`image2D`, `uimage3D`, ...) with
/// its format qualifier and memory qualifiers, or `None` for other types.
pub fn image_kind(
    glsl_type: &str,
    format: Option<&str>,
    readonly: bool,
    writeonly: bool,
) -> Option<ResourceKind> {
    let (sample_type, rest) = split_type(glsl_type, "image")?;
    let dim = DIMS.iter().find(|(s, _)| *s == rest)?.1;
    Some(ResourceKind::StorageImage {
        dim: dim.to_string(),
        format: format.map(str::to_string),
        sample_type: sample_type.to_string(),
        readonly,
        writeonly,
    })
}

/// Whether `glsl_type` is a sampler or image type.
pub fn is_opaque_type(glsl_type: &str) -> bool {
    sampler_kind(glsl_type).is_some() || image_kind(glsl_type, None, false, false).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ProgramClass::*;
    use ResourceRef as R;
    use pretty_assertions::assert_eq;

    fn c(name: &str, ctx: &ResourceContext) -> (String, ResourceRef) {
        let r = canonicalize(name, ctx);
        (r.name, r.resource)
    }

    fn table(class: ProgramClass, rows: &[(&str, &str, ResourceRef)]) {
        let ctx = ResourceContext::new(class);
        for (input, name, resource) in rows {
            assert_eq!(
                c(input, &ctx),
                ((*name).to_string(), resource.clone()),
                "{input} in {class:?}"
            );
        }
    }

    #[test]
    fn fullscreen_table() {
        table(
            Fullscreen,
            &[
                ("colortex0", "colortex0", R::ColorTex(0)),
                ("gcolor", "colortex0", R::ColorTex(0)),
                ("gdepth", "colortex1", R::ColorTex(1)),
                ("gnormal", "colortex2", R::ColorTex(2)),
                ("composite", "colortex3", R::ColorTex(3)),
                ("gaux1", "colortex4", R::ColorTex(4)),
                ("gaux4", "colortex7", R::ColorTex(7)),
                ("colortex15", "colortex15", R::ColorTex(15)),
                ("colortex31", "colortex31", R::ColorTex(31)),
                ("depthtex0", "depthtex0", R::DepthTex(0)),
                ("gdepthtex", "depthtex0", R::DepthTex(0)),
                ("depthtex2", "depthtex2", R::DepthTex(2)),
                ("shadowtex0", "shadowtex0", R::ShadowTex(0)),
                ("shadowtex1", "shadowtex1", R::ShadowTex(1)),
                ("shadow", "shadowtex0", R::ShadowTex(0)),
                ("shadowtex0HW", "shadowtex0HW", R::ShadowTexHw(0)),
                ("shadowtex1HW", "shadowtex1HW", R::ShadowTexHw(1)),
                ("shadowcolor", "shadowcolor0", R::ShadowColor(0)),
                ("shadowcolor0", "shadowcolor0", R::ShadowColor(0)),
                ("shadowcolor7", "shadowcolor7", R::ShadowColor(7)),
                ("noisetex", "noisetex", R::Noise),
                ("dhDepthTex", "dhDepthTex0", R::DhDepthTex(0)),
                ("dhDepthTex0", "dhDepthTex0", R::DhDepthTex(0)),
                ("dhDepthTex1", "dhDepthTex1", R::DhDepthTex(1)),
                ("colorimg3", "colorimg3", R::ColorImage(3)),
                ("colorimg31", "colorimg31", R::ColorImage(31)),
                ("shadowcolorimg1", "shadowcolorimg1", R::ShadowColorImage(1)),
                // Not bound in fullscreen programs: GL unit 0 = colortex0.
                ("gtexture", "colortex0", R::ColorTex(0)),
                ("texture", "colortex0", R::ColorTex(0)),
                ("tex", "colortex0", R::ColorTex(0)),
                ("u_MainSampler", "colortex0", R::ColorTex(0)),
                ("lightmap", "colortex0", R::ColorTex(0)),
                ("normals", "colortex0", R::ColorTex(0)),
                ("specular", "colortex0", R::ColorTex(0)),
                ("iris_overlay", "colortex0", R::ColorTex(0)),
                ("dhBlockAtlas", "colortex0", R::ColorTex(0)),
                // Out of range / malformed indices are not builtins.
                ("colortex32", "colortex32", R::Unknown("colortex32".into())),
                ("colortex05", "colortex05", R::Unknown("colortex05".into())),
                ("depthtex3", "depthtex3", R::Unknown("depthtex3".into())),
                ("shadowtex2", "shadowtex2", R::Unknown("shadowtex2".into())),
                (
                    "shadowcolor8",
                    "shadowcolor8",
                    R::Unknown("shadowcolor8".into()),
                ),
                (
                    "shadowtex0DH",
                    "shadowtex0DH",
                    R::Unknown("shadowtex0DH".into()),
                ),
                ("colortex", "colortex", R::Unknown("colortex".into())),
                ("overlay", "overlay", R::Unknown("overlay".into())),
                ("myLut", "myLut", R::Unknown("myLut".into())),
            ],
        );
    }

    #[test]
    fn dh_albedo_and_overlay_are_white() {
        for name in ["gtexture", "texture", "tex", "u_MainSampler"] {
            table(Dh, &[(name, "gtexture", R::White)]);
        }
        table(Dh, &[("iris_overlay", "iris_overlay", R::White)]);
    }

    #[test]
    fn gbuffers_table_with_visibility_rule() {
        for class in [Gbuffers, Shadow] {
            table(
                class,
                &[
                    ("gtexture", "gtexture", R::Atlas),
                    ("texture", "gtexture", R::Atlas),
                    ("tex", "gtexture", R::Atlas),
                    ("u_MainSampler", "gtexture", R::Atlas),
                    ("iris_overlay", "iris_overlay", R::Overlay),
                ],
            );
        }
        // gbuffers/shadow/DH can only read colortex4+; lower targets fall to unit 0, which is
        // the atlas in gbuffers/shadow and the white texture in DH programs (Iris).
        for (class, unit0) in [(Gbuffers, R::Atlas), (Shadow, R::Atlas), (Dh, R::White)] {
            let rows: Vec<(&str, &str, ResourceRef)> = [
                "colortex0", "colortex1", "colortex2", "colortex3", "gcolor", "gdepth", "gnormal",
                "composite",
                // `gdepthtex` is a composite-only alias in Iris.
                "gdepthtex",
            ]
            .into_iter()
            .map(|n| (n, "gtexture", unit0.clone()))
            .collect();
            table(class, &rows);
        }
        for class in [Gbuffers, Shadow, Dh] {
            table(
                class,
                &[
                    ("lightmap", "lightmap", R::Lightmap),
                    ("normals", "normals", R::Normals),
                    ("specular", "specular", R::Specular),
                    ("colortex4", "colortex4", R::ColorTex(4)),
                    ("gaux1", "colortex4", R::ColorTex(4)),
                    ("gaux2", "colortex5", R::ColorTex(5)),
                    ("colortex12", "colortex12", R::ColorTex(12)),
                    ("depthtex0", "depthtex0", R::DepthTex(0)),
                    ("depthtex1", "depthtex1", R::DepthTex(1)),
                    ("depthtex2", "depthtex2", R::DepthTex(2)),
                    ("shadowtex0", "shadowtex0", R::ShadowTex(0)),
                    ("shadow", "shadowtex0", R::ShadowTex(0)),
                    ("shadowcolor1", "shadowcolor1", R::ShadowColor(1)),
                    ("noisetex", "noisetex", R::Noise),
                    ("dhDepthTex", "dhDepthTex0", R::DhDepthTex(0)),
                    ("colorimg0", "colorimg0", R::ColorImage(0)),
                    (
                        "unknownSampler",
                        "unknownSampler",
                        R::Unknown("unknownSampler".into()),
                    ),
                ],
            );
        }
        assert_eq!(
            c("dhBlockAtlas", &ResourceContext::new(Dh)),
            ("dhBlockAtlas".into(), R::DhBlockAtlas)
        );
        assert_eq!(
            c("dhBlockAtlas", &ResourceContext::new(Gbuffers)),
            ("gtexture".into(), R::Atlas)
        );
        // Compute programs see what fullscreen programs see.
        assert_eq!(
            c("colortex1", &ResourceContext::new(Compute)),
            ("colortex1".into(), R::ColorTex(1))
        );
        assert_eq!(
            c("gtexture", &ResourceContext::new(Compute)),
            ("colortex0".into(), R::ColorTex(0))
        );
    }

    #[test]
    fn shadow_watershadow_rule() {
        let plain = ResourceContext::new(Fullscreen);
        assert_eq!(c("shadow", &plain).1, R::ShadowTex(0));
        let ws = ResourceContext::new(Fullscreen).detect_watershadow([
            "colortex0",
            "watershadow",
            "shadow",
        ]);
        assert!(ws.watershadow_declared);
        assert_eq!(
            c("watershadow", &ws),
            ("shadowtex0".into(), R::ShadowTex(0))
        );
        assert_eq!(c("shadow", &ws), ("shadowtex1".into(), R::ShadowTex(1)));
        assert_eq!(c("shadowtex0", &ws), ("shadowtex0".into(), R::ShadowTex(0)));
        assert_eq!(c("shadowtex1", &ws), ("shadowtex1".into(), R::ShadowTex(1)));
        // `watershadow` itself always means shadowtex0.
        assert_eq!(c("watershadow", &plain).1, R::ShadowTex(0));
        assert!(
            !ResourceContext::new(Gbuffers)
                .detect_watershadow(["shadow"])
                .watershadow_declared
        );
    }

    #[test]
    fn stage_textures_override_builtin_groups() {
        // photon: texture.deferred.depthtex0 / texture.deferred.colortex6.1
        let ctx = ResourceContext::new(Fullscreen)
            .with_custom_texture("deferred", "depthtex0")
            .with_custom_texture("deferred", "gaux2");
        let (name, r) = c("depthtex0", &ctx);
        assert_eq!(name, "sb_tex_deferred_depthtex0");
        assert_eq!(r, R::CustomTexture("deferred.depthtex0".into()));
        // The override applies to every name of the group.
        assert_eq!(
            c("gdepthtex", &ctx).1,
            R::CustomTexture("deferred.depthtex0".into())
        );
        assert_eq!(
            c("colortex5", &ctx),
            (
                "sb_tex_deferred_gaux2".into(),
                R::CustomTexture("deferred.gaux2".into())
            )
        );
        assert_eq!(c("depthtex1", &ctx).1, R::DepthTex(1));
        // A stage texture for a non-builtin name is ignored by Iris.
        let ctx = ResourceContext::new(Fullscreen).with_custom_texture("composite", "myLut");
        assert_eq!(c("myLut", &ctx).1, R::Unknown("myLut".into()));
        // ... and so is one for a name the class does not bind (reads unit 0).
        let ctx = ResourceContext::new(Gbuffers).with_custom_texture("gbuffers", "colortex2");
        assert_eq!(c("colortex2", &ctx).1, R::Atlas);
        // Overriding the unit-0 group applies to names that fall back to unit 0.
        let ctx = ResourceContext::new(Fullscreen).with_custom_texture("composite", "colortex0");
        assert_eq!(
            c("colortex0", &ctx).1,
            R::CustomTexture("composite.colortex0".into())
        );
        assert_eq!(
            c("gtexture", &ctx).1,
            R::CustomTexture("composite.colortex0".into())
        );
    }

    /// Regression: raw `texture.<stage>.<name>` entries apply only to declarations of
    /// exactly `<name>` with the raw texture's sampler type (Iris `TextureTransformer`).
    /// photon declares `sampler2D colortex0` (scene color) in most composite programs and
    /// `sampler3D colortex0` (worley noise, `texture.composite.colortex0 = *.dat
    /// TEXTURE_3D ...`) in one; only the latter reads the raw texture.
    #[test]
    fn raw_stage_textures_match_name_and_type() {
        let s = |t: &str| sampler_kind(t).unwrap();
        let ctx = ResourceContext::new(Fullscreen)
            .with_raw_texture("composite", "colortex0", "3d")
            .with_raw_texture("composite", "myLut", "2d")
            .with_raw_texture("composite", "colortex9", "2d_rect");
        let raw = |name: &str, dim: &str| {
            Canonical::new(
                format!("sb_tex_composite_{name}_{dim}"),
                R::CustomTexture(format!("composite.{name}.{dim}")),
            )
        };
        assert_eq!(
            canonicalize_with_kind("colortex0", &s("sampler3D"), &ctx),
            raw("colortex0", "3d")
        );
        // Integer variants of the type match too.
        assert_eq!(
            canonicalize_with_kind("colortex0", &s("usampler3D"), &ctx),
            raw("colortex0", "3d")
        );
        // Other types keep the render target ...
        assert_eq!(
            canonicalize_with_kind("colortex0", &s("sampler2D"), &ctx),
            Canonical::new("colortex0", R::ColorTex(0))
        );
        // ... and so do aliases (the rename is by exact name).
        assert_eq!(
            canonicalize_with_kind("gcolor", &s("sampler3D"), &ctx),
            Canonical::new("colortex0", R::ColorTex(0))
        );
        // The untyped API ignores raw textures.
        assert_eq!(canonicalize("colortex0", &ctx).resource, R::ColorTex(0));
        // Raw textures also apply to non-builtin names (unlike image stage textures) ...
        assert_eq!(
            canonicalize_with_kind("myLut", &s("sampler2D"), &ctx),
            raw("myLut", "2d")
        );
        assert_eq!(
            canonicalize_with_kind("myLut", &s("sampler3D"), &ctx).resource,
            R::Unknown("myLut".into())
        );
        // ... but not to shadow samplers or images.
        assert_eq!(
            canonicalize_with_kind("myLut", &s("sampler2DShadow"), &ctx).resource,
            R::Unknown("myLut".into())
        );
        let img = image_kind("image2D", None, false, false).unwrap();
        assert_eq!(
            canonicalize_with_kind("myLut", &img, &ctx).resource,
            R::Unknown("myLut".into())
        );
        assert_eq!(
            canonicalize_with_kind("colortex9", &s("sampler2DRect"), &ctx),
            raw("colortex9", "2d_rect")
        );
        // A raw texture renames even a name the class does not bind (Iris renames
        // before registering samplers).
        let gb = ResourceContext::new(Gbuffers).with_raw_texture("gbuffers", "colortex2", "3d");
        assert_eq!(
            canonicalize_with_kind("colortex2", &s("sampler3D"), &gb),
            Canonical::new(
                "sb_tex_gbuffers_colortex2_3d",
                R::CustomTexture("gbuffers.colortex2.3d".into())
            )
        );
        assert_eq!(
            canonicalize_with_kind("colortex2", &s("sampler2D"), &gb).resource,
            R::Atlas
        );
        // An image stage texture and a raw one of the same sampler coexist.
        let both = ResourceContext::new(Fullscreen)
            .with_custom_texture("deferred", "colortex6")
            .with_raw_texture("deferred", "colortex6", "3d");
        assert_eq!(
            canonicalize_with_kind("colortex6", &s("sampler3D"), &both).resource,
            R::CustomTexture("deferred.colortex6.3d".into())
        );
        assert_eq!(
            canonicalize_with_kind("colortex6", &s("sampler2D"), &both).resource,
            R::CustomTexture("deferred.colortex6".into())
        );
    }

    #[test]
    fn raw_texture_types_and_ids() {
        assert_eq!(raw_texture_dim("TEXTURE_3D"), Some("3d"));
        assert_eq!(raw_texture_dim("texture_2d"), Some("2d"));
        assert_eq!(raw_texture_dim(" TEXTURE_1D "), Some("1d"));
        assert_eq!(raw_texture_dim("TEXTURE_RECTANGLE"), Some("2d_rect"));
        assert_eq!(raw_texture_dim("TEXTURE_CUBE_MAP"), None);
        assert_eq!(raw_texture_dim(""), None);
        assert_eq!(
            raw_texture_id("deferred", "colortex6", "3d"),
            "deferred.colortex6.3d"
        );
        assert_eq!(
            parse_custom_texture_id("deferred.colortex6.3d"),
            Some(("deferred", "colortex6", Some("3d")))
        );
        assert_eq!(
            parse_custom_texture_id("custom.blueNoise"),
            Some(("custom", "blueNoise", None))
        );
        assert_eq!(
            split_custom_texture_id("deferred.colortex6.3d"),
            Some(("deferred", "colortex6"))
        );
        for bad in ["", ".", "a.", ".b", "a.b.", "a.b.c.d", "a..c"] {
            assert_eq!(parse_custom_texture_id(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn custom_textures() {
        let ctx = ResourceContext::new(Gbuffers)
            .with_custom_texture(CUSTOM_STAGE, "perlinNoiseTex")
            .with_custom_texture(CUSTOM_STAGE, "colortex9")
            .with_custom_texture(CUSTOM_STAGE, "colortex1")
            .with_custom_texture(CUSTOM_STAGE, "noisetex");
        assert_eq!(
            c("perlinNoiseTex", &ctx),
            (
                "perlinNoiseTex".into(),
                R::CustomTexture("custom.perlinNoiseTex".into())
            )
        );
        // customTexture.* is registered after the render targets ...
        assert_eq!(
            c("colortex9", &ctx),
            (
                "sb_tex_custom_colortex9".into(),
                R::CustomTexture("custom.colortex9".into())
            )
        );
        // ... and takes names the class does not bind ...
        assert_eq!(
            c("colortex1", &ctx).1,
            R::CustomTexture("custom.colortex1".into())
        );
        // ... but noise, depth, shadow and world samplers are registered after it.
        assert_eq!(c("noisetex", &ctx).1, R::Noise);
    }

    #[test]
    fn custom_images() {
        let ctx = ResourceContext::new(Compute)
            .with_custom_image("voxel_img", Some("voxel_sampler"))
            .with_custom_image("debug", Some("none"))
            .with_custom_image("lut", None);
        assert_eq!(
            c("voxel_img", &ctx),
            ("voxel_img".into(), R::Image("voxel_img".into()))
        );
        assert_eq!(
            c("voxel_sampler", &ctx),
            ("voxel_sampler".into(), R::Image("voxel_img".into()))
        );
        assert_eq!(c("debug", &ctx).1, R::Image("debug".into()));
        assert_eq!(c("none", &ctx).1, R::Unknown("none".into()));
        assert_eq!(c("lut", &ctx).1, R::Image("lut".into()));
        assert_eq!(ctx.custom_images.len(), 4);
        // Builtin names win over images.
        let ctx = ResourceContext::new(Compute).with_custom_image("colorimg2", None);
        assert_eq!(c("colorimg2", &ctx).1, R::ColorImage(2));
    }

    #[test]
    fn binding_names_and_ids() {
        assert_eq!(
            custom_texture_binding_name("custom", "blueNoiseTex"),
            "blueNoiseTex"
        );
        assert_eq!(
            custom_texture_binding_name("custom", "lightmap"),
            "sb_tex_custom_lightmap"
        );
        assert_eq!(
            custom_texture_binding_name("composite", "colortex0"),
            "sb_tex_composite_colortex0"
        );
        assert_eq!(
            custom_texture_binding_name("my-stage", "a.b"),
            "sb_tex_my_stage_a_b"
        );
        assert_eq!(
            custom_texture_id("deferred", "depthtex0"),
            "deferred.depthtex0"
        );
        assert_eq!(
            split_custom_texture_id("deferred.depthtex0"),
            Some(("deferred", "depthtex0"))
        );
        assert_eq!(split_custom_texture_id("nodot"), None);
        assert!(is_builtin_resource_name("gtexture"));
        assert!(is_builtin_resource_name("colortex2"));
        assert!(is_builtin_resource_name("gdepthtex"));
        assert!(is_builtin_resource_name("dhBlockAtlas"));
        assert!(is_builtin_resource_name("shadowcolorimg7"));
        assert!(!is_builtin_resource_name("perlinNoiseTex"));
    }

    #[test]
    fn classes() {
        assert_eq!(
            ProgramClass::from_geometry(GeometryProgram::Terrain),
            Gbuffers
        );
        assert_eq!(
            ProgramClass::from_geometry(GeometryProgram::ShadowCutout),
            Shadow
        );
        assert_eq!(ProgramClass::from_geometry(GeometryProgram::DhShadow), Dh);
        assert_eq!(ProgramClass::from_geometry(GeometryProgram::DhWater), Dh);
        let comp = ProgramKind::GeometryCompute {
            program: GeometryProgram::Shadow,
            letter: Some('a'),
        };
        assert_eq!(ProgramClass::from_program_kind(&comp), Shadow);
        let pass = ProgramKind::Composite {
            group: sb_core::PassGroup::Deferred,
            index: 2,
        };
        assert_eq!(ProgramClass::from_program_kind(&pass), Fullscreen);
        let cs = ProgramKind::Compute {
            group: sb_core::PassGroup::Composite,
            index: 0,
            letter: None,
        };
        assert_eq!(ProgramClass::from_program_kind(&cs), Compute);
        assert_eq!(
            Gbuffers.default_sampler(),
            Canonical::new("gtexture", R::Atlas)
        );
        assert_eq!(
            Fullscreen.default_sampler(),
            Canonical::new("colortex0", R::ColorTex(0))
        );
        let ctx = ResourceContext::new(Gbuffers).with_watershadow(true);
        assert_eq!(ctx.for_class(Fullscreen).class, Fullscreen);
        assert!(ctx.for_class(Fullscreen).watershadow_declared);
    }

    #[test]
    fn opaque_types() {
        let s = |dim: &str, shadow: bool, st: &str| ResourceKind::Sampler {
            dim: dim.into(),
            shadow,
            sample_type: st.into(),
        };
        assert_eq!(sampler_kind("sampler2D"), Some(s("2d", false, "float")));
        assert_eq!(
            sampler_kind("sampler2DShadow"),
            Some(s("2d", true, "float"))
        );
        assert_eq!(sampler_kind("usampler3D"), Some(s("3d", false, "uint")));
        assert_eq!(
            sampler_kind("isampler2DArray"),
            Some(s("2d_array", false, "int"))
        );
        assert_eq!(
            sampler_kind("samplerCubeArrayShadow"),
            Some(s("cube_array", true, "float"))
        );
        assert_eq!(
            sampler_kind("sampler2DRect"),
            Some(s("2d_rect", false, "float"))
        );
        assert_eq!(
            sampler_kind("sampler2DRectShadow"),
            Some(s("2d_rect", true, "float"))
        );
        assert_eq!(
            sampler_kind("samplerBuffer"),
            Some(s("buffer", false, "float"))
        );
        assert_eq!(
            sampler_kind("sampler2DMSArray"),
            Some(s("2d_ms_array", false, "float"))
        );
        assert_eq!(sampler_kind("usampler1D"), Some(s("1d", false, "uint")));
        assert_eq!(
            sampler_kind("sampler1DArrayShadow"),
            Some(s("1d_array", true, "float"))
        );
        assert_eq!(sampler_kind("isampler2DShadow"), None);
        assert_eq!(sampler_kind("sampler3DShadow"), None);
        assert_eq!(sampler_kind("sampler4D"), None);
        assert_eq!(sampler_kind("image2D"), None);
        assert_eq!(sampler_kind("vec4"), None);
        assert_eq!(sampler_kind(""), None);
        assert_eq!(
            image_kind("uimage3D", Some("r32ui"), true, false),
            Some(ResourceKind::StorageImage {
                dim: "3d".into(),
                format: Some("r32ui".into()),
                sample_type: "uint".into(),
                readonly: true,
                writeonly: false
            })
        );
        assert!(matches!(
            image_kind("image2D", None, false, true),
            Some(ResourceKind::StorageImage {
                writeonly: true,
                ..
            })
        ));
        assert_eq!(image_kind("image2DShadow", None, false, false), None);
        assert_eq!(image_kind("sampler2D", None, false, false), None);
        assert!(is_opaque_type("iimage1D"));
        assert!(is_opaque_type("samplerCube"));
        assert!(!is_opaque_type("mat4"));
    }

    #[test]
    fn ssbo_and_ubo() {
        assert_eq!(
            canonicalize_ssbo("VoxelData", 3),
            Canonical::new("VoxelData", R::Ssbo(3))
        );
        assert_eq!(
            canonicalize_ubo("Params"),
            Canonical::new("Params", R::UniformBlock("Params".into()))
        );
    }
}
