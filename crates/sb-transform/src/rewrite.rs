//! Per-stage rewriting of pack code (ARCHITECTURE §8, spec §3.2).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use sb_core::model::{OutputTarget, ResourceKind};
use sb_core::{GlslType, ShaderStage};

use crate::ast::*;
use crate::compat;
use crate::names;
use crate::program::{Ctx, ResourceUse, Section, StageWork};
use crate::scope::{Occ, walk_idents};

/// Run every per-stage rewrite (everything that does not depend on the other stages).
pub(crate) fn rewrite_stage(w: &mut StageWork, ctx: &Ctx) {
    normalize(w);
    hygiene(w, ctx);
    legacy_and_collisions(w);
    storage_qualifiers(w);
    profile_globals(w, ctx);
    uniforms(w, ctx);
    resources(w, ctx);
    storage_blocks(w, ctx);
    compat::rewrite_builtins(w, ctx);
    compat::vertex_inputs(w, ctx);
    compat::fixed_function_varyings(w, ctx);
    frag_outputs(w, ctx);
    compat::profile_pieces(w, ctx);
    crate::fixes::apply(w, ctx);
    crate::depth::apply(w, ctx);
    varying_global_inputs(w, ctx);
    rename_main(w);
    epilogues(w, ctx);
}

/// After linking: geometry-stage `EmitVertex()` hooks.
pub(crate) fn finish_stage(w: &mut StageWork, _ctx: &Ctx) {
    if w.stage != ShaderStage::Geometry || w.emit_hooks.is_empty() {
        return;
    }
    let hooks = w.emit_hooks.join("\n    ");
    w.unit.walk_exprs_mut(&mut |e| {
        if let Expr::Call(Callee::Name(n), args) = e {
            if n == "EmitVertex" && args.is_empty() {
                *n = "sb_emitVertex".into();
            } else if n == "EmitStreamVertex" && args.len() == 1 {
                *n = "sb_emitStreamVertex".into();
            }
        }
        Walk::Children
    });
    // Prototypes before the pack code, definitions after it (hooks reference pack outputs).
    w.piece(Section::Late, &["sb_emitVertex"], "void sb_emitVertex();");
    w.piece(Section::Tail, &["sb_emitVertex"], format!("void sb_emitVertex() {{\n    {hooks}\n    EmitVertex();\n}}"));
    w.piece(Section::Late, &["sb_emitStreamVertex"], "void sb_emitStreamVertex(int sb_stream);");
    w.piece(
        Section::Tail,
        &["sb_emitStreamVertex"],
        format!("void sb_emitStreamVertex(int sb_stream) {{\n    {hooks}\n    EmitStreamVertex(sb_stream);\n}}"),
    );
}

// ------------------------------------------------------------------------- helpers

/// Remove global declarators matching `pred` (one declaration per declarator is
/// returned, with the item's line). Declarations left without declarators are removed
/// unless they define a struct.
pub(crate) fn take_global_decls(
    unit: &mut TranslationUnit,
    mut pred: impl FnMut(&Declaration, &Declarator) -> bool,
) -> Vec<(Declaration, Line)> {
    let mut out = Vec::new();
    unit.items.retain_mut(|item| {
        let ItemKind::Decl(d) = &mut item.kind else { return true };
        if d.vars.is_empty() {
            return true;
        }
        let mut kept = Vec::new();
        for v in std::mem::take(&mut d.vars) {
            if pred(d, &v) {
                let mut ty = d.ty.clone();
                if let TypeBase::Struct(s) = &ty.ty.base
                    && let Some(n) = &s.name
                {
                    ty.ty.base = TypeBase::Named(n.clone());
                }
                out.push((Declaration { ty, vars: vec![v] }, item.line));
            } else {
                kept.push(v);
            }
        }
        d.vars = kept;
        !d.vars.is_empty() || matches!(&d.ty.ty.base, TypeBase::Struct(s) if s.name.is_some())
    });
    out
}

fn strip_precision_quals(q: &mut Vec<Qualifier>) {
    q.retain(|x| !matches!(x, Qualifier::Precision(_)));
}

fn strip_precision_type(t: &mut TypeSpec) {
    if let TypeBase::Struct(s) = &mut t.base {
        for f in &mut s.fields {
            strip_precision_quals(&mut f.quals);
            strip_precision_type(&mut f.ty);
        }
    }
}

fn strip_precision_decl(d: &mut Declaration) {
    strip_precision_quals(&mut d.ty.quals);
    strip_precision_type(&mut d.ty.ty);
}

fn strip_precision_proto(p: &mut Prototype) {
    strip_precision_quals(&mut p.ret.quals);
    for param in &mut p.params {
        strip_precision_quals(&mut param.quals);
    }
}

/// Unsized struct member arrays `T[] m` -> `T m[]` (Iris CompatibilityTransformer).
fn move_member_arrays(fields: &mut [Field]) {
    for f in fields {
        if !f.ty.array.is_empty() {
            let dims = std::mem::take(&mut f.ty.array);
            for (_, d) in &mut f.names {
                let mut nd = dims.clone();
                nd.append(d);
                *d = nd;
            }
        }
    }
}

