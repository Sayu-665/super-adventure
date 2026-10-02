//! Phase B: cross-stage interface linking (spec §3.4).
//!
//! For each adjacent stage pair (producer → consumer), interface variables are
//! matched by name; missing producer outputs are added (zero-initialized, Iris
//! `transformGrouped` rule a), type mismatches are repaired through a temporary global
//! in the producer (rule c), unconsumed outputs are demoted to plain globals, and both
//! sides get the same explicit locations. Integer varyings are `flat` on both sides.

use std::collections::{BTreeSet, HashMap};

use sb_core::{GlslType, ScalarKind, ShaderStage};

use crate::ast::*;
use crate::program::{Ctx, Section, StageWork};

/// An interface variable found in a stage.
#[derive(Debug, Clone)]
struct IVar {
    /// Variable name, or block name for interface blocks.
    name: String,
    /// Element type (type-level array dims moved into `dims`).
    elem: TypeSpec,
    /// Array dims excluding the per-vertex dimension.
    dims: Vec<ArrayDim>,
    /// Arrayed per vertex (geometry/tessellation inputs, tessellation control outputs).
    per_vertex: bool,
    /// Interface block members.
    block: Option<Vec<Field>>,
    /// Declared `flat`.
    flat: bool,
    /// Generated declaration (index into `gen_iface`) or pack item (index into `unit.items`).
    generated: bool,
    index: usize,
}

fn per_vertex(stage: ShaderStage, input: bool, patch: bool) -> bool {
    !patch
        && ((input && matches!(stage, ShaderStage::Geometry | ShaderStage::TessControl | ShaderStage::TessEval))
            || (!input && stage == ShaderStage::TessControl))
}

fn collect(w: &StageWork, input: bool) -> Vec<IVar> {
    let mut out = Vec::new();
    let lists: [(&Vec<Item>, bool); 2] = [(&w.gen_iface, true), (&w.unit.items, false)];
    for (items, generated) in lists {
        for (index, item) in items.iter().enumerate() {
            let (quals, name, elem, mut dims, block) = match &item.kind {
                ItemKind::Decl(d) if d.vars.len() == 1 => {
                    let v = &d.vars[0];
                    let mut dims = d.ty.ty.array.clone();
                    dims.extend(v.array.iter().cloned());
                    let elem = TypeSpec { base: d.ty.ty.base.clone(), array: Vec::new() };
                    (&d.ty.quals, v.name.clone(), elem, dims, None)
                }
                ItemKind::Block(b) => {
                    let dims = b.instance.as_ref().map(|(_, d)| d.clone()).unwrap_or_default();
                    (&b.quals, b.name.clone(), TypeSpec::named(b.name.clone()), dims, Some(b.fields.clone()))
                }
                _ => continue,
            };
            if name.starts_with("gl_") {
                continue;
            }
            let is = if input { crate::analyze::is_input(w.stage, quals) } else { crate::analyze::is_output(w.stage, quals) };
            if !is {
                continue;
            }
            let patch = has_storage(quals, &Storage::Patch);
            let pv = per_vertex(w.stage, input, patch);
            if pv && !dims.is_empty() {
                dims.remove(0);
            }
            out.push(IVar {
                name,
                elem,
                dims,
                per_vertex: pv,
                block,
                flat: interpolation(quals) == Some(Interp::Flat),
                generated,
                index,
            });
        }
    }
    out
}

fn item_mut<'w>(w: &'w mut StageWork, v: &IVar) -> &'w mut Item {
    if v.generated { &mut w.gen_iface[v.index] } else { &mut w.unit.items[v.index] }
}

fn quals_mut(item: &mut Item) -> Option<&mut Vec<Qualifier>> {
    match &mut item.kind {
        ItemKind::Decl(d) => Some(&mut d.ty.quals),
        ItemKind::Block(b) => Some(&mut b.quals),
        _ => None,
    }
}

/// Constant value of a layout expression.
fn layout_u32(e: &Expr, env: &crate::consteval::ConstEnv) -> Option<u32> {
    crate::consteval::eval(e, env).and_then(|c| c.as_u32())
}

/// Lowest explicit member location of an interface block whose members carry
/// `layout(location = …)` qualifiers.
fn member_base(fields: &[Field], env: &crate::consteval::ConstEnv) -> Option<u32> {
    fields.iter().filter_map(|f| layout_value(&f.quals, "location").flatten().and_then(|e| layout_u32(e, env))).min()
}

