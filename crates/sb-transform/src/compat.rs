//! Compatibility builtins (`gl_Vertex`, matrices, fixed-function varyings, `gl_Fog`,
//! `ftransform()`), Iris attributes and the draw-profile pieces implementing them.

use std::collections::{BTreeSet, HashMap};

use sb_core::model::OutputTarget;
use sb_core::{GlslType, ShaderStage};

use crate::ast::*;
use crate::program::{Ctx, Section, StageWork};
use crate::profiles::SEMANTIC_KEYS;

/// Generated global of each semantic key.
pub(crate) fn semantic_global(key: &str) -> &'static str {
    match key {
        "position" => "sb_gl_Vertex",
        "color" => "sb_gl_Color",
        "uv0" => "sb_gl_MultiTexCoord0",
        "lightmap" => "sb_gl_MultiTexCoord1",
        "normal" => "sb_gl_Normal",
        "entity" => "sb_mc_Entity",
        "mid_tex_coord" => "sb_mc_midTexCoord",
        "tangent" => "sb_at_tangent",
        "mid_block" => "sb_at_midBlock",
        "velocity" => "sb_at_velocity",
        "overlay" => "sb_vaUV1",
        "model_view" => "sb_ModelView",
        "projection" => "sb_Projection",
        "texture_matrix" => "sb_TextureMatrix",
        "lightmap_matrix" => "sb_LightmapMatrix",
        "normal_matrix" => "sb_NormalMatrix",
        "chunk_offset" => "sb_ChunkOffset",
        _ => "sb_unknown_semantic",
    }
}

/// Semantics available in every stage (they do not depend on vertex attributes).
const MATRIX_SEMANTICS: &[&str] = &["model_view", "projection", "texture_matrix", "lightmap_matrix", "normal_matrix", "chunk_offset"];

/// Whether semantic `key` is a per-draw (matrix-like) semantic rather than a vertex
/// attribute.
pub(crate) fn is_matrix_semantic(key: &str) -> bool {
    MATRIX_SEMANTICS.contains(&key)
}

/// Matrix bases for derived `*Inverse`/`*Transpose`/`*InverseTranspose` globals.
const MATRIX_BASES: &[(&str, &str, &str)] = &[
    // (gl name stem, generated base, type)
    ("gl_ModelViewMatrix", "sb_ModelView", "mat4"),
    ("gl_ProjectionMatrix", "sb_Projection", "mat4"),
    ("gl_ModelViewProjectionMatrix", "sb_ModelViewProjection", "mat4"),
];

