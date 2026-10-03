//! Phase A: parse one preprocessed stage and collect what the pack-global layout,
//! binding table and the program transform need ([`StageInfo`]).

use std::collections::BTreeSet;
use std::sync::Arc;

use sb_core::model::ResourceKind;
use sb_core::{Diagnostic, Diagnostics, GlslType, ShaderStage, SourceLocation};
use sb_preprocess::{ExtensionDirective, Preprocessed, Profile};

use crate::ast::*;
use crate::consteval::{self, ConstEnv};

/// Iris/OptiFine vertex attributes a pack may declare (`in`/`attribute`).
pub const IRIS_ATTRIBUTES: &[&str] = &[
    "mc_Entity",
    "mc_midTexCoord",
    "at_tangent",
    "at_midBlock",
    "at_velocity",
    "vaPosition",
    "vaColor",
    "vaUV0",
    "vaUV1",
    "vaUV2",
    "vaNormal",
    "mc_chunkFade",
];

/// A loose (non-opaque) uniform declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct LooseUniform {
    /// Declared name.
    pub name: String,
    /// Declared type (with array length).
    pub ty: GlslType,
    /// Constant initializer components (`uniform float x = 1.0;`), see
    /// `sb_uniforms::UniformDecl::default`.
    pub default: Option<Vec<f32>>,
    /// Declaration site.
    pub location: Option<SourceLocation>,
}

/// An opaque uniform (sampler or image) declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpaqueUniform {
    /// Declared name.
    pub name: String,
    /// GLSL type (`sampler2D`, `usampler3D`, `image2D`, ...).
    pub glsl_type: String,
    /// Image format qualifier (`rgba16f`), if any.
    pub format: Option<String>,
    /// `readonly` memory qualifier.
    pub readonly: bool,
    /// `writeonly` memory qualifier.
    pub writeonly: bool,
    /// Array length, if declared as an array.
    pub array: Option<u32>,
    /// `layout(binding = N)` written by the pack, if any.
    pub binding: Option<u32>,
    /// Declaration site.
    pub location: Option<SourceLocation>,
}

impl OpaqueUniform {
    /// The resource kind (sb-uniforms [`sampler_kind`](sb_uniforms::sampler_kind) /
    /// [`image_kind`](sb_uniforms::image_kind)).
    pub fn kind(&self) -> Option<ResourceKind> {
        sb_uniforms::sampler_kind(&self.glsl_type)
            .or_else(|| sb_uniforms::image_kind(&self.glsl_type, self.format.as_deref(), self.readonly, self.writeonly))
    }

    /// Whether this is a storage image.
    pub fn is_image(&self) -> bool {
        matches!(self.kind(), Some(ResourceKind::StorageImage { .. }))
    }
}

/// A uniform or storage block the pack declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockInfo {
    /// Block name.
    pub name: String,
    /// Instance name, if any.
    pub instance: Option<String>,
    /// `layout(binding = N)` (for SSBOs: the `bufferObject.N` index).
    pub binding: Option<u32>,
    /// Declaration site.
    pub location: Option<SourceLocation>,
}

/// A stage input or output variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceVar {
    /// Variable name (or block name for interface blocks).
    pub name: String,
    /// GLSL element type text (`vec4`, struct name, block name).
    pub ty: String,
    /// Array dimensions (`None` = unsized, e.g. geometry inputs `in vec4 x[]`).
    pub array: Vec<Option<u32>>,
    /// `flat`, `smooth` or `noperspective`.
    pub interpolation: Option<String>,
    /// `layout(location = N)` written by the pack.
    pub location: Option<u32>,
    /// Declared with the legacy `attribute`/`varying` keywords.
    pub legacy: bool,
    /// `patch` qualifier (tessellation).
    pub patch: bool,
    /// Interface block (the name is the block name).
    pub block: bool,
}