/// Locations used by an interface block whose members have explicit locations:
/// the span from the lowest member location to the end of the highest one.
fn member_footprint(fields: &[Field], structs: &HashMap<String, StructDef>, env: &crate::consteval::ConstEnv) -> Option<u32> {
    let base = member_base(fields, env)?;
    let mut cur = base;
    let mut end = base;
    for f in fields {
        if let Some(l) = layout_value(&f.quals, "location").flatten().and_then(|e| layout_u32(e, env)) {
            cur = l;
        }
        for (_, d) in &f.names {
            let n = slots(&f.ty, structs, env, 0) * dims_product(d, env);
            end = end.max(cur + n);
            cur += n;
        }
    }
    Some(end - base)
}

/// Remove `location` and `component` layout qualifiers.
fn strip_location(q: &mut Vec<Qualifier>) {
    for x in q.iter_mut() {
        if let Qualifier::Layout(ids) = x {
            ids.retain(|l| !l.name.eq_ignore_ascii_case("location") && !l.name.eq_ignore_ascii_case("component"));
        }
    }
    q.retain(|x| !matches!(x, Qualifier::Layout(ids) if ids.is_empty()));
}

/// Give an interface declaration the explicit location `location` (and `flat` when
/// required). Blocks whose members carry explicit locations keep them, shifted so the
/// lowest one becomes `location`, and get no block-level location (glslang's
/// auto-mapping would otherwise decorate both, which Vulkan forbids).
fn set_location(item: &mut Item, location: u32, flat: bool, env: &crate::consteval::ConstEnv) {
    let member_min = match &item.kind {
        ItemKind::Block(b) => member_base(&b.fields, env),
        _ => None,
    };
    let Some(q) = quals_mut(item) else { return };
    strip_location(q);
    match (member_min, &mut item.kind) {
        (Some(min), ItemKind::Block(b)) => {
            for f in &mut b.fields {
                for x in f.quals.iter_mut() {
                    if let Qualifier::Layout(ids) = x {
                        for l in ids.iter_mut().filter(|l| l.name.eq_ignore_ascii_case("location")) {
                            if let Some(old) = l.value.as_ref().and_then(|e| layout_u32(e, env)) {
                                l.value = Some(Expr::Int(location.saturating_add(old - min).min(i32::MAX as u32) as i32));
                            }
                        }
                    }
                }
            }
        }
        _ => {
            if let Some(q) = quals_mut(item) {
                q.insert(0, Qualifier::Layout(vec![LayoutId { name: "location".into(), value: Some(Expr::Int(location as i32)) }]));
            }
        }
    }
    if !flat {
        return;
    }
    match &mut item.kind {
        // Interface blocks cannot be qualified; their integer members are.
        ItemKind::Block(b) => {
            for f in &mut b.fields {
                let int = f.ty.name().and_then(GlslType::parse).is_some_and(|t| !matches!(t.scalar, ScalarKind::Float));
                if int && interpolation(&f.quals) != Some(Interp::Flat) {
                    f.quals.retain(|x| !matches!(x, Qualifier::Interp(_)));
                    f.quals.insert(0, Qualifier::Interp(Interp::Flat));
                }
            }
        }
        ItemKind::Decl(d) => {
            let q = &mut d.ty.quals;
            if interpolation(q) != Some(Interp::Flat) {
                q.retain(|x| !matches!(x, Qualifier::Interp(_)));
                q.insert(1, Qualifier::Interp(Interp::Flat));
            }
        }
        _ => {}
    }
}

/// Turn an interface declaration into a plain global (drops in/out, interpolation,
/// auxiliary and layout qualifiers).
fn demote(item: &mut Item) {
    if let Some(q) = quals_mut(item) {
        q.retain(|x| match x {
            Qualifier::Storage(s) => !matches!(s, Storage::In | Storage::Out | Storage::Centroid | Storage::Sample | Storage::Patch),
            Qualifier::Layout(_) | Qualifier::Interp(_) | Qualifier::Invariant => false,
            _ => true,
        });
    }
}

fn struct_defs(w: &StageWork) -> HashMap<String, StructDef> {
    let mut out = HashMap::new();
    for item in &w.unit.items {
        if let ItemKind::Decl(d) = &item.kind
            && let TypeBase::Struct(s) = &d.ty.ty.base
            && let Some(n) = &s.name
        {
            out.insert(n.clone(), (**s).clone());
        }
    }
    out
}

fn dims_product(dims: &[ArrayDim], env: &crate::consteval::ConstEnv) -> u32 {
    dims.iter()
        .map(|d| match d {
            ArrayDim::Sized(e) => crate::consteval::array_len(e, env).unwrap_or(1),
            ArrayDim::Unsized => 1,
        })
        .product::<u32>()
        .max(1)
}