// ------------------------------------------------------------------------- passes

/// Strip precision qualifiers and statements, drop the pack's `invariant gl_Position`
/// (we emit our own), split multi-declarator global declarations, move unsized struct
/// member arrays.
fn normalize(w: &mut StageWork) {
    w.unit.items.retain(|i| !matches!(&i.kind, ItemKind::Precision(..)) && !matches!(&i.kind, ItemKind::Invariant(n) if n == "gl_Position"));
    let mut items = Vec::with_capacity(w.unit.items.len());
    for mut item in std::mem::take(&mut w.unit.items) {
        match &mut item.kind {
            ItemKind::Function(f) => {
                strip_precision_proto(&mut f.proto);
                for s in &mut f.body {
                    s.walk_stmts_mut(&mut |s| match &mut s.kind {
                        StmtKind::Decl(d) => {
                            strip_precision_decl(d);
                            if let TypeBase::Struct(st) = &mut d.ty.ty.base {
                                move_member_arrays(&mut st.fields);
                            }
                        }
                        StmtKind::For { cond: Some(Condition::Decl { ty, .. }), .. }
                        | StmtKind::While { cond: Condition::Decl { ty, .. }, .. } => strip_precision_quals(&mut ty.quals),
                        _ => {}
                    });
                }
            }
            ItemKind::Prototype(p) => strip_precision_proto(p),
            ItemKind::Block(b) => {
                strip_precision_quals(&mut b.quals);
                for f in &mut b.fields {
                    strip_precision_quals(&mut f.quals);
                    strip_precision_type(&mut f.ty);
                }
                move_member_arrays(&mut b.fields);
            }
            ItemKind::Decl(d) => {
                strip_precision_decl(d);
                if let TypeBase::Struct(st) = &mut d.ty.ty.base {
                    move_member_arrays(&mut st.fields);
                }
                let anonymous_struct = matches!(&d.ty.ty.base, TypeBase::Struct(s) if s.name.is_none());
                if d.vars.len() > 1 && !anonymous_struct {
                    let line = item.line;
                    let vars = std::mem::take(&mut d.vars);
                    let mut first = true;
                    for v in vars {
                        let mut ty = d.ty.clone();
                        if !first && let TypeBase::Struct(s) = &ty.ty.base {
                            ty.ty.base = TypeBase::Named(s.name.clone().unwrap_or_default());
                        }
                        first = false;
                        items.push(Item { kind: ItemKind::Decl(Declaration { ty, vars: vec![v] }), line });
                    }
                    continue;
                }
            }
            _ => {}
        }
        items.push(item);
    }
    w.unit.items = items;
}

/// Rename pack identifiers that clash with ours or with Vulkan keywords:
/// `sb_*` -> `sbu_*` (the preprocessor's `sb_kw_*` escapes excepted), names the draw
/// profile declares -> `sbu_<name>`, reserved words -> `sb_kw_<name>`, and `texture`
/// declared as anything but a sampler uniform -> `sb_kw_texture`.
fn hygiene(w: &mut StageWork, ctx: &Ctx) {
    let info = &w.src.info;
    let uniforms: BTreeSet<String> =
        info.loose_uniforms.iter().map(|u| u.name.clone()).chain(info.opaque_uniforms.iter().map(|o| o.name.clone())).collect();
    let texture_is_sampler = info.opaque_uniforms.iter().any(|o| o.name == "texture");
    let mut declared: BTreeMap<String, bool /*function or struct*/> = BTreeMap::new();
    walk_idents(&mut w.unit, &mut |n, occ| match occ {
        Occ::GlobalDecl | Occ::LocalDecl | Occ::ParamDecl | Occ::BlockMemberDecl => {
            declared.entry(n.clone()).or_insert(false);
        }
        Occ::FunctionDecl => {
            declared.insert(n.clone(), true);
        }
        Occ::TypeName if !is_builtin_type(n) => {
            declared.insert(n.clone(), true);
        }
        _ => {}
    });
    let mut plan: HashMap<String, String> = HashMap::new();
    for name in declared.keys() {
        if name.starts_with("sb_") && !name.starts_with("sb_kw_") && !uniforms.contains(name) {
            plan.insert(name.clone(), format!("sbu_{}", &name[3..]));
        } else if ctx.profile_names.contains(name) && !uniforms.contains(name) {
            plan.insert(name.clone(), format!("sbu_{name}"));
        } else if names::is_reserved_at_460(name) && !names::is_vulkan_type_keyword(name) {
            plan.insert(name.clone(), format!("sb_kw_{name}"));
        } else if names::is_vulkan_type_keyword(name) && !declared[name] {
            plan.insert(name.clone(), format!("sb_kw_{name}"));
        }
    }
    if plan.is_empty() && !declared.contains_key("texture") {
        return;
    }
    walk_idents(&mut w.unit, &mut |n, occ| {
        if n == "texture" {
            let rename = match occ {
                Occ::LocalDecl | Occ::ParamDecl | Occ::LocalRef => true,
                Occ::GlobalDecl | Occ::GlobalRef => !texture_is_sampler,
                _ => false,
            };
            if rename {
                *n = "sb_kw_texture".into();
            }
            return;
        }
        let Some(new) = plan.get(n.as_str()) else { return };
        let apply = match occ {
            Occ::Call | Occ::TypeName => declared.get(n.as_str()).copied().unwrap_or(false),
            _ => true,
        };
        if apply {
            *n = new.clone();
        }
    });
}