/// Replacement of a compatibility builtin identifier in `stage`.
fn replacement(name: &str, stage: ShaderStage) -> Option<&'static str> {
    use ShaderStage as S;
    let vs = stage == S::Vertex;
    let fs = stage == S::Fragment;
    let gs = stage == S::Geometry;
    Some(match name {
        "gl_Vertex" if vs => "sb_gl_Vertex",
        "gl_Color" if vs => "sb_gl_Color",
        "gl_SecondaryColor" if vs => "sb_gl_SecondaryColor",
        "gl_Normal" if vs => "sb_gl_Normal",
        "gl_MultiTexCoord0" if vs => "sb_gl_MultiTexCoord0",
        "gl_MultiTexCoord1" | "gl_MultiTexCoord2" if vs => "sb_gl_MultiTexCoord1",
        "gl_MultiTexCoord3" if vs => "sb_mc_midTexCoord",
        "gl_MultiTexCoord4" | "gl_MultiTexCoord5" | "gl_MultiTexCoord6" | "gl_MultiTexCoord7" if vs => "sb_gl_MultiTexCoordZero",
        "gl_FogCoord" if vs => "sb_gl_FogCoord",
        "gl_ModelViewMatrix" => "sb_ModelView",
        "gl_ProjectionMatrix" => "sb_Projection",
        "gl_ModelViewProjectionMatrix" => "sb_ModelViewProjection",
        "gl_NormalMatrix" => "sb_NormalMatrix",
        "gl_ModelViewMatrixInverse" => "sb_ModelViewInverse",
        "gl_ModelViewMatrixTranspose" => "sb_ModelViewTranspose",
        "gl_ModelViewMatrixInverseTranspose" => "sb_ModelViewInverseTranspose",
        "gl_ProjectionMatrixInverse" => "sb_ProjectionInverse",
        "gl_ProjectionMatrixTranspose" => "sb_ProjectionTranspose",
        "gl_ProjectionMatrixInverseTranspose" => "sb_ProjectionInverseTranspose",
        "gl_ModelViewProjectionMatrixInverse" => "sb_ModelViewProjectionInverse",
        "gl_ModelViewProjectionMatrixTranspose" => "sb_ModelViewProjectionTranspose",
        "gl_ModelViewProjectionMatrixInverseTranspose" => "sb_ModelViewProjectionInverseTranspose",
        "gl_Fog" => "sb_gl_Fog",
        "gl_VertexID" => "gl_VertexIndex",
        "gl_InstanceID" => "gl_InstanceIndex",
        "gl_FrontColor" if !fs => "sb_v_Color",
        "gl_FrontSecondaryColor" if !fs => "sb_v_SecondaryColor",
        // Two-sided vertex color is never enabled (OptiFine, Iris): the fragment `gl_Color`
        // is the front color, whatever the back color written.
        "gl_BackColor" if !fs => "sb_BackColor",
        "gl_BackSecondaryColor" if !fs => "sb_BackSecondaryColor",
        "gl_TexCoord" => "sb_v_TexCoord",
        "gl_FogFragCoord" => "sb_v_FogFragCoord",
        "gl_Color" if fs => "sb_v_Color",
        "gl_SecondaryColor" if fs => "sb_v_SecondaryColor",
        "gl_FrontColorIn" | "gl_BackColorIn" if gs => "sb_vin_Color",
        "gl_FrontSecondaryColorIn" | "gl_BackSecondaryColorIn" if gs => "sb_vin_SecondaryColor",
        "gl_TexCoordIn" if gs => "sb_vin_TexCoord",
        "gl_FogFragCoordIn" if gs => "sb_vin_FogFragCoord",
        "gl_MaxTextureCoords" => "sb_MaxTextureCoords",
        _ => return None,
    })
}

/// `gl_TextureMatrix*[i]` replacement for a literal index.
fn texture_matrix(base: &str, index: Option<i64>) -> Option<String> {
    let variant = base.strip_prefix("gl_TextureMatrix")?;
    if !matches!(variant, "" | "Inverse" | "Transpose" | "InverseTranspose") {
        return None;
    }
    Some(match index {
        Some(0) => format!("sb_TextureMatrix{variant}"),
        Some(1 | 2) => format!("sb_LightmapMatrix{variant}"),
        Some(_) => "mat4(1.0)".to_string(),
        None => format!("sb_TextureMatrices{variant}"),
    })
}