/// Locations used by one element of `ty`.
fn slots(ty: &TypeSpec, structs: &HashMap<String, StructDef>, env: &crate::consteval::ConstEnv, depth: u32) -> u32 {
    let mult = dims_product(&ty.array, env);
    let base = match &ty.base {
        TypeBase::Named(n) => match GlslType::parse(n) {
            Some(t) => {
                let per_col = if t.scalar == ScalarKind::Double && t.rows > 2 { 2 } else { 1 };
                u32::from(t.cols) * per_col
            }
            None => structs.get(n).filter(|_| depth < 8).map_or(1, |s| fields_slots(&s.fields, structs, env, depth + 1)),
        },
        TypeBase::Struct(s) => fields_slots(&s.fields, structs, env, depth + 1),
    };
    base * mult
}

fn fields_slots(fields: &[Field], structs: &HashMap<String, StructDef>, env: &crate::consteval::ConstEnv, depth: u32) -> u32 {
    fields
        .iter()
        .map(|f| f.names.iter().map(|(_, d)| slots(&f.ty, structs, env, depth) * dims_product(d, env)).sum::<u32>())
        .sum()
}

fn int_like(ty: &TypeSpec, structs: &HashMap<String, StructDef>) -> bool {
    match &ty.base {
        TypeBase::Named(n) => match GlslType::parse(n) {
            Some(t) => !matches!(t.scalar, ScalarKind::Float),
            None => structs.get(n).is_some_and(|s| s.fields.iter().any(|f| int_like(&f.ty, structs))),
        },
        TypeBase::Struct(s) => s.fields.iter().any(|f| int_like(&f.ty, structs)),
    }
}

/// The producer output a consumer input is matched with.
fn producer_name(input: &str, producer: ShaderStage, ctx: &Ctx) -> String {
    if let Some(rest) = input.strip_prefix("sb_vin_") {
        return format!("sb_v_{rest}");
    }
    if producer == ShaderStage::Vertex && ctx.varying_globals.contains_key(input) {
        return format!("sb_vary_{input}");
    }
    input.to_string()
}

fn same_type(a: &IVar, b: &IVar) -> bool {
    crate::print::type_spec(&a.elem) == crate::print::type_spec(&b.elem)
        && crate::print::array_dims(&a.dims) == crate::print::array_dims(&b.dims)
        && a.block.is_none() == b.block.is_none()
}

fn zero_value(ty: &TypeSpec) -> Option<String> {
    let n = ty.name()?;
    let t = GlslType::parse(n)?;
    Some(format!("{}(0)", t.glsl_name()))
}

/// Statement initializing `target` (of element type `elem` with `dims`) to zero.
fn zero_init(target: &str, elem: &TypeSpec, dims: &[ArrayDim], env: &crate::consteval::ConstEnv) -> Option<String> {
    let z = zero_value(elem)?;
    if dims.is_empty() {
        return Some(format!("{target} = {z};"));
    }
    let n = dims_product(dims, env);
    if dims.len() != 1 {
        return None;
    }
    Some(format!("for (int sb_i = 0; sb_i < {n}; sb_i++) {{ {target}[sb_i] = {z}; }}"))
}

/// Link all stages of a graphics program.
pub(crate) fn link(works: &mut [StageWork], ctx: &Ctx) {
    if ctx.opts.target == sb_core::model::OutputTarget::Renderpearl {
        // glslang's location auto-mapping (used by the host's shader compiler) gives a
        // block variable a location even when its members have one, which Vulkan
        // forbids: lay such blocks out member after member instead.
        for w in works.iter_mut() {
            for item in w.unit.items.iter_mut().chain(w.gen_iface.iter_mut()) {
                if let ItemKind::Block(b) = &mut item.kind {
                    for f in &mut b.fields {
                        strip_location(&mut f.quals);
                    }
                }
            }
        }
    }
    if works.len() < 2 || works.iter().any(|w| w.stage == ShaderStage::Compute) {
        if let Some(w) = works.first_mut()
            && w.stage != ShaderStage::Compute
        {
            // Lone stage: locations for whatever it declares.
            let inputs = collect(w, true);
            assign_sequential(w, &inputs);
        }
        return;
    }
    for c in (1..works.len()).rev() {
        let (left, right) = works.split_at_mut(c);
        resolve(&mut left[c - 1], &mut right[0], ctx);
    }
    // First-stage inputs are vertex attributes (profile inputs), not varyings.
    for c in 1..works.len() {
        let (left, right) = works.split_at_mut(c);
        assign_locations(&mut left[c - 1], &mut right[0]);
    }
}