/// Builtin type names (for distinguishing struct names in type specifiers).
pub(crate) fn is_builtin_type(n: &str) -> bool {
    GlslType::parse(n).is_some() || matches!(n, "void" | "atomic_uint") || sb_uniforms::is_opaque_type(n) || n.starts_with("sampler") || n.starts_with("isampler") || n.starts_with("usampler") || n.starts_with("image") || n.starts_with("iimage") || n.starts_with("uimage")
}

/// Legacy sampling calls (`texture2D` -> `texture`, `shadow2D(..)` -> `vec4(texture(..))`)
/// and pack functions that clash with GLSL 4.60 built-ins or legacy names (renamed to
/// `sb_u_<name>` with the calls that match their arity).
fn legacy_and_collisions(w: &mut StageWork) {
    let mut user: HashMap<String, BTreeSet<usize>> = HashMap::new();
    for item in &w.unit.items {
        let p = match &item.kind {
            ItemKind::Function(f) => &f.proto,
            ItemKind::Prototype(p) => p,
            _ => continue,
        };
        let n = p.name.as_str();
        if n != "main"
            && (names::legacy_texture(n).is_some()
                || names::is_vulkan_type_keyword(n)
                || names::is_builtin_function(n)
                || names::is_extension_builtin(n)
                || n == "ftransform")
        {
            let arity = if p.params.len() == 1 && p.params[0].name.is_none() && p.params[0].ty.name() == Some("void") { 0 } else { p.params.len() };
            user.entry(n.to_string()).or_default().insert(arity);
        }
    }
    for item in &mut w.unit.items {
        let p = match &mut item.kind {
            ItemKind::Function(f) => &mut f.proto,
            ItemKind::Prototype(p) => p,
            _ => continue,
        };
        if user.contains_key(&p.name) {
            p.name = format!("sb_u_{}", p.name);
        }
    }
    let trinary = w.src.extensions.iter().any(|e| e.name == "GL_AMD_shader_trinary_minmax" && e.behavior != "disable");
    let mut polyfills: BTreeSet<&'static str> = BTreeSet::new();
    w.unit.walk_exprs_mut(&mut |e| {
        if let Expr::Call(Callee::Name(n), args) = e {
            if user.get(n.as_str()).is_some_and(|a| a.contains(&args.len())) {
                *n = format!("sb_u_{n}");
                return Walk::Children;
            }
            if trinary && args.len() == 3 {
                let poly = match n.as_str() {
                    "min3" => Some("sb_min3"),
                    "max3" => Some("sb_max3"),
                    "mid3" => Some("sb_mid3"),
                    _ => None,
                };
                if let Some(p) = poly {
                    polyfills.insert(p);
                    *n = p.to_string();
                    return Walk::Children;
                }
            }
            if let Some(l) = names::legacy_texture(n) {
                *n = l.core.to_string();
                if l.shadow {
                    let inner = std::mem::replace(e, Expr::Int(0));
                    *e = Expr::call("vec4", vec![inner]);
                }
            }
        }
        Walk::Children
    });
    // GL_AMD_shader_trinary_minmax only exists on AMD hardware: emulate it.
    let half = w.src.extensions.iter().any(|e| e.name.starts_with("GL_EXT_shader_explicit_arithmetic_types") && e.behavior != "disable");
    for p in polyfills {
        let mut types = vec!["float", "vec2", "vec3", "vec4", "int", "ivec2", "ivec3", "ivec4", "uint", "uvec2", "uvec3", "uvec4"];
        if half {
            types.extend(["float16_t", "f16vec2", "f16vec3", "f16vec4"]);
        }
        let body = match p {
            "sb_min3" => "min(a, min(b, c))",
            "sb_max3" => "max(a, max(b, c))",
            _ => "max(min(a, b), min(max(a, b), c))",
        };
        let text: Vec<String> = types.iter().map(|t| format!("{t} {p}({t} a, {t} b, {t} c) {{ return {body}; }}")).collect();
        w.piece(Section::Late, &[p], text.join("\n"));
    }
}