/// Rewrite compatibility builtins in the pack code of `w`.
pub(crate) fn rewrite_builtins(w: &mut StageWork, ctx: &Ctx) {
    let stage = w.stage;
    let draw_parameters = ctx.opts.draw_parameters && stage == ShaderStage::Vertex;
    let mut base_instance = false;
    // Remove `gl_ClipVertex = ...;` statements.
    for f in w.unit.functions_mut() {
        for s in &mut f.body {
            s.walk_stmts_mut(&mut |s| {
                if let StmtKind::Expr(Expr::Assign(lhs, _, _)) = &s.kind
                    && root_ident(lhs) == Some("gl_ClipVertex")
                {
                    s.kind = StmtKind::Empty;
                }
            });
        }
    }
    let ftransform = crate::parse::parse_expr("sb_Projection * (sb_ModelView * sb_gl_Vertex)").unwrap_or(Expr::Int(0));
    let mut unknown: BTreeSet<String> = BTreeSet::new();
    w.unit.walk_exprs_mut(&mut |e| {
        match e {
            Expr::Index(base, idx) => {
                if let Some(b) = base.as_ident()
                    && b.starts_with("gl_TextureMatrix")
                {
                    let lit = match idx.as_ref() {
                        Expr::Int(i) => Some(i64::from(*i)),
                        Expr::UInt(i) => Some(i64::from(*i)),
                        _ => None,
                    };
                    if let Some(r) = texture_matrix(b, lit) {
                        if lit.is_some() {
                            *e = if r.starts_with("mat4(") { Expr::raw(r) } else { Expr::Ident(r) };
                            return Walk::Skip;
                        }
                        **base = Expr::Ident(r);
                    }
                }
                Walk::Children
            }
            Expr::Ident(n) if n == "gl_InstanceID" && draw_parameters => {
                *e = Expr::raw("(gl_InstanceIndex - gl_BaseInstance)");
                base_instance = true;
                Walk::Skip
            }
            Expr::Ident(n) if n.starts_with("gl_") => {
                if let Some(r) = replacement(n, stage) {
                    *n = r.to_string();
                } else if let Some(r) = texture_matrix(n, None) {
                    *n = r;
                } else if is_unsupported_compat(n) {
                    unknown.insert(n.clone());
                }
                Walk::Children
            }
            Expr::Call(Callee::Name(n), args) if n == "ftransform" && args.is_empty() => {
                *e = ftransform.clone();
                Walk::Skip
            }
            _ => Walk::Children,
        }
    });
    for n in unknown {
        w.error("xf.unsupported-builtin", format!("the fixed-function builtin `{n}` is not supported"), 0);
    }
    if base_instance {
        w.extensions.insert("GL_ARB_shader_draw_parameters".into());
    }
}

fn is_unsupported_compat(n: &str) -> bool {
    n.starts_with("gl_LightSource")
        || n.starts_with("gl_LightModel")
        || n.starts_with("gl_FrontMaterial")
        || n.starts_with("gl_BackMaterial")
        || n.starts_with("gl_FrontLightProduct")
        || n.starts_with("gl_BackLightProduct")
        || n.starts_with("gl_FrontLightModelProduct")
        || n.starts_with("gl_BackLightModelProduct")
        || n.starts_with("gl_TextureEnvColor")
        || n.starts_with("gl_EyePlane")
        || n.starts_with("gl_ObjectPlane")
        || n == "gl_ClipPlane"
        || n == "gl_Point"
}

/// The identifier at the root of an lvalue (`a.b[1].c` -> `a`).
pub(crate) fn root_ident(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n) => Some(n),
        Expr::Field(a, _) | Expr::Index(a, _) => root_ident(a),
        _ => None,
    }
}

/// Every identifier referenced (as a variable or callee) in the pack code.
pub(crate) fn referenced(unit: &TranslationUnit) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    unit.walk_exprs(&mut |e| {
        match e {
            Expr::Ident(n) => {
                out.insert(n.clone());
            }
            Expr::Call(Callee::Name(n), _) => {
                out.insert(n.clone());
            }
            _ => {}
        }
        Walk::Children
    });
    for item in &unit.items {
        if let ItemKind::Invariant(n) = &item.kind {
            out.insert(n.clone());
        }
    }
    out
}

/// Declare the fixed-function varyings the stage uses.
pub(crate) fn fixed_function_varyings(w: &mut StageWork, ctx: &Ctx) {
    let used = referenced(&w.unit);
    let n = ctx.texcoord_count;
    let decls: &[(&str, &str, &str)] = &[
        ("sb_v_Color", "vec4", ""),
        ("sb_v_SecondaryColor", "vec4", ""),
        ("sb_v_TexCoord", "vec4", "T"),
        ("sb_v_FogFragCoord", "float", ""),
    ];
    for (name, ty, arr) in decls {
        let array = if arr.is_empty() { String::new() } else { format!("[{n}]") };
        if used.contains(*name) {
            match w.stage {
                ShaderStage::Fragment => w.add_iface(&format!("in {ty} {name}{array};")),
                ShaderStage::Compute => {}
                ShaderStage::TessControl => w.add_iface(&format!("out {ty} {name}[]{array};")),
                _ => w.add_iface(&format!("out {ty} {name}{array};")),
            }
        }
        let vin = name.replace("sb_v_", "sb_vin_");
        if used.contains(&vin) && matches!(w.stage, ShaderStage::Geometry | ShaderStage::TessControl | ShaderStage::TessEval) {
            w.add_iface(&format!("in {ty} {vin}[]{array};"));
        }
    }
}