fn assign_sequential(w: &mut StageWork, vars: &[IVar]) {
    if w.stage == ShaderStage::Vertex {
        return;
    }
    let structs = struct_defs(w);
    let env = crate::consteval::global_consts(&w.unit);
    let mut next = 0u32;
    for v in vars {
        let n = slot_count(v, &structs, &env);
        let flat = v.flat || int_like(&v.elem, &structs);
        set_location(item_mut(w, v), next, flat, &env);
        next += n;
    }
}

fn slot_count(v: &IVar, structs: &HashMap<String, StructDef>, env: &crate::consteval::ConstEnv) -> u32 {
    let elem = match &v.block {
        Some(fields) => member_footprint(fields, structs, env).unwrap_or_else(|| fields_slots(fields, structs, env, 0)),
        None => slots(&v.elem, structs, env, 0),
    };
    elem * dims_product(&v.dims, env)
}

fn resolve(p: &mut StageWork, c: &mut StageWork, ctx: &Ctx) {
    let inputs = collect(c, true);
    let outputs = collect(p, false);
    let c_refs = crate::compat::referenced(&c.unit);
    let env = crate::consteval::global_consts(&c.unit);
    let mut consumed: BTreeSet<String> = BTreeSet::new();
    let mut remove_inputs: Vec<IVar> = Vec::new();
    for input in &inputs {
        let pname = producer_name(&input.name, p.stage, ctx);
        if let Some(out) = outputs.iter().find(|o| o.name == pname) {
            consumed.insert(pname.clone());
            if same_type(out, input) {
                continue;
            }
            repair_mismatch(p, out, input, &pname);
            continue;
        }
        let referenced = input.generated || c_refs.contains(&input.name) || input.block.is_some();
        if !referenced {
            remove_inputs.push(input.clone());
            continue;
        }
        consumed.insert(pname.clone());
        add_producer_output(p, input, &pname, ctx, &env);
    }
    // Unconsumed producer outputs become plain globals.
    for out in outputs.iter().filter(|o| !consumed.contains(&o.name)) {
        demote(item_mut(p, out));
    }
    remove_inputs.sort_by_key(|v| std::cmp::Reverse((v.generated, v.index)));
    for v in remove_inputs {
        if v.generated {
            c.gen_iface.remove(v.index);
        } else {
            c.unit.items.remove(v.index);
        }
    }
}

/// Producer declaration text for an output matching `input`.
fn output_decl(p: &StageWork, input: &IVar, name: &str) -> String {
    let flat = if input.flat { "flat " } else { "" };
    let pv = if p.stage == ShaderStage::TessControl { "[]" } else { "" };
    let dims = crate::print::array_dims(&input.dims);
    match &input.block {
        Some(fields) => {
            let body: Vec<String> = fields.iter().map(crate::print::field).collect();
            format!("{flat}out {name} {{ {} }} sb_blk_{name}{pv}{dims};", body.join(" "))
        }
        None => format!("{flat}out {} {name}{pv}{dims};", crate::print::type_spec(&input.elem)),
    }
}

fn add_producer_output(p: &mut StageWork, input: &IVar, pname: &str, ctx: &Ctx, env: &crate::consteval::ConstEnv) {
    let decl = output_decl(p, input, pname);
    p.add_iface(&decl);
    let is_generated = pname.starts_with("sb_v_") || pname.starts_with("sb_vary_") || ctx.varying_globals.contains_key(pname);
    match p.stage {
        ShaderStage::Vertex => {
            if let Some(g) = pname.strip_prefix("sb_vary_") {
                p.epilogue.push(format!("{pname} = {g};"));
            } else if pname == "sb_v_Color" {
                p.prologue.push("sb_v_Color = sb_gl_Color;".into());
            } else if input.block.is_none() {
                match zero_init(pname, &input.elem, &input.dims, env) {
                    Some(s) => p.prologue.push(s),
                    None => p.warn("xf.missing-varying", format!("cannot zero-initialize `{pname}`"), 0),
                }
                if !is_generated {
                    p.warn(
                        "xf.missing-varying",
                        format!("`{}` is read by the next stage but never written by the {} stage; it reads zero", input.name, p.stage),
                        0,
                    );
                }
            }
        }
        ShaderStage::Geometry if is_generated && input.block.is_none() => {
            // Forward from the first input vertex.
            let in_name = if let Some(rest) = pname.strip_prefix("sb_v_") { format!("sb_vin_{rest}") } else { format!("sb_vary_{pname}") };
            let dims = crate::print::array_dims(&input.dims);
            let flat = if input.flat { "flat " } else { "" };
            p.add_iface(&format!("{flat}in {} {in_name}[]{dims};", crate::print::type_spec(&input.elem)));
            p.emit_hooks.push(format!("{pname} = {in_name}[0];"));
        }
        _ => {
            if input.block.is_none()
                && let Some(s) = zero_init(pname, &input.elem, &input.dims, env)
            {
                if p.stage == ShaderStage::Geometry {
                    p.emit_hooks.insert(0, s);
                } else if p.stage != ShaderStage::TessControl {
                    p.prologue.push(s);
                }
            }
            if !is_generated {
                p.warn("xf.missing-varying", format!("`{}` is never written by the {} stage; it reads zero", input.name, p.stage), 0);
            }
        }
    }
}