/// `attribute` -> `in`; `varying` -> `out` in producing stages, `in` in the fragment
/// stage; geometry `varying in`/`varying out` -> `in`/`out`.
fn storage_qualifiers(w: &mut StageWork) {
    let stage = w.stage;
    let fix = |q: &mut Vec<Qualifier>| {
        let has_in = has_storage(q, &Storage::In);
        let has_out = has_storage(q, &Storage::Out);
        let mut out = Vec::with_capacity(q.len());
        for x in q.drain(..) {
            match x {
                Qualifier::Storage(Storage::Attribute) => {
                    if !has_in {
                        out.push(Qualifier::Storage(Storage::In));
                    }
                }
                Qualifier::Storage(Storage::Varying) => {
                    if !has_in && !has_out {
                        out.push(Qualifier::Storage(if stage == ShaderStage::Fragment { Storage::In } else { Storage::Out }));
                    }
                }
                other => out.push(other),
            }
        }
        *q = out;
    };
    for item in &mut w.unit.items {
        match &mut item.kind {
            ItemKind::Decl(d) => fix(&mut d.ty.quals),
            ItemKind::Block(b) => fix(&mut b.quals),
            _ => {}
        }
    }
}

/// Profile globals: in the vertex stage, pack declarations of a profile global's name
/// are removed (the profile defines it); in later stages, `uniform`/`in` declarations of
/// a varying profile global are removed (it is forwarded from the vertex stage).
fn profile_globals(w: &mut StageWork, ctx: &Ctx) {
    if ctx.profile_globals.is_empty() {
        return;
    }
    if w.stage == ShaderStage::Vertex {
        let taken = take_global_decls(&mut w.unit, |_, v| ctx.profile_globals.contains_key(&v.name));
        for (d, line) in taken {
            let name = &d.vars[0].name;
            w.info("xf.profile-global", format!("`{name}` is provided by draw profile `{}`; the pack declaration is replaced", ctx.profile.name), line);
        }
    } else if w.stage != ShaderStage::Compute {
        let stage = w.stage;
        let taken = take_global_decls(&mut w.unit, |d, v| {
            ctx.varying_globals.contains_key(&v.name)
                && (has_storage(&d.ty.quals, &Storage::Uniform) || crate::analyze::is_input(stage, &d.ty.quals))
        });
        drop(taken);
    }
}

/// Loose uniforms: declarations are removed and references resolve to `sb_Frame` /
/// `sb_Draw` members (renamed to `<name>__<type>` members when the layout says so).
/// Core-profile per-draw names resolve to the profile semantics.
fn uniforms(w: &mut StageWork, ctx: &Ctx) {
    let env = crate::consteval::global_consts(&w.unit);
    let taken = take_global_decls(&mut w.unit, |d, _| {
        has_storage(&d.ty.quals, &Storage::Uniform) && d.ty.ty.name().is_none_or(|t| !sb_uniforms::is_opaque_type(t))
    });
    let mut renames: HashMap<String, String> = HashMap::new();
    for (d, line) in taken {
        let v = &d.vars[0];
        let name = v.name.clone();
        if let Some(r) = names::core_profile_replacement(&name) {
            renames.insert(name, r.to_string());
            continue;
        }
        let ty = d.ty.ty.name().and_then(GlslType::parse).and_then(|t| {
            let mut dims = crate::analyze::dims(&d.ty.ty.array, &env);
            dims.extend(crate::analyze::dims(&v.array, &env));
            match dims.as_slice() {
                [] => Some(t),
                [Some(n)] => Some(t.with_array(*n)),
                _ => None,
            }
        });
        let Some(ty) = ty else {
            // Unsupported (struct or multi-dimensional): keep as a plain zero global.
            w.warn("xf.uniform-unsupported", format!("uniform `{name}` has an unsupported type; it becomes a plain global"), line);
            let mut nd = d.clone();
            nd.ty.quals.retain(|q| !matches!(q, Qualifier::Storage(Storage::Uniform) | Qualifier::Layout(_)));
            nd.vars[0].init = None;
            w.unit.items.insert(0, Item { kind: ItemKind::Decl(nd), line });
            continue;
        };
        match ctx.pack.members.get(&name, ty) {
            Some(m) => {
                if m.member != name {
                    renames.insert(name, m.member.clone());
                }
            }
            None => w.error(
                "xf.uniform-missing",
                format!("uniform `{name}` ({ty}) is not part of the pack uniform layout (add the stage's uniform_decls to the LayoutBuilder)"),
                line,
            ),
        }
    }
    let globals = compat::pack_globals(&w.unit);
    for (n, r) in names::CORE_PROFILE_NAMES {
        if !globals.contains_key(*n) {
            renames.entry((*n).to_string()).or_insert_with(|| (*r).to_string());
        }
    }
    if renames.is_empty() {
        return;
    }
    walk_idents(&mut w.unit, &mut |n, occ| {
        if occ == Occ::GlobalRef
            && let Some(r) = renames.get(n.as_str())
        {
            *n = r.clone();
        }
    });
}