/// Conversion of a semantic expression of type `from` to the declared type `to`
/// (attributes: a wider vector is padded with zeros and 1 in the `w` slot, as GL
/// attribute fetch and Iris's `patchIntegerAttribute` do).
pub(crate) fn convert(expr: &str, from: GlslType, to: GlslType) -> String {
    convert_padded(expr, from, to, true)
}

/// Conversion of a varying of type `from` to the consumer's type `to` (Iris
/// `transformGrouped` rule c: `T(tmp, vec4(0))`, i.e. padding with zeros, `w`
/// included).
pub(crate) fn convert_varying(expr: &str, from: GlslType, to: GlslType) -> String {
    convert_padded(expr, from, to, false)
}

fn convert_padded(expr: &str, from: GlslType, to: GlslType, w_one: bool) -> String {
    if from == to {
        return expr.to_string();
    }
    let to_name = to.glsl_name();
    if to.is_matrix() || from.is_matrix() {
        return format!("{to_name}({expr})");
    }
    let swz = ["x", "xy", "xyz", "xyzw"];
    let (fr, tr) = (usize::from(from.rows), usize::from(to.rows));
    if tr <= fr {
        if tr == fr {
            return format!("{to_name}({expr})");
        }
        return format!("{to_name}(({expr}).{})", swz[tr - 1]);
    }
    // Pad: zeros, and 1 in the w slot.
    let scalar = GlslType::scalar(to.scalar).glsl_name();
    let mut parts = vec![format!("({expr})")];
    for i in fr..tr {
        parts.push(format!("{scalar}({})", if i == 3 && w_one { "1" } else { "0" }));
    }
    format!("{to_name}({})", parts.join(", "))
}

/// Semantic key and type an Iris attribute reads.
fn attribute_semantic(name: &str) -> Option<(&'static str, GlslType)> {
    Some(match name {
        "mc_Entity" => ("entity", GlslType::VEC4),
        "mc_midTexCoord" => ("mid_tex_coord", GlslType::VEC4),
        "at_tangent" => ("tangent", GlslType::VEC4),
        "at_midBlock" => ("mid_block", GlslType::VEC4),
        "at_velocity" => ("velocity", GlslType::VEC3),
        "vaPosition" => ("position", GlslType::VEC3),
        "vaColor" => ("color", GlslType::VEC4),
        "vaUV0" => ("uv0", GlslType::VEC4),
        "vaUV1" => ("overlay", GlslType::IVEC2),
        "vaUV2" => ("lightmap", GlslType::VEC4),
        "vaNormal" => ("normal", GlslType::VEC3),
        _ => return None,
    })
}