/// Everything the analysis learned about one stage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StageInfo {
    /// `#version` number (110 without directive).
    pub version: u32,
    /// Compatibility code: version below 150 or the `compatibility` profile.
    pub compat: bool,
    /// Loose uniforms.
    pub loose_uniforms: Vec<LooseUniform>,
    /// Samplers and images.
    pub opaque_uniforms: Vec<OpaqueUniform>,
    /// Shader storage blocks.
    pub storage_blocks: Vec<BlockInfo>,
    /// Uniform blocks.
    pub uniform_blocks: Vec<BlockInfo>,
    /// Stage inputs (`in`, `attribute`, `varying` in consuming stages).
    pub inputs: Vec<InterfaceVar>,
    /// Stage outputs (`out`, `varying` in producing stages; fragment outputs).
    pub outputs: Vec<InterfaceVar>,
    /// Compatibility builtins referenced (`gl_Vertex`, `gl_FragData`, ...; plus
    /// `ftransform` when called).
    pub compat_builtins: BTreeSet<String>,
    /// Iris attributes declared or referenced (`mc_Entity`, `vaPosition`, ...).
    pub iris_attributes: BTreeSet<String>,
    /// Literal `gl_FragData[i]` indices.
    pub frag_data_indices: BTreeSet<u32>,
    /// `gl_FragData` indexed with a non-literal.
    pub frag_data_dynamic: bool,
    /// `gl_FragColor` is used.
    pub uses_frag_color: bool,
    /// Compute `local_size_{x,y,z}` (1 for unspecified components).
    pub local_size: Option<[u32; 3]>,
    /// Every identifier referenced (variables and called functions).
    pub identifiers: BTreeSet<String>,
    /// Builtin uniforms the stage needs without declaring them: `gl_Fog` members,
    /// `alphaTestRef` (fragment stages) and registry builtins referenced without a
    /// declaration (names Iris injects). Add them to the pack layout.
    pub implicit_uniforms: Vec<(String, GlslType)>,
    /// Names of the functions the stage defines.
    pub functions: BTreeSet<String>,
}

impl StageInfo {
    /// `UniformDecl`s for every loose and implicit uniform of the stage, for
    /// [`sb_uniforms::LayoutBuilder`].
    pub fn uniform_decls(&self, program: Option<&str>) -> Vec<sb_uniforms::UniformDecl> {
        let mut out = Vec::new();
        for u in &self.loose_uniforms {
            let mut d = sb_uniforms::UniformDecl::from_registry(u.name.clone(), u.ty);
            if let Some(def) = &u.default {
                d = d.with_default(def.clone());
            }
            if let Some(l) = &u.location {
                d = d.at(l.clone());
            }
            if let Some(p) = program {
                d = d.in_program(p);
            }
            out.push(d);
        }
        for (name, ty) in &self.implicit_uniforms {
            let mut d = sb_uniforms::UniformDecl::from_registry(name.clone(), *ty);
            if let Some(p) = program {
                d = d.in_program(p);
            }
            out.push(d);
        }
        out
    }
}

/// One analyzed (parsed) stage.
#[derive(Debug, Clone)]
pub struct AnalyzedStage {
    /// Shader stage.
    pub stage: ShaderStage,
    /// Entry file (relative to `shaders/`).
    pub file: String,
    /// `#version` number (110 without a directive).
    pub version: u32,
    /// `#version` profile.
    pub profile: Option<Profile>,
    /// Hoisted `#extension` directives.
    pub extensions: Vec<ExtensionDirective>,
    /// Collected information.
    pub info: StageInfo,
    /// Analysis warnings.
    pub diagnostics: Diagnostics,
    pub(crate) unit: Arc<TranslationUnit>,
    /// Parsed-text line (1-based, index = line - 1) -> original location.
    pub(crate) line_map: Arc<Vec<SourceLocation>>,
}

impl AnalyzedStage {
    /// Original location of a 1-based parsed-text line.
    pub fn location_of(&self, line: u32) -> Option<SourceLocation> {
        (line as usize).checked_sub(1).and_then(|i| self.line_map.get(i)).cloned()
    }

    /// Opaque uniforms with their resource kinds, for the binding table.
    pub fn resources(&self) -> Vec<(&OpaqueUniform, ResourceKind)> {
        self.info.opaque_uniforms.iter().filter_map(|o| o.kind().map(|k| (o, k))).collect()
    }
}

/// Parse and analyze one preprocessed stage. `file` is the entry file (for messages).
///
/// The parse text is the preprocessed code itself (the minimal glsl-lang lexer takes
/// the version as an option, `#extension` lines are never fed to glsl-lang), so parse
/// lines map to the original sources through `pre.line_map`. A parse error yields an
/// `xf.parse` diagnostic at the original location.
pub fn analyze(stage: ShaderStage, pre: &Preprocessed, file: &str) -> Result<AnalyzedStage, Diagnostics> {
    crate::stack::with_big_stack(|| analyze_inner(stage, pre, file))
}