/// Opaque uniforms: canonical names, explicit set/binding (Vulkan target), host
/// samplers for profile-provided names, image formats.
fn resources(w: &mut StageWork, ctx: &Ctx) {
    let vulkan = ctx.opts.target == OutputTarget::Vulkan;
    let taken = take_global_decls(&mut w.unit, |d, _| {
        has_storage(&d.ty.quals, &Storage::Uniform) && d.ty.ty.name().is_some_and(sb_uniforms::is_opaque_type)
    });
    if taken.is_empty() {
        return;
    }
    let image_reads = image_reads(&w.unit);
    let mut renames: HashMap<String, String> = HashMap::new();
    let mut declared: BTreeSet<String> = BTreeSet::new();
    for (d, line) in taken {
        let v = &d.vars[0];
        let name = v.name.clone();
        let tn = d.ty.ty.name().unwrap_or_default().to_string();
        let format = crate::analyze::image_format(&d.ty.quals);
        let readonly = has_storage(&d.ty.quals, &Storage::ReadOnly);
        let writeonly = has_storage(&d.ty.quals, &Storage::WriteOnly);
        let Some(kind) = sb_uniforms::sampler_kind(&tn).or_else(|| sb_uniforms::image_kind(&tn, format.as_deref(), readonly, writeonly)) else {
            w.error("xf.resource-type", format!("unsupported opaque type `{tn}` for `{name}`"), line);
            continue;
        };
        let (canonical, entry) = if let Some(b) = ctx.opts.custom_texture_renames.get(&name) {
            (sb_uniforms::Canonical::new(b.clone(), sb_core::model::ResourceRef::Unknown(b.clone())), ctx.pack.bindings.get(b))
        } else {
            let c = sb_uniforms::canonicalize_with_kind(&name, &kind, &ctx.res_ctx);
            let e = sb_uniforms::find_binding(ctx.pack.bindings, &c, &kind);
            (c, e)
        };
        let Some(entry) = entry else {
            w.error(
                "xf.unbound-resource",
                format!("`{name}` (canonical `{}`, {tn}) has no entry in the binding table", canonical.name),
                line,
            );
            continue;
        };
        if crate::program::needs_raw_vulkan(&kind) {
            w.requires_raw_vulkan = true;
        }
        let provider = ctx.profile.sampler_providing(&canonical.name).filter(|p| {
            sb_uniforms::sampler_kind(&p.ty).is_some_and(|pk| sb_uniforms::kinds_compatible(&pk, &kind))
        });
        let glsl_name = match provider {
            Some(p) => p.name.clone(),
            None => entry.name.clone(),
        };
        renames.insert(name.clone(), glsl_name.clone());
        w.opaque_types.insert(glsl_name.clone(), tn.clone());
        if !w.resources.iter().any(|r| r.glsl_name == glsl_name) {
            w.resources.push(ResourceUse { glsl_name: glsl_name.clone(), binding_name: entry.name.clone(), always: false });
        }
        if provider.is_some() || !declared.insert(glsl_name.clone()) {
            continue;
        }
        let dims = crate::print::array_dims(&v.array);
        let mut layout: Vec<String> = Vec::new();
        if vulkan {
            layout.push(format!("set = {}", entry.set));
            layout.push(format!("binding = {}", entry.binding));
        }
        let mut mem: Vec<&str> = Vec::new();
        if let ResourceKind::StorageImage { .. } = kind {
            let mut readonly = readonly;
            let mut writeonly = writeonly;
            let fmt = format.clone().or_else(|| ctx.opts.image_formats.get(&canonical.name).cloned());
            match fmt {
                Some(f) => layout.push(f),
                None => {
                    if image_reads.contains(&name) {
                        if ctx.opts.storage_image_read_without_format {
                            w.extensions.insert("GL_EXT_shader_image_load_formatted".into());
                        } else {
                            w.warn(
                                "xf.image-format",
                                format!("image `{name}` is read but has no format qualifier and the device cannot read without format"),
                                line,
                            );
                            w.extensions.insert("GL_EXT_shader_image_load_formatted".into());
                        }
                    } else if !writeonly {
                        writeonly = true;
                        readonly = false;
                    }
                }
            }
            for q in &d.ty.quals {
                match q {
                    Qualifier::Storage(Storage::Coherent) => mem.push("coherent"),
                    Qualifier::Storage(Storage::Volatile) => mem.push("volatile"),
                    Qualifier::Storage(Storage::Restrict) => mem.push("restrict"),
                    _ => {}
                }
            }
            if readonly {
                mem.push("readonly");
            }
            if writeonly {
                mem.push("writeonly");
            }
        }
        let layout = if layout.is_empty() { String::new() } else { format!("layout({}) ", layout.join(", ")) };
        let mem = if mem.is_empty() { String::new() } else { format!("{} ", mem.join(" ")) };
        w.piece(Section::Resources, &[&glsl_name], format!("{layout}{mem}uniform {tn} {glsl_name}{dims};"));
    }
    if renames.iter().all(|(a, b)| a == b) {
        return;
    }
    walk_idents(&mut w.unit, &mut |n, occ| {
        if occ == Occ::GlobalRef
            && let Some(r) = renames.get(n.as_str())
        {
            *n = r.clone();
        }
    });
}

/// Names of images passed as the first argument of `imageLoad`/`imageAtomic*`.
fn image_reads(unit: &TranslationUnit) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    unit.walk_exprs(&mut |e| {
        if let Expr::Call(Callee::Name(n), args) = e
            && (n == "imageLoad" || n.starts_with("imageAtomic"))
            && let Some(root) = args.first().and_then(compat::root_ident)
        {
            out.insert(root.to_string());
        }
        Walk::Children
    });
    out
}