/// Vertex stage: replace pack-declared Iris attributes (`mc_Entity`, `vaPosition`, ...)
/// and other vertex inputs. Attributes become globals of the declared type initialized
/// from the semantics; inputs matching a profile input (same name and type) are left
/// to the profile; anything else becomes a zero global with a warning.
pub(crate) fn vertex_inputs(w: &mut StageWork, ctx: &Ctx) {
    if w.stage != ShaderStage::Vertex {
        return;
    }
    let env = crate::consteval::global_consts(&w.unit);
    let taken = crate::rewrite::take_global_decls(&mut w.unit, |d, _| crate::analyze::is_input(ShaderStage::Vertex, &d.ty.quals));
    for (d, line) in taken {
        let Some(v) = d.vars.first() else { continue };
        let name = v.name.clone();
        let ty_name = d.ty.ty.name().unwrap_or("").to_string();
        let ty = GlslType::parse(&ty_name);
        let has_array = !d.ty.ty.array.is_empty() || !v.array.is_empty();
        if let (Some((key, from)), Some(to), false) = (attribute_semantic(&name), ty, has_array) {
            let src = semantic_global(key);
            let expr = if name == "vaPosition" { format!("{src}.xyz - sb_ChunkOffset") } else { src.to_string() };
            let init = convert(&expr, from, to);
            w.piece(Section::Derived, &[&name], format!("{ty_name} {name} = {init};"));
            continue;
        }
        if name == "mc_chunkFade" && !has_array {
            if !ctx.profile_globals.contains_key("mc_chunkFade") {
                w.piece(Section::Derived, &[&name], format!("{ty_name} mc_chunkFade = {ty_name}(-1.0);"));
            }
            continue;
        }
        if let Some(pi) = ctx.profile.inputs.iter().find(|i| i.name == name)
            && pi.ty == ty_name
            && !has_array
        {
            continue;
        }
        // Unknown attribute: zero-initialized global.
        let dims = crate::analyze::dims(&v.array, &env);
        w.warn(
            "xf.unknown-attribute",
            format!("vertex input `{name}` is not provided by draw profile `{}`; it reads zero", ctx.profile.name),
            line,
        );
        let decl = Declaration {
            ty: FullType { quals: Vec::new(), ty: d.ty.ty.clone() },
            vars: vec![Declarator { name: name.clone(), array: v.array.clone(), init: None }],
        };
        let mut text = crate::print::declaration(&decl);
        if dims.is_empty()
            && let Some(t) = ty
            && d.ty.ty.array.is_empty()
        {
            // GL's initial generic attribute value is (0, 0, 0, 1): what an attribute
            // without a vertex buffer reads (narrower types read its first components).
            if t.rows == 4 && t.cols == 1 {
                text.push_str(&format!(" = {ty_name}(0, 0, 0, 1)"));
            } else {
                text.push_str(&format!(" = {ty_name}(0)"));
            }
        }
        text.push(';');
        w.piece(Section::Derived, &[&name], text);
    }
    // Iris declares its attributes itself when a (core-profile) program uses them
    // without a declaration.
    for (name, ty) in [
        ("vaPosition", "vec3"),
        ("vaColor", "vec4"),
        ("vaUV0", "vec2"),
        ("vaUV1", "ivec2"),
        ("vaUV2", "ivec2"),
        ("vaNormal", "vec3"),
        ("mc_Entity", "vec4"),
        ("mc_midTexCoord", "vec4"),
        ("at_tangent", "vec4"),
        ("at_midBlock", "vec4"),
        ("at_velocity", "vec3"),
    ] {
        if w.has_piece(name) {
            continue;
        }
        let (key, from) = attribute_semantic(name).unwrap_or(("position", GlslType::VEC4));
        let src = semantic_global(key);
        let expr = if name == "vaPosition" { format!("{src}.xyz - sb_ChunkOffset") } else { src.to_string() };
        let to = GlslType::parse(ty).unwrap_or(GlslType::VEC4);
        w.piece(Section::Derived, &[name], format!("{ty} {name} = {};", convert(&expr, from, to)));
    }
    if !w.has_piece("mc_chunkFade") && !ctx.profile_globals.contains_key("mc_chunkFade") {
        w.piece(Section::Derived, &["mc_chunkFade"], "float mc_chunkFade = -1.0;");
    }
}

fn depends_on_vertex_only(expr: &str, ctx: &Ctx, vertex_helpers: &BTreeSet<String>) -> bool {
    crate::text::identifiers(expr).any(|id| ctx.profile.inputs.iter().any(|i| i.name == id) || vertex_helpers.contains(id))
}