fn analyze_inner(stage: ShaderStage, pre: &Preprocessed, file: &str) -> Result<AnalyzedStage, Diagnostics> {
    let version = pre.version_number();
    let profile = pre.version.and_then(|v| v.profile);
    let line_map = Arc::new(pre.line_map.clone());
    let loc = |line: u32| -> Option<SourceLocation> {
        (line as usize).checked_sub(1).and_then(|i| line_map.get(i)).cloned()
    };
    let unit = match crate::parse::parse_glsl(&pre.code, version) {
        Ok(u) => u,
        Err(e) => {
            let mut d = Diagnostics::new();
            let location = loc(e.line).or_else(|| (e.line == 0).then(|| SourceLocation::new(file, 0)));
            d.push(
                Diagnostic::error("xf.parse", format!("GLSL syntax error: {}", e.message))
                    .at_opt(location)
                    .in_stage(stage),
            );
            return Err(d);
        }
    };
    let mut diagnostics = Diagnostics::new();
    let info = collect_info(stage, version, profile, &unit, &loc, &mut diagnostics);
    Ok(AnalyzedStage {
        stage,
        file: file.to_string(),
        version,
        profile,
        extensions: pre.extensions.clone(),
        info,
        diagnostics,
        unit: Arc::new(unit),
        line_map,
    })
}

/// Evaluate array dims (`None` for unsized or non-constant).
pub(crate) fn dims(d: &[ArrayDim], env: &ConstEnv) -> Vec<Option<u32>> {
    d.iter()
        .map(|x| match x {
            ArrayDim::Unsized => None,
            ArrayDim::Sized(e) => consteval::array_len(e, env),
        })
        .collect()
}

fn layout_u32(quals: &[Qualifier], name: &str, env: &ConstEnv) -> Option<u32> {
    layout_value(quals, name).flatten().and_then(|e| consteval::eval(e, env)).and_then(|v| v.as_u32())
}

/// Image format layout qualifiers.
pub(crate) const IMAGE_FORMATS: &[&str] = &[
    "rgba32f", "rgba16f", "rg32f", "rg16f", "r11f_g11f_b10f", "r32f", "r16f", "rgba16", "rgb10_a2", "rgba8", "rg16", "rg8",
    "r16", "r8", "rgba16_snorm", "rgba8_snorm", "rg16_snorm", "rg8_snorm", "r16_snorm", "r8_snorm", "rgba32i", "rgba16i",
    "rgba8i", "rg32i", "rg16i", "rg8i", "r32i", "r16i", "r8i", "rgba32ui", "rgba16ui", "rgb10_a2ui", "rgba8ui", "rg32ui",
    "rg16ui", "rg8ui", "r32ui", "r16ui", "r8ui", "r64ui", "r64i",
];

pub(crate) fn image_format(quals: &[Qualifier]) -> Option<String> {
    quals.iter().rev().find_map(|q| match q {
        Qualifier::Layout(ids) => ids
            .iter()
            .rev()
            .find(|l| l.value.is_none() && IMAGE_FORMATS.contains(&l.name.to_ascii_lowercase().as_str()))
            .map(|l| l.name.to_ascii_lowercase()),
        _ => None,
    })
}

fn interp_name(i: Interp) -> &'static str {
    match i {
        Interp::Smooth => "smooth",
        Interp::Flat => "flat",
        Interp::NoPerspective => "noperspective",
    }
}

/// Whether a declaration with these qualifiers is a stage input in `stage`.
pub(crate) fn is_input(stage: ShaderStage, quals: &[Qualifier]) -> bool {
    let has = |s: Storage| has_storage(quals, &s);
    if has(Storage::Uniform) || has(Storage::Buffer) || has(Storage::Const) || has(Storage::Shared) {
        return false;
    }
    if has(Storage::In) || has(Storage::Attribute) {
        return true;
    }
    has(Storage::Varying) && !has(Storage::Out) && matches!(stage, ShaderStage::Fragment)
}

/// Whether a declaration with these qualifiers is a stage output in `stage`.
pub(crate) fn is_output(stage: ShaderStage, quals: &[Qualifier]) -> bool {
    let has = |s: Storage| has_storage(quals, &s);
    if has(Storage::Uniform) || has(Storage::Buffer) || has(Storage::Const) || has(Storage::In) {
        return false;
    }
    if has(Storage::Out) {
        return true;
    }
    has(Storage::Varying) && stage != ShaderStage::Fragment
}