fn set_binding(quals: &mut Vec<Qualifier>, packing: &str, set_binding: Option<(u32, u32)>) {
    let mut has_packing = false;
    for q in quals.iter_mut() {
        if let Qualifier::Layout(ids) = q {
            ids.retain(|l| !l.name.eq_ignore_ascii_case("binding") && !l.name.eq_ignore_ascii_case("set") && !l.name.eq_ignore_ascii_case("location"));
            if ids.iter().any(|l| matches!(l.name.as_str(), "std140" | "std430" | "packed" | "shared" | "scalar")) {
                has_packing = true;
                for l in ids.iter_mut() {
                    // `packed`/`shared` layouts have no defined offsets in Vulkan.
                    if l.name == "packed" || l.name == "shared" {
                        l.name = packing.to_string();
                    }
                }
            }
        }
    }
    quals.retain(|q| !matches!(q, Qualifier::Layout(ids) if ids.is_empty()));
    let mut ids = Vec::new();
    if !has_packing {
        ids.push(LayoutId { name: packing.into(), value: None });
    }
    if let Some((s, b)) = set_binding {
        ids.push(LayoutId { name: "set".into(), value: Some(Expr::Int(s as i32)) });
        ids.push(LayoutId { name: "binding".into(), value: Some(Expr::Int(b as i32)) });
    }
    if !ids.is_empty() {
        quals.insert(0, Qualifier::Layout(ids));
    }
}

/// Pack SSBOs (`buffer` blocks, bound to `bufferObject.N`) and uniform blocks.
fn storage_blocks(w: &mut StageWork, ctx: &Ctx) {
    let vulkan = ctx.opts.target == OutputTarget::Vulkan;
    let env = crate::consteval::global_consts(&w.unit);
    let mut errors = Vec::new();
    let mut uses = Vec::new();
    let mut raw = false;
    for item in &mut w.unit.items {
        let line = item.line;
        let ItemKind::Block(b) = &mut item.kind else { continue };
        let is_buffer = has_storage(&b.quals, &Storage::Buffer);
        let is_uniform = has_storage(&b.quals, &Storage::Uniform);
        if !is_buffer && !is_uniform {
            continue;
        }
        let (canonical, kind, packing) = if is_buffer {
            raw = true;
            let binding = layout_value(&b.quals, "binding").flatten().and_then(|e| crate::consteval::eval(e, &env)).and_then(|v| v.as_u32());
            let Some(binding) = binding else {
                errors.push((format!("storage block `{}` has no constant `layout(binding = N)`", b.name), line));
                continue;
            };
            (sb_uniforms::canonicalize_ssbo(&b.name, binding), ResourceKind::StorageBuffer, "std430")
        } else {
            (sb_uniforms::canonicalize_ubo(&b.name), ResourceKind::UniformBuffer, "std140")
        };
        let entry = sb_uniforms::find_binding(ctx.pack.bindings, &canonical, &kind);
        let Some(entry) = entry else {
            errors.push((format!("block `{}` has no entry in the binding table", b.name), line));
            continue;
        };
        set_binding(&mut b.quals, packing, vulkan.then_some((entry.set, entry.binding)));
        uses.push(ResourceUse {
            glsl_name: b.instance.as_ref().map_or_else(|| b.name.clone(), |i| i.0.clone()),
            binding_name: entry.name.clone(),
            always: true,
        });
    }
    for (m, l) in errors {
        w.error("xf.unbound-resource", m, l);
    }
    w.requires_raw_vulkan |= raw;
    w.resources.extend(uses);
}