/// Rule c: the producer keeps a temporary of its own type and converts at the end.
fn repair_mismatch(p: &mut StageWork, out: &IVar, input: &IVar, pname: &str) {
    let (Some(pt), Some(ct)) = (out.elem.name().and_then(GlslType::parse), input.elem.name().and_then(GlslType::parse)) else {
        p.error(
            "xf.varying-mismatch",
            format!("varying `{pname}` has different types in the {} stage and the next stage", p.stage),
            0,
        );
        return;
    };
    if out.per_vertex || !out.dims.is_empty() || !input.dims.is_empty() || pt.is_matrix() != ct.is_matrix() {
        p.error(
            "xf.varying-mismatch",
            format!("varying `{pname}` is declared as {} here but as {} in the next stage", pt.glsl_name(), ct.glsl_name()),
            0,
        );
        return;
    }
    let tmp = format!("sb_tmp_{pname}");
    crate::scope::walk_idents(&mut p.unit, &mut |n, occ| {
        if occ == crate::scope::Occ::GlobalRef && n == pname {
            *n = tmp.clone();
        }
    });
    // The output now has the consumer's type.
    let item = item_mut(p, out);
    if let ItemKind::Decl(d) = &mut item.kind {
        d.ty.ty = TypeSpec { base: TypeBase::Named(ct.glsl_name()), array: Vec::new() };
    }
    p.piece(Section::Late, &[&tmp], format!("{} {tmp};", pt.glsl_name()));
    let conv = crate::compat::convert(&tmp, pt, ct);
    let stmt = format!("{pname} = {conv};");
    if p.stage == ShaderStage::Geometry {
        p.emit_hooks.push(stmt);
    } else {
        p.epilogue.push(stmt);
    }
    p.warn(
        "xf.varying-type",
        format!("varying `{pname}` is {} in the {} stage but {} in the next stage; converted", pt.glsl_name(), p.stage, ct.glsl_name()),
        0,
    );
}

fn assign_locations(p: &mut StageWork, c: &mut StageWork) {
    let inputs = collect(c, true);
    let outputs = collect(p, false);
    let structs = struct_defs(c);
    let env = crate::consteval::global_consts(&c.unit);
    let p_structs = struct_defs(p);
    let p_env = crate::consteval::global_consts(&p.unit);
    let mut next = 0u32;
    let mut matched: BTreeSet<String> = BTreeSet::new();
    for input in &inputs {
        let n = slot_count(input, &structs, &env);
        let pname = outputs
            .iter()
            .find(|o| {
                o.name == input.name
                    || input.name.strip_prefix("sb_vin_").is_some_and(|r| o.name == format!("sb_v_{r}"))
                    || o.name == format!("sb_vary_{}", input.name)
            })
            .cloned();
        let flat = input.flat
            || int_like(&input.elem, &structs)
            || input.block.as_ref().is_some_and(|f| f.iter().any(|x| int_like(&x.ty, &structs)))
            || pname.as_ref().is_some_and(|o| o.flat || int_like(&o.elem, &p_structs));
        set_location(item_mut(c, input), next, flat, &env);
        if let Some(o) = &pname {
            set_location(item_mut(p, o), next, flat, &p_env);
            matched.insert(o.name.clone());
        }
        next += n;
    }
    if next > 32 {
        c.warn("xf.too-many-varyings", format!("the {} stage reads {next} varying locations (more than 32)", c.stage), 0);
    }
    // Outputs without a consumer (should have been demoted) still need locations.
    for o in outputs.iter().filter(|o| !matched.contains(&o.name)) {
        let n = slot_count(o, &p_structs, &p_env);
        let flat = o.flat || int_like(&o.elem, &p_structs);
        set_location(item_mut(p, o), next, flat, &p_env);
        next += n;
    }
}