fn collect_info(
    stage: ShaderStage,
    version: u32,
    profile: Option<Profile>,
    unit: &TranslationUnit,
    loc: &dyn Fn(u32) -> Option<SourceLocation>,
    diags: &mut Diagnostics,
) -> StageInfo {
    let env = consteval::global_consts(unit);
    let mut info = StageInfo {
        version,
        compat: version < 150 || profile == Some(Profile::Compatibility),
        ..StageInfo::default()
    };
    let mut declared: BTreeSet<String> = BTreeSet::new();

    for item in &unit.items {
        let l = item.line;
        match &item.kind {
            ItemKind::Decl(d) => {
                let quals = &d.ty.quals;
                for v in &d.vars {
                    declared.insert(v.name.clone());
                }
                if let TypeBase::Struct(s) = &d.ty.ty.base
                    && let Some(n) = &s.name
                {
                    declared.insert(n.clone());
                }
                if has_storage(quals, &Storage::Uniform) {
                    collect_uniform(d, &env, l, loc, &mut info, diags);
                } else if is_input(stage, quals) || is_output(stage, quals) {
                    let input = is_input(stage, quals);
                    for v in d.vars.iter().filter(|v| !v.name.starts_with("gl_")) {
                        // Outermost first: `vec4[2] x[]` is `vec4 x[][2]`.
                        let mut array = dims(&v.array, &env);
                        array.extend(dims(&d.ty.ty.array, &env));
                        let var = InterfaceVar {
                            name: v.name.clone(),
                            ty: crate::print::type_spec(&TypeSpec { base: d.ty.ty.base.clone(), array: Vec::new() }),
                            array,
                            interpolation: interpolation(quals).map(|i| interp_name(i).to_string()),
                            location: layout_u32(quals, "location", &env),
                            legacy: has_storage(quals, &Storage::Attribute) || has_storage(quals, &Storage::Varying),
                            patch: has_storage(quals, &Storage::Patch),
                            block: false,
                        };
                        if input { info.inputs.push(var) } else { info.outputs.push(var) }
                    }
                }
            }
            ItemKind::Block(b) => {
                if b.instance.is_none() {
                    for f in &b.fields {
                        for (n, _) in &f.names {
                            declared.insert(n.clone());
                        }
                    }
                } else if let Some((n, _)) = &b.instance {
                    declared.insert(n.clone());
                }
                let bi = BlockInfo {
                    name: b.name.clone(),
                    instance: b.instance.as_ref().map(|i| i.0.clone()),
                    binding: layout_u32(&b.quals, "binding", &env),
                    location: loc(l),
                };
                if has_storage(&b.quals, &Storage::Buffer) {
                    info.storage_blocks.push(bi);
                } else if has_storage(&b.quals, &Storage::Uniform) {
                    info.uniform_blocks.push(bi);
                } else if is_input(stage, &b.quals) || is_output(stage, &b.quals) {
                    let var = InterfaceVar {
                        name: b.name.clone(),
                        ty: b.name.clone(),
                        array: b.instance.as_ref().map(|(_, d)| dims(d, &env)).unwrap_or_default(),
                        interpolation: interpolation(&b.quals).map(|i| interp_name(i).to_string()),
                        location: layout_u32(&b.quals, "location", &env),
                        legacy: false,
                        patch: has_storage(&b.quals, &Storage::Patch),
                        block: true,
                    };
                    if is_input(stage, &b.quals) { info.inputs.push(var) } else { info.outputs.push(var) }
                }
            }
            ItemKind::Function(f) => {
                declared.insert(f.proto.name.clone());
                info.functions.insert(f.proto.name.clone());
                for p in &f.proto.params {
                    if let Some(n) = &p.name {
                        declared.insert(n.clone());
                    }
                }
                for s in &f.body {
                    s.walk_stmts(&mut |s| {
                        match &s.kind {
                            StmtKind::Decl(d) => {
                                for v in &d.vars {
                                    declared.insert(v.name.clone());
                                }
                            }
                            StmtKind::While { cond: Condition::Decl { name, .. }, .. } => {
                                declared.insert(name.clone());
                            }
                            StmtKind::For { cond: Some(Condition::Decl { name, .. }), .. } => {
                                declared.insert(name.clone());
                            }
                            _ => {}
                        }
                    });
                }
            }
            ItemKind::Prototype(p) => {
                declared.insert(p.name.clone());
            }
            ItemKind::QualifierOnly(q) => {
                if stage == ShaderStage::Compute && has_storage(q, &Storage::In) {
                    let mut ls = info.local_size.unwrap_or([1, 1, 1]);
                    for (i, axis) in ["local_size_x", "local_size_y", "local_size_z"].iter().enumerate() {
                        if let Some(v) = layout_u32(q, axis, &env) {
                            ls[i] = v;
                        }
                    }
                    info.local_size = Some(ls);
                }
            }
            ItemKind::Precision(..) | ItemKind::Invariant(_) => {}
        }
    }
    if stage == ShaderStage::Compute && info.local_size.is_none() {
        info.local_size = Some([1, 1, 1]);
    }

    // Expressions: identifiers, compat builtins, gl_FragData usage.
    unit.walk_exprs(&mut |e| {
        match e {
            Expr::Ident(n) => {
                info.identifiers.insert(n.clone());
                if n.starts_with("gl_") {
                    info.compat_builtins.insert(n.clone());
                }
                if n == "gl_FragColor" {
                    info.uses_frag_color = true;
                }
            }
            Expr::Call(Callee::Name(n), args) => {
                info.identifiers.insert(n.clone());
                if n == "ftransform" && args.is_empty() {
                    info.compat_builtins.insert(n.clone());
                }
            }
            Expr::Index(base, idx) if base.as_ident() == Some("gl_FragData") => match idx.as_ref() {
                Expr::Int(i) if *i >= 0 => {
                    info.frag_data_indices.insert(*i as u32);
                }
                Expr::UInt(i) => {
                    info.frag_data_indices.insert(*i);
                }
                _ => info.frag_data_dynamic = true,
            },
            _ => {}
        }
        Walk::Children
    });

    for a in IRIS_ATTRIBUTES {
        if declared.contains(*a) || info.identifiers.contains(*a) {
            info.iris_attributes.insert((*a).to_string());
        }
    }

    // Implicit uniforms.
    let mut implicit: Vec<(String, GlslType)> = Vec::new();
    let add = |name: &str, implicit: &mut Vec<(String, GlslType)>| {
        if let Some(b) = sb_uniforms::get(name)
            && !implicit.iter().any(|(n, _)| n == name)
        {
            implicit.push((name.to_string(), b.ty));
        }
    };
    if info.compat_builtins.contains("gl_Fog") {
        for n in ["sb_FogColor", "fogDensity", "fogStart", "fogEnd", "fogScale"] {
            add(n, &mut implicit);
        }
    }
    if stage == ShaderStage::Fragment {
        add("alphaTestRef", &mut implicit);
    }
    let loose: BTreeSet<&str> = info.loose_uniforms.iter().map(|u| u.name.as_str()).collect();
    for id in &info.identifiers {
        if !declared.contains(id) && !loose.contains(id.as_str()) && sb_uniforms::is_builtin(id) && !crate::names::is_core_profile_name(id) {
            add(id, &mut implicit);
        }
    }
    info.implicit_uniforms = implicit;
    info
}