/// Add the profile's pieces for this stage: host blocks, samplers, inputs, constants,
/// helper functions, semantic globals and their derived matrices, profile globals,
/// `gl_Fog`.
pub(crate) fn profile_pieces(w: &mut StageWork, ctx: &Ctx) {
    let p = ctx.profile;
    let vulkan = ctx.opts.target == OutputTarget::Vulkan;
    let stage = w.stage;
    let compute = stage == ShaderStage::Compute;
    // Host blocks.
    for b in p.blocks.iter().filter(|_| !compute) {
        let layout = if vulkan {
            match host_block_binding(ctx, &b.name) {
                Some((set, binding)) => format!("layout(std140, set = {set}, binding = {binding})"),
                None => {
                    w.error(
                        "xf.unbound-resource",
                        format!("host uniform block `{}` of profile `{}` has no binding (register the profile's resources)", b.name, p.name),
                        0,
                    );
                    "layout(std140)".to_string()
                }
            }
        } else {
            "layout(std140)".to_string()
        };
        w.piece(Section::HostBlocks, &[&b.instance], format!("{layout} uniform {} {{ {} }} {};", b.name, b.members, b.instance));
    }
    // Push constants: one anonymous block, members referenced unqualified (declared in
    // every stage that references one of them).
    if !compute && !p.push_constants.trim().is_empty() {
        match crate::profiles::push_constant_members(&p.push_constants) {
            Ok(members) => {
                let provides: Vec<&str> = members.iter().map(String::as_str).collect();
                w.piece(
                    Section::HostBlocks,
                    &provides,
                    format!("layout(push_constant) uniform {} {{ {} }};", crate::profiles::PUSH_CONSTANT_BLOCK, p.push_constants.trim()),
                );
            }
            Err(e) => w.error("xf.profile", format!("profile `{}`: {e}", p.name), 0),
        }
    }
    // Host samplers.
    for s in p.samplers.iter().filter(|_| !compute) {
        let decl = if vulkan {
            match host_sampler_binding(ctx, s) {
                Some((set, binding)) => format!("layout(set = {set}, binding = {binding}) uniform {} {};", s.ty, s.name),
                None => {
                    w.error(
                        "xf.unbound-resource",
                        format!("host sampler `{}` of profile `{}` has no binding (register the profile's resources)", s.name, p.name),
                        0,
                    );
                    format!("uniform {} {};", s.ty, s.name)
                }
            }
        } else {
            format!("uniform {} {};", s.ty, s.name)
        };
        w.piece(Section::Resources, &[&s.name], decl);
    }
    // Vertex inputs.
    if stage == ShaderStage::Vertex {
        for i in &p.inputs {
            w.piece(Section::Inputs, &[&i.name], format!("layout(location = {}) in {} {};", i.location, i.ty, i.name));
        }
    }
    // Helper code.
    let code = match stage {
        _ if compute => "",
        ShaderStage::Vertex => p.code_vertex.as_str(),
        ShaderStage::Fragment => p.code_fragment.as_str(),
        _ => "",
    };
    let vertex_helpers: BTreeSet<String> = crate::text::defined_functions(&p.code_vertex).into_iter().collect();
    let mut constants: BTreeSet<String> = BTreeSet::new();
    if !code.trim().is_empty() {
        match crate::parse::parse_glsl(code, 460) {
            Ok(unit) => {
                for item in unit.items {
                    let provides: Vec<String> = match &item.kind {
                        ItemKind::Function(f) => vec![f.proto.name.clone()],
                        ItemKind::Prototype(f) => vec![f.name.clone()],
                        ItemKind::Decl(d) => d.vars.iter().map(|v| v.name.clone()).collect(),
                        _ => Vec::new(),
                    };
                    let mut pr = crate::print::Printer::new();
                    pr.item(&item);
                    let text = pr.finish().0;
                    for id in crate::text::identifiers(&text) {
                        if id.starts_with("SB_") && id.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
                            constants.insert(id.to_string());
                        }
                    }
                    w.pieces.push(crate::program::Piece { provides, text, section: Section::Helpers });
                }
            }
            Err(e) => w.error("xf.profile", format!("helper code of profile `{}` does not parse: {}", p.name, e.message), 0),
        }
    }
    for c in constants {
        let v = ctx.opts.profile_constants.get(&c).copied().unwrap_or(-1);
        w.piece(Section::Constants, &[&c], format!("const int {c} = {v};"));
    }
    // Semantic globals.
    for (key, ty) in SEMANTIC_KEYS {
        let global = semantic_global(key);
        let is_matrix = MATRIX_SEMANTICS.contains(key);
        if stage != ShaderStage::Vertex && !is_matrix {
            continue;
        }
        let identity = match *ty {
            "mat3" => "mat3(1.0)",
            "vec3" => "vec3(0.0)",
            _ => "mat4(1.0)",
        };
        let host = |e: &str| {
            crate::text::identifiers(e).any(|id| p.blocks.iter().any(|b| b.instance == id) || p.samplers.iter().any(|s| s.name == id))
        };
        // Compute shaders have no host blocks; other stages cannot read vertex inputs.
        let usable = |e: &str| {
            if compute { !host(e) && !depends_on_vertex_only(e, ctx, &vertex_helpers) } else { stage == ShaderStage::Vertex || !depends_on_vertex_only(e, ctx, &vertex_helpers) }
        };
        let primary = semantic_expr(ctx, key);
        let expr = if usable(primary) {
            primary.to_string()
        } else {
            match crate::profiles::default_semantics().get(key).filter(|f| usable(f)) {
                Some(f) => f.to_string(),
                None => identity.to_string(),
            }
        };
        w.piece(Section::Semantics, &[global], format!("{ty} {global} = {expr};"));
    }
    w.piece(Section::Late, &["sb_BackColor"], "vec4 sb_BackColor;");
    w.piece(Section::Late, &["sb_BackSecondaryColor"], "vec4 sb_BackSecondaryColor;");
    if stage == ShaderStage::Vertex {
        w.piece(Section::Semantics, &["sb_gl_SecondaryColor"], "vec4 sb_gl_SecondaryColor = vec4(0.0, 0.0, 0.0, 1.0);");
        w.piece(Section::Semantics, &["sb_gl_MultiTexCoordZero"], "vec4 sb_gl_MultiTexCoordZero = vec4(0.0, 0.0, 0.0, 1.0);");
        w.piece(Section::Semantics, &["sb_gl_FogCoord"], "float sb_gl_FogCoord = 0.0;");
    }
    w.piece(Section::Constants, &["sb_MaxTextureCoords"], "const int sb_MaxTextureCoords = 8;");
    // Derived matrices.
    w.piece(Section::Derived, &["sb_ModelViewProjection"], "mat4 sb_ModelViewProjection = sb_Projection * sb_ModelView;");
    for (_, base, ty) in MATRIX_BASES.iter().chain([("", "sb_TextureMatrix", "mat4"), ("", "sb_LightmapMatrix", "mat4")].iter()) {
        let inv = format!("{base}Inverse");
        let tr = format!("{base}Transpose");
        let itr = format!("{base}InverseTranspose");
        w.piece(Section::Derived, &[&inv], format!("{ty} {inv} = inverse({base});"));
        w.piece(Section::Derived, &[&tr], format!("{ty} {tr} = transpose({base});"));
        w.piece(Section::Derived, &[&itr], format!("{ty} {itr} = transpose(inverse({base}));"));
    }
    for variant in ["", "Inverse", "Transpose", "InverseTranspose"] {
        let name = format!("sb_TextureMatrices{variant}");
        let t = format!("sb_TextureMatrix{variant}");
        let l = format!("sb_LightmapMatrix{variant}");
        w.piece(
            Section::Derived,
            &[&name],
            format!("mat4 {name}[8] = mat4[8]({t}, {l}, {l}, mat4(1.0), mat4(1.0), mat4(1.0), mat4(1.0), mat4(1.0));"),
        );
    }
    if compute {
        // gl_Fog below; nothing else applies to compute shaders.
    }
    // Profile globals (vertex stage).
    if stage == ShaderStage::Vertex {
        for g in &p.globals {
            // Iris: chunks never fade in the shadow pass (`mc_chunkFade` is -1 there).
            let init = if ctx.opts.is_shadow_pass && g.name == "mc_chunkFade" { "-1.0" } else { g.init.as_str() };
            w.piece(Section::ProfileGlobals, &[&g.name], format!("{} {} = {};", g.ty, g.name, init));
        }
    }
    // gl_Fog.
    w.piece(
        Section::Late,
        &["sb_gl_Fog", "sb_FogParameters"],
        "struct sb_FogParameters { vec4 color; float density; float start; float end; float scale; };\n\
         sb_FogParameters sb_gl_Fog = sb_FogParameters(sb_FogColor, fogDensity, fogStart, fogEnd, fogScale);",
    );
}