/// Fragment outputs: `gl_FragColor`/`gl_FragData[i]` and user `out`s get explicit
/// (remapped) locations; the alpha test is prepared for the epilogue.
fn frag_outputs(w: &mut StageWork, ctx: &Ctx) {
    if w.stage != ShaderStage::Fragment {
        return;
    }
    let env = crate::consteval::global_consts(&w.unit);
    // gl_FragColor -> gl_FragData[0]
    let mut literal: BTreeSet<u32> = BTreeSet::new();
    let mut dynamic = false;
    w.unit.walk_exprs_mut(&mut |e| {
        if let Expr::Ident(n) = e
            && n == "gl_FragColor"
        {
            *e = Expr::Index(Box::new(Expr::ident("gl_FragData")), Box::new(Expr::Int(0)));
            literal.insert(0);
            return Walk::Skip;
        }
        if let Expr::Index(base, idx) = e
            && base.as_ident() == Some("gl_FragData")
        {
            match crate::consteval::eval(idx, &env).and_then(|v| v.as_u32()) {
                Some(i) if i < 32 => {
                    literal.insert(i);
                }
                _ => dynamic = true,
            }
        }
        Walk::Children
    });
    let phys = |logical: u32| -> Option<u32> {
        match &ctx.opts.output_locations {
            None => Some(logical),
            Some(v) => v.get(logical as usize).copied(),
        }
    };
    let mut alpha_target: Option<String> = None;
    // User outputs.
    // `out float gl_FragDepth;` style redeclarations of builtins stay as they are.
    let outs = take_global_decls(&mut w.unit, |d, v| has_storage(&d.ty.quals, &Storage::Out) && !v.name.starts_with("gl_"));
    let n_user = outs.len();
    let mut used_logical: BTreeSet<u32> = literal.clone();
    let mut user_logical: BTreeSet<u32> = BTreeSet::new();
    let mut pending: Vec<(Declaration, Line, Option<u32>)> = Vec::new();
    for (d, line) in outs {
        let explicit = layout_value(&d.ty.quals, "location").flatten().and_then(|e| crate::consteval::eval(e, &env)).and_then(|v| v.as_u32());
        let name = &d.vars[0].name;
        let logical = explicit.or_else(|| {
            if n_user == 1 && literal.is_empty() && !dynamic {
                Some(0)
            } else {
                name.strip_prefix("outColor").and_then(|s| s.parse::<u32>().ok())
            }
        });
        if let Some(l) = logical {
            used_logical.insert(l);
        }
        pending.push((d, line, logical));
    }
    let mut next_free = 0u32;
    for (mut d, line, logical) in pending {
        let logical = match logical {
            Some(l) => l,
            None => {
                while used_logical.contains(&next_free) {
                    next_free += 1;
                }
                used_logical.insert(next_free);
                w.warn(
                    "xf.output-location",
                    format!("fragment output `{}` has no location; it is assigned location {next_free} in declaration order", d.vars[0].name),
                    line,
                );
                next_free
            }
        };
        user_logical.insert(logical);
        let name = d.vars[0].name.clone();
        let base = d.ty.ty.name().and_then(GlslType::parse).map_or("float", |t| match t.scalar {
            sb_core::ScalarKind::Int => "int",
            sb_core::ScalarKind::Uint => "uint",
            _ => "float",
        });
        let count = {
            let mut dims = crate::analyze::dims(&d.ty.ty.array, &env);
            dims.extend(crate::analyze::dims(&d.vars[0].array, &env));
            dims.first().copied().flatten().unwrap_or(1)
        };
        d.ty.quals.retain(|q| !matches!(q, Qualifier::Layout(_)));
        if logical == 0 {
            alpha_target = Some(if count > 1 { format!("{name}[0]") } else { name.clone() });
            if d.ty.ty.name() != Some("vec4") {
                alpha_target = None;
            }
        }
        match phys(logical) {
            Some(p) => {
                d.ty.quals.insert(0, Qualifier::Layout(vec![LayoutId { name: "location".into(), value: Some(Expr::Int(p as i32)) }]));
                for k in 0..count {
                    w.frag_outputs.push((p + k, base.to_string(), name.clone()));
                }
                w.unit.items.insert(0, Item { kind: ItemKind::Decl(d), line });
            }
            None => {
                d.ty.quals.retain(|q| !matches!(q, Qualifier::Storage(Storage::Out)));
                w.info("xf.output-removed", format!("fragment output `{name}` has no attachment; its writes are discarded"), line);
                w.unit.items.insert(0, Item { kind: ItemKind::Decl(d), line });
            }
        }
    }
    // gl_FragData.
    if dynamic {
        let n = ctx.opts.output_locations.as_ref().map_or(8, |v| v.len().max(1) as u32).max(literal.iter().max().map_or(0, |m| m + 1));
        w.unit.walk_exprs_mut(&mut |e| {
            if let Expr::Ident(n) = e
                && n == "gl_FragData"
            {
                *n = "sb_FragDataArr".into();
            }
            Walk::Children
        });
        w.piece(Section::Late, &["sb_FragDataArr"], format!("vec4 sb_FragDataArr[{n}];"));
        for i in 0..n {
            if user_logical.contains(&i) {
                continue;
            }
            if let Some(p) = phys(i) {
                w.add_iface(&format!("layout(location = {p}) out vec4 sb_FragData{i};"));
                w.frag_outputs.push((p, "float".into(), format!("sb_FragData{i}")));
                w.epilogue.push(format!("sb_FragData{i} = sb_FragDataArr[{i}];"));
            }
        }
        if alpha_target.is_none() {
            alpha_target = Some("sb_FragDataArr[0]".into());
        }
    } else if !literal.is_empty() {
        w.unit.walk_exprs_mut(&mut |e| {
            if let Expr::Index(base, idx) = e
                && base.as_ident() == Some("gl_FragData")
                && let Some(i) = crate::consteval::eval(idx, &env).and_then(|v| v.as_u32())
            {
                *e = Expr::Ident(format!("sb_FragData{i}"));
                return Walk::Skip;
            }
            Walk::Children
        });
        for &i in &literal {
            match phys(i) {
                Some(p) => {
                    w.add_iface(&format!("layout(location = {p}) out vec4 sb_FragData{i};"));
                    w.frag_outputs.push((p, "float".into(), format!("sb_FragData{i}")));
                }
                None => {
                    let name = format!("sb_FragData{i}");
                    w.piece(Section::Late, &[&name], format!("vec4 {name};"));
                }
            }
        }
        if literal.contains(&0) {
            alpha_target = Some("sb_FragData0".into());
        }
    }
    // `gl_FragDepth` read but never written: lenient drivers return the fragment's
    // depth; Vulkan requires DepthReplacing for any FragDepth use.
    let mut writes_depth = false;
    let mut reads_depth = false;
    w.unit.walk_exprs(&mut |e| {
        match e {
            Expr::Assign(lhs, _, _) if compat::root_ident(lhs) == Some("gl_FragDepth") => writes_depth = true,
            Expr::Ident(n) if n == "gl_FragDepth" => reads_depth = true,
            _ => {}
        }
        Walk::Children
    });
    if reads_depth && !writes_depth {
        w.unit.walk_exprs_mut(&mut |e| {
            if let Expr::Ident(n) = e
                && n == "gl_FragDepth"
            {
                *e = Expr::Field(Box::new(Expr::ident("gl_FragCoord")), "z".into());
                return Walk::Skip;
            }
            Walk::Children
        });
        w.unit.items.retain(|i| !matches!(&i.kind, ItemKind::Decl(d) if d.vars.iter().any(|v| v.name == "gl_FragDepth")));
        w.warn("xf.frag-depth", "gl_FragDepth is read but never written; reads use gl_FragCoord.z", 0);
    }
    // Alpha test.
    if let Some(at) = ctx.opts.alpha_test
        && let Some(target) = alpha_target
    {
        use sb_core::program::AlphaFunc;
        match at.func {
            AlphaFunc::Always => {}
            AlphaFunc::Never => w.epilogue.push("discard;".into()),
            func => {
                let op = func.glsl_op().unwrap_or(">");
                let reference = if ctx.pack.members.get("alphaTestRef", GlslType::FLOAT).is_some_and(|m| m.member == "alphaTestRef") {
                    "alphaTestRef".to_string()
                } else {
                    let mut s = String::new();
                    s.push_str(&format!("{:?}", at.reference));
                    s
                };
                w.epilogue.push(format!("if (!({target}.a {op} {reference})) discard;"));
            }
        }
    }
}