fn collect_uniform(
    d: &Declaration,
    env: &ConstEnv,
    line: u32,
    loc: &dyn Fn(u32) -> Option<SourceLocation>,
    info: &mut StageInfo,
    diags: &mut Diagnostics,
) {
    let quals = &d.ty.quals;
    let Some(type_name) = d.ty.ty.name() else {
        diags.push(
            Diagnostic::warning("xf.uniform-struct", "struct-typed loose uniforms are not supported; the uniform reads zero")
                .at_opt(loc(line)),
        );
        return;
    };
    for v in &d.vars {
        let mut array = dims(&d.ty.ty.array, env);
        array.extend(dims(&v.array, env));
        if sb_uniforms::is_opaque_type(type_name) {
            if array.len() > 1 || array.iter().any(Option::is_none) {
                diags.push(
                    Diagnostic::warning("xf.opaque-array", format!("opaque uniform `{}` has an unsupported array shape", v.name))
                        .at_opt(loc(line)),
                );
            }
            info.opaque_uniforms.push(OpaqueUniform {
                name: v.name.clone(),
                glsl_type: type_name.to_string(),
                format: image_format(quals),
                readonly: has_storage(quals, &Storage::ReadOnly),
                writeonly: has_storage(quals, &Storage::WriteOnly),
                array: array.first().copied().flatten(),
                binding: layout_u32(quals, "binding", env),
                location: loc(line),
            });
            continue;
        }
        let Some(mut ty) = GlslType::parse(type_name) else {
            diags.push(
                Diagnostic::warning(
                    "xf.uniform-struct",
                    format!("uniform `{}` has unsupported type `{type_name}`; it reads zero", v.name),
                )
                .at_opt(loc(line)),
            );
            continue;
        };
        match array.as_slice() {
            [] => {}
            [Some(n)] => ty = ty.with_array(*n),
            _ => {
                diags.push(
                    Diagnostic::warning("xf.uniform-array", format!("uniform `{}` has an unsupported array shape", v.name))
                        .at_opt(loc(line)),
                );
                continue;
            }
        }
        let default = match &v.init {
            Some(Init::Expr(e)) => consteval::eval(e, env).and_then(|c| {
                let expected = sb_uniforms::component_count(ty) as usize;
                (c.comps.len() == expected).then(|| c.comps.iter().map(|x| *x as f32).collect())
            }),
            _ => None,
        };
        info.loose_uniforms.push(LooseUniform { name: v.name.clone(), ty, default, location: loc(line) });
    }
}