/// Effective semantic expression, honoring the shadow pass for world-space profiles.
fn semantic_expr<'a>(ctx: &'a Ctx, key: &str) -> &'a str {
    if ctx.opts.is_shadow_pass && ctx.profile.world_space {
        match key {
            "model_view" => return "shadowModelView",
            "projection" => return "shadowProjection",
            _ => {}
        }
    }
    ctx.profile.semantic(key)
}

/// (set, binding) of a host uniform block.
fn host_block_binding(ctx: &Ctx, name: &str) -> Option<(u32, u32)> {
    ctx.pack
        .bindings
        .entries
        .iter()
        .find(|e| {
            e.name == name
                || matches!(&e.resource, sb_core::model::ResourceRef::UniformBlock(n) if n == name)
        })
        .map(|e| (e.set, e.binding))
}

/// (set, binding) of a host sampler: the binding of the canonical name it provides, or
/// of its own name.
fn host_sampler_binding(ctx: &Ctx, s: &crate::profiles::ProfileSampler) -> Option<(u32, u32)> {
    let kind = sb_uniforms::sampler_kind(&s.ty)?;
    for provided in &s.provides {
        let canonical = sb_uniforms::canonicalize_with_kind(provided, &kind, &ctx.res_ctx);
        if let Some(e) = sb_uniforms::find_binding(ctx.pack.bindings, &canonical, &kind) {
            return Some((e.set, e.binding));
        }
        if let Some(e) = ctx.pack.bindings.get(provided) {
            return Some((e.set, e.binding));
        }
    }
    ctx.pack.bindings.get(&s.name).map(|e| (e.set, e.binding))
}