/// Non-vertex stages: varying profile globals referenced by pack or helper code
/// become stage inputs.
fn varying_global_inputs(w: &mut StageWork, ctx: &Ctx) {
    if matches!(w.stage, ShaderStage::Vertex | ShaderStage::Compute) || ctx.varying_globals.is_empty() {
        return;
    }
    let reach = crate::emit::reachable(w, ctx);
    let declared = compat::pack_globals(&w.unit);
    for (name, ty) in &ctx.varying_globals {
        if !reach.contains(name) || declared.contains_key(name) {
            continue;
        }
        let flat = if GlslType::parse(ty).is_some_and(|t| !matches!(t.scalar, sb_core::ScalarKind::Float)) { "flat " } else { "" };
        match w.stage {
            ShaderStage::Fragment => w.add_iface(&format!("{flat}in {ty} {name};")),
            _ => {
                // Pre-raster consumers read the first vertex's value through a global.
                w.add_iface(&format!("{flat}in {ty} sb_vary_{name}[];"));
                w.piece(Section::Late, &[name], format!("{ty} {name};"));
                w.prologue.push(format!("{name} = sb_vary_{name}[0];"));
            }
        }
    }
}

/// Rename the pack's `main` to `sb_user_main`.
fn rename_main(w: &mut StageWork) {
    let mut found = false;
    for item in &mut w.unit.items {
        let p = match &mut item.kind {
            ItemKind::Function(f) => &mut f.proto,
            ItemKind::Prototype(p) => p,
            _ => continue,
        };
        if p.name == "main" {
            p.name = "sb_user_main".into();
            found |= matches!(item.kind, ItemKind::Function(_));
        }
    }
    w.unit.walk_exprs_mut(&mut |e| {
        if let Expr::Call(Callee::Name(n), _) = e
            && n == "main"
        {
            *n = "sb_user_main".into();
        }
        Walk::Children
    });
    if !found {
        w.error("xf.no-main", "the stage has no `main` function", 0);
    }
}

/// Depth remap / Y flip of the last pre-raster stage.
fn epilogues(w: &mut StageWork, ctx: &Ctx) {
    use sb_core::model::DepthMode;
    if ctx.last_pre_raster != Some(w.stage) {
        return;
    }
    let mut stmts = Vec::new();
    match ctx.opts.depth_mode {
        DepthMode::ForwardZeroToOne => stmts.push("gl_Position.z = 0.5 * (gl_Position.z + gl_Position.w);".to_string()),
        DepthMode::ReversedZeroToOne => stmts.push("gl_Position.z = 0.5 * (gl_Position.w - gl_Position.z);".to_string()),
        DepthMode::GlNegOneToOne => {}
    }
    if ctx.opts.flip_y {
        stmts.push("gl_Position.y = -gl_Position.y;".into());
    }
    if w.stage == ShaderStage::Geometry {
        w.emit_hooks.extend(stmts);
    } else {
        w.epilogue.extend(stmts);
    }
}