/// The resources a draw profile needs in the binding table: its host blocks
/// (`UniformBuffer`) and samplers (under the canonical name they provide, or their own
/// name). Call this for every profile a pack is translated with before building the
/// [`BindingTable`](sb_core::model::BindingTable).
pub fn register_profile_resources(
    profile: &crate::profiles::DrawProfile,
    builder: &mut sb_uniforms::BindingTableBuilder,
    resources: &sb_uniforms::ResourceContext,
) {
    for b in &profile.blocks {
        let c = sb_uniforms::canonicalize_ubo(&b.name);
        builder.add_canonical(&c, sb_core::model::ResourceKind::UniformBuffer);
    }
    for s in &profile.samplers {
        let Some(kind) = sb_uniforms::sampler_kind(&s.ty) else { continue };
        match s.provides.first() {
            Some(p) => {
                let c = sb_uniforms::canonicalize_with_kind(p, &kind, resources);
                builder.add_canonical(&c, kind);
            }
            None => {
                builder.add(&s.name, kind, sb_core::model::ResourceRef::Unknown(s.name.clone()));
            }
        }
    }
}

/// Names defined at global scope by pack code (variables, functions, block instances,
/// members of anonymous blocks, struct names).
pub(crate) fn pack_globals(unit: &TranslationUnit) -> HashMap<String, Line> {
    let mut out = HashMap::new();
    for item in &unit.items {
        match &item.kind {
            ItemKind::Decl(d) => {
                for v in &d.vars {
                    out.insert(v.name.clone(), item.line);
                }
                if let TypeBase::Struct(s) = &d.ty.ty.base
                    && let Some(n) = &s.name
                {
                    out.insert(n.clone(), item.line);
                }
            }
            ItemKind::Function(f) => {
                out.insert(f.proto.name.clone(), item.line);
            }
            ItemKind::Prototype(p) => {
                out.insert(p.name.clone(), item.line);
            }
            ItemKind::Block(b) => match &b.instance {
                Some((n, _)) => {
                    out.insert(n.clone(), item.line);
                }
                None => {
                    for f in &b.fields {
                        for (n, _) in &f.names {
                            out.insert(n.clone(), item.line);
                        }
                    }
                }
            },
            _ => {}
        }
    }
    out
}
