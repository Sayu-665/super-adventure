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
                    // Outermost first: `vec4[2] x[3]` is `vec4 x[3][2]`.
                    let mut dims = v.array.clone();
                    dims.extend(d.ty.ty.array.iter().cloned());
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
        // A producer output the consumer reads smoothly (see `assign_locations`).
        if let ItemKind::Decl(d) = &mut item.kind {
            d.ty.quals.retain(|x| !matches!(x, Qualifier::Interp(Interp::Flat)));
        }
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

/// The producer output a consumer input is matched with. Generated varyings travel
/// between pre-rasterization stages under their output names and are read through
/// per-vertex inputs with distinct names: fixed-function varyings as `sb_v_X` /
/// `sb_vin_X[]`, varying profile globals `g` as `sb_vary_g` / `sb_varyin_g[]` (the
/// fragment stage reads `g` itself).
fn producer_name(input: &str, ctx: &Ctx) -> String {
    if let Some(rest) = input.strip_prefix("sb_vin_") {
        return format!("sb_v_{rest}");
    }
    if let Some(rest) = input.strip_prefix("sb_varyin_") {
        return format!("sb_vary_{rest}");
    }
    if ctx.varying_globals.contains_key(input) {
        return format!("sb_vary_{input}");
    }
    input.to_string()
}

/// The per-vertex input an intermediate stage forwards the generated varying `pname`
/// from (see [`producer_name`]).
fn forwarded_input(pname: &str) -> Option<String> {
    if let Some(rest) = pname.strip_prefix("sb_v_") {
        return Some(format!("sb_vin_{rest}"));
    }
    pname.strip_prefix("sb_vary_").map(|rest| format!("sb_varyin_{rest}"))
}

/// Whether the stage already declares an interface variable named `name`.
fn declares(w: &StageWork, name: &str) -> bool {
    w.gen_iface.iter().chain(&w.unit.items).any(|i| matches!(&i.kind, ItemKind::Decl(d) if d.vars.iter().any(|v| v.name == name)))
}

fn same_type(a: &IVar, b: &IVar) -> bool {
    crate::print::type_spec(&a.elem) == crate::print::type_spec(&b.elem)
        && crate::print::array_dims(&a.dims) == crate::print::array_dims(&b.dims)
        && a.block.as_ref().map(|f| block_shape(f)) == b.block.as_ref().map(|f| block_shape(f))
}

/// Member types of an interface block in order. Vulkan matches interfaces by location,
/// so member names may differ between stages (hygiene renames a member of an anonymous
/// block that collides with a profile name in one stage only); types and order may not.
fn block_shape(fields: &[Field]) -> Vec<String> {
    fields
        .iter()
        .flat_map(|f| f.names.iter().map(move |(_, d)| format!("{}{}", crate::print::type_spec(&f.ty), crate::print::array_dims(d))))
        .collect()
}

/// Member names and types of an interface block (qualifiers ignored), for messages.
fn block_members(fields: &[Field]) -> Vec<String> {
    fields
        .iter()
        .flat_map(|f| f.names.iter().map(move |(n, d)| format!("{} {n}{}", crate::print::type_spec(&f.ty), crate::print::array_dims(d))))
        .collect()
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

/// Renderpearl target, vertex + fragment programs: Mojang's `PipelineBuilder` (26.3)
/// rejects interface variables of struct type and requires a `Location` decoration on
/// every interface variable, so in/out interface blocks become one variable per member:
/// `sb_ib_<Block>_<k>` for the k-th member (by position, as Vulkan matches block
/// members), with the block's and the member's interpolation and auxiliary qualifiers.
/// Member references (`inst.member`, or the bare member name of an instance-less block)
/// follow. Arrayed block instances are left alone (see [`renderpearl_inexpressible`]).
fn flatten_interface_blocks(w: &mut StageWork) {
    let stage = w.stage;
    let mut renames: HashMap<String, Vec<(String, String)>> = HashMap::new(); // instance -> (member, new)
    let mut bare: HashMap<String, String> = HashMap::new(); // member of an instance-less block -> new
    let mut items = Vec::with_capacity(w.unit.items.len());
    for item in std::mem::take(&mut w.unit.items) {
        let line = item.line;
        let ItemKind::Block(b) = &item.kind else {
            items.push(item);
            continue;
        };
        let iface = crate::analyze::is_input(stage, &b.quals) || crate::analyze::is_output(stage, &b.quals);
        if !iface || b.name.starts_with("gl_") {
            items.push(item);
            continue;
        }
        if b.instance.as_ref().is_some_and(|(_, d)| !d.is_empty()) {
            // Arrayed instances: see `renderpearl_inexpressible`.
            items.push(item);
            continue;
        }
        let block_quals: Vec<Qualifier> = b.quals.iter().filter(|q| !matches!(q, Qualifier::Layout(_))).cloned().collect();
        let mut k = 0usize;
        let mut members = Vec::new();
        for f in &b.fields {
            for (name, dims) in &f.names {
                let new = format!("sb_ib_{}_{k}", b.name);
                k += 1;
                let mut quals: Vec<Qualifier> = f.quals.iter().filter(|q| !matches!(q, Qualifier::Layout(_))).cloned().collect();
                for q in &block_quals {
                    if !quals.contains(q) {
                        quals.push(q.clone());
                    }
                }
                let decl = Declaration {
                    ty: FullType { quals, ty: f.ty.clone() },
                    vars: vec![Declarator { name: new.clone(), array: dims.clone(), init: None }],
                };
                items.push(Item { kind: ItemKind::Decl(decl), line });
                members.push((name.clone(), new));
            }
        }
        match &b.instance {
            Some((inst, _)) => {
                renames.insert(inst.clone(), members);
            }
            None => bare.extend(members),
        }
    }
    w.unit.items = items;
    if renames.is_empty() && bare.is_empty() {
        return;
    }
    let rewrite = |e: &mut Expr, is_local: &dyn Fn(&str) -> bool| {
        e.walk_mut(&mut |x| {
            match x {
                Expr::Field(base, member) => {
                    if let Some(inst) = base.as_ident()
                        && !is_local(inst)
                        && let Some(new) = renames.get(inst).and_then(|m| m.iter().find(|(n, _)| n == member)).map(|(_, n)| n.clone())
                    {
                        *x = Expr::Ident(new);
                        return Walk::Skip;
                    }
                }
                Expr::Ident(n) if !is_local(n) => {
                    if let Some(new) = bare.get(n.as_str()) {
                        *n = new.clone();
                    }
                }
                _ => {}
            }
            Walk::Children
        });
    };
    for item in &mut w.unit.items {
        match &mut item.kind {
            ItemKind::Function(f) => crate::scope::walk_function_exprs(f, &mut |root, is_local| rewrite(root, is_local)),
            ItemKind::Decl(d) => d.walk_exprs_mut(&mut |e| {
                rewrite(e, &|_| false);
                Walk::Skip
            }),
            _ => {}
        }
    }
}

/// A non-struct component of a struct-typed varying: access path from the variable
/// (`.a.b[2].c`), type and its own array dimensions.
struct Leaf {
    path: String,
    ty: TypeSpec,
    dims: Vec<ArrayDim>,
}

/// The leaves of a value of struct `name` (struct members of struct type are expanded,
/// arrays of structs element by element). `None` when a struct array has no constant
/// length or the nesting is too deep.
fn struct_leaves(name: &str, structs: &HashMap<String, StructDef>, env: &crate::consteval::ConstEnv, depth: u32) -> Option<Vec<Leaf>> {
    let def = structs.get(name).filter(|_| depth < 8)?;
    let mut out = Vec::new();
    for f in &def.fields {
        for (member, dims) in &f.names {
            let mut all_dims = f.ty.array.clone();
            all_dims.extend(dims.iter().cloned());
            let elem = TypeSpec { base: f.ty.base.clone(), array: Vec::new() };
            let inner = match &elem.base {
                TypeBase::Named(n) if structs.contains_key(n) => Some(n.clone()),
                TypeBase::Struct(_) => return None,
                _ => None,
            };
            match inner {
                None => out.push(Leaf { path: format!(".{member}"), ty: elem, dims: all_dims }),
                Some(s) => {
                    let sub = struct_leaves(&s, structs, env, depth + 1)?;
                    let indices: Vec<String> = match all_dims.as_slice() {
                        [] => vec![String::new()],
                        [ArrayDim::Sized(e)] => (0..crate::consteval::array_len(e, env)?).map(|i| format!("[{i}]")).collect(),
                        _ => return None,
                    };
                    for idx in indices {
                        for l in &sub {
                            out.push(Leaf { path: format!(".{member}{idx}{}", l.path), ty: l.ty.clone(), dims: l.dims.clone() });
                        }
                    }
                }
            }
        }
    }
    Some(out)
}

/// Renderpearl target, vertex + fragment programs: struct-typed varyings (rejected by
/// Mojang's pipeline builder like interface blocks) travel as one varying per leaf
/// member, `sb_is_<var>_<k>`; the pack keeps a plain global of the struct type, copied
/// to the leaves after `main` (producer) or from them before `main` (consumer). Struct
/// varyings that cannot be expanded (struct arrays without a constant length) are left
/// alone (see [`renderpearl_inexpressible`]).
fn flatten_struct_varyings(w: &mut StageWork) {
    let stage = w.stage;
    let structs = struct_defs(w);
    if structs.is_empty() {
        return;
    }
    let env = crate::consteval::global_consts(&w.unit);
    let mut new_items: Vec<(usize, Item)> = Vec::new();
    let mut copies: Vec<String> = Vec::new();
    for (index, item) in w.unit.items.iter_mut().enumerate() {
        let line = item.line;
        let ItemKind::Decl(d) = &mut item.kind else { continue };
        let input = crate::analyze::is_input(stage, &d.ty.quals);
        if (!input && !crate::analyze::is_output(stage, &d.ty.quals)) || d.vars.len() != 1 {
            continue;
        }
        let Some(sname) = d.ty.ty.name().filter(|n| structs.contains_key(*n)).map(str::to_string) else { continue };
        let var = d.vars[0].name.clone();
        let mut dims = d.ty.ty.array.clone();
        dims.extend(d.vars[0].array.iter().cloned());
        let indices: Option<Vec<String>> = match dims.as_slice() {
            [] => Some(vec![String::new()]),
            [ArrayDim::Sized(e)] => crate::consteval::array_len(e, &env).map(|n| (0..n).map(|i| format!("[{i}]")).collect()),
            _ => None,
        };
        // Not expandable: see `renderpearl_inexpressible`.
        let (Some(indices), Some(leaves)) = (indices, struct_leaves(&sname, &structs, &env, 0)) else { continue };
        let quals: Vec<Qualifier> = d.ty.quals.iter().filter(|q| !matches!(q, Qualifier::Layout(_))).cloned().collect();
        let mut k = 0usize;
        for idx in &indices {
            for leaf in &leaves {
                let name = format!("sb_is_{var}_{k}");
                k += 1;
                let decl = Declaration {
                    ty: FullType { quals: quals.clone(), ty: leaf.ty.clone() },
                    vars: vec![Declarator { name: name.clone(), array: leaf.dims.clone(), init: None }],
                };
                new_items.push((index, Item { kind: ItemKind::Decl(decl), line }));
                let field = format!("{var}{idx}{}", leaf.path);
                copies.push(if input { format!("{field} = {name};") } else { format!("{name} = {field};") });
            }
        }
        // The pack's variable becomes a plain global.
        d.ty.quals.retain(|x| match x {
            Qualifier::Storage(s) => !matches!(s, Storage::In | Storage::Out | Storage::Centroid | Storage::Sample | Storage::Patch),
            Qualifier::Layout(_) | Qualifier::Interp(_) | Qualifier::Invariant => false,
            _ => true,
        });
    }
    // Leaves follow their variable, in order.
    let mut by_index: std::collections::BTreeMap<usize, Vec<Item>> = std::collections::BTreeMap::new();
    for (index, item) in new_items {
        by_index.entry(index).or_default().push(item);
    }
    for (index, items) in by_index.into_iter().rev() {
        for (k, item) in items.into_iter().enumerate() {
            w.unit.items.insert(index + 1 + k, item);
        }
    }
    if stage == ShaderStage::Fragment {
        // Before `main`: prepend in declaration order.
        let mut p = copies;
        p.append(&mut w.prologue);
        w.prologue = p;
    } else {
        w.epilogue.extend(copies);
    }
}

/// Whether a GLSL type name is 64-bit (rejected in interfaces by Mojang's pipeline
/// builder).
fn is_64bit(ty: &str) -> bool {
    ty == "double" || ty.starts_with("dvec") || ty.starts_with("dmat") || ty.contains("64")
}

/// A vertex output or fragment input of a vertex + fragment program that the Renderpearl
/// target cannot express even after [`flatten_interface_blocks`] and
/// [`flatten_struct_varyings`] (Mojang's 26.3 pipeline builder rejects 64-bit and
/// struct-typed interface variables): a 64-bit varying or member, an arrayed interface
/// block, or a struct varying that cannot be expanded. Returns a description.
fn renderpearl_inexpressible(w: &StageWork) -> Option<String> {
    let structs = struct_defs(w);
    let env = crate::consteval::global_consts(&w.unit);
    let wide = |ty: &TypeSpec| -> bool {
        let Some(n) = ty.name() else { return false };
        if is_64bit(n) {
            return true;
        }
        // Struct members (any depth).
        let mut stack = vec![n.to_string()];
        let mut seen = 0;
        while let Some(s) = stack.pop() {
            seen += 1;
            let Some(def) = structs.get(&s).filter(|_| seen < 64) else { continue };
            for f in &def.fields {
                if let Some(m) = f.ty.name() {
                    if is_64bit(m) {
                        return true;
                    }
                    stack.push(m.to_string());
                }
            }
        }
        false
    };
    for item in &w.unit.items {
        match &item.kind {
            ItemKind::Decl(d) if crate::analyze::is_input(w.stage, &d.ty.quals) || crate::analyze::is_output(w.stage, &d.ty.quals) => {
                for v in &d.vars {
                    if v.name.starts_with("gl_") {
                        continue;
                    }
                    if wide(&d.ty.ty) {
                        return Some(format!("varying `{}` has a 64-bit type", v.name));
                    }
                    if let Some(s) = d.ty.ty.name().filter(|n| structs.contains_key(*n)) {
                        let mut dims = d.ty.ty.array.clone();
                        dims.extend(v.array.iter().cloned());
                        let sized = match dims.as_slice() {
                            [] => true,
                            [ArrayDim::Sized(e)] => crate::consteval::array_len(e, &env).is_some(),
                            _ => false,
                        };
                        if !sized || struct_leaves(s, &structs, &env, 0).is_none() {
                            return Some(format!("struct varying `{}` cannot be expanded", v.name));
                        }
                    }
                }
            }
            ItemKind::Block(b) if !b.name.starts_with("gl_") && (crate::analyze::is_input(w.stage, &b.quals) || crate::analyze::is_output(w.stage, &b.quals)) => {
                if b.instance.as_ref().is_some_and(|(_, d)| !d.is_empty()) {
                    return Some(format!("interface block `{}` is arrayed", b.name));
                }
                if b.fields.iter().any(|f| wide(&f.ty)) {
                    return Some(format!("interface block `{}` has a 64-bit member", b.name));
                }
            }
            _ => {}
        }
    }
    None
}

/// Link all stages of a graphics program.
pub(crate) fn link(works: &mut [StageWork], ctx: &Ctx) {
    // Vertex + fragment programs are Renderpearl candidates; those with interfaces it
    // cannot express need the raw Vulkan path (decided the same way for every target).
    if works.iter().all(|w| matches!(w.stage, ShaderStage::Vertex | ShaderStage::Fragment)) {
        for w in works.iter_mut() {
            if let Some(why) = renderpearl_inexpressible(w) {
                w.requires_raw_vulkan = true;
                w.info("xf.renderpearl-interface", format!("{why}: Mojang's pipeline builder rejects it; the program needs the raw Vulkan path"), 0);
            }
        }
    }
    if ctx.opts.target == sb_core::model::OutputTarget::Renderpearl
        && works.iter().all(|w| matches!(w.stage, ShaderStage::Vertex | ShaderStage::Fragment))
    {
        for w in works.iter_mut() {
            flatten_interface_blocks(w);
            flatten_struct_varyings(w);
        }
    }
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
        assign_locations(&mut left[c - 1], &mut right[0], ctx);
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
    let p_refs = crate::compat::referenced(&p.unit);
    let env = crate::consteval::global_consts(&c.unit);
    let p_env = crate::consteval::global_consts(&p.unit);
    let mut consumed: BTreeSet<String> = BTreeSet::new();
    let mut remove_inputs: Vec<IVar> = Vec::new();
    for input in &inputs {
        let pname = producer_name(&input.name, ctx);
        if let Some(out) = outputs.iter().find(|o| o.name == pname) {
            consumed.insert(pname.clone());
            if same_type(out, input) {
                // Rule b: an output the producer never assigns (its name occurs only in
                // the declaration) but the consumer reads is zero instead of undefined.
                if !out.generated
                    && out.block.is_none()
                    && !p_refs.contains(&pname)
                    && c_refs.contains(&input.name)
                    && p.stage != ShaderStage::TessControl
                {
                    zero_unwritten(p, out, &pname, &p_env);
                }
                continue;
            }
            repair_mismatch(p, out, input, &pname, &p_env, &env);
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
    // Intermediate stages (tessellation, geometry) forward generated varyings: a varying
    // profile global the stage holds itself (it reads or writes it) with its own value,
    // anything else from its per-vertex input (the first vertex; the control point of
    // the invocation in a tessellation control stage).
    if p.stage != ShaderStage::Vertex
        && input.block.is_none()
        && let Some(in_name) = forwarded_input(pname)
    {
        let own = pname
            .strip_prefix("sb_vary_")
            .filter(|g| p.has_piece(g) || crate::compat::pack_globals(&p.unit).contains_key(*g))
            .map(str::to_string);
        let vertex = if p.stage == ShaderStage::TessControl { "gl_InvocationID" } else { "0" };
        let source = match own {
            Some(g) => g,
            None => {
                if !declares(p, &in_name) {
                    let dims = crate::print::array_dims(&input.dims);
                    let flat = if input.flat { "flat " } else { "" };
                    p.add_iface(&format!("{flat}in {} {in_name}[]{dims};", crate::print::type_spec(&input.elem)));
                }
                format!("{in_name}[{vertex}]")
            }
        };
        match p.stage {
            ShaderStage::Geometry => p.emit_hooks.push(format!("{pname} = {source};")),
            ShaderStage::TessControl => p.epilogue.push(format!("{pname}[gl_InvocationID] = {source};")),
            _ => p.epilogue.push(format!("{pname} = {source};")),
        }
        return;
    }
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

/// Rule b: zero-initialize an output the producer never writes.
fn zero_unwritten(p: &mut StageWork, out: &IVar, pname: &str, env: &crate::consteval::ConstEnv) {
    let Some(stmt) = zero_init(pname, &out.elem, &out.dims, env) else { return };
    if p.stage == ShaderStage::Geometry {
        p.emit_hooks.insert(0, stmt);
    } else {
        p.prologue.push(stmt);
    }
    p.info("xf.unwritten-varying", format!("`{pname}` is never written by the {} stage; it reads zero", p.stage), 0);
}

/// Constant length of a one-dimensional array interface variable.
fn array_len(dims: &[ArrayDim], env: &crate::consteval::ConstEnv) -> Option<u32> {
    match dims {
        [ArrayDim::Sized(e)] => crate::consteval::array_len(e, env),
        _ => None,
    }
}

/// Rule c: the producer keeps a temporary of its own type and converts at the end.
/// Arrays of different lengths take the consumer's length (missing elements read
/// zero).
fn repair_mismatch(
    p: &mut StageWork,
    out: &IVar,
    input: &IVar,
    pname: &str,
    p_env: &crate::consteval::ConstEnv,
    c_env: &crate::consteval::ConstEnv,
) {
    if let (Some(a), Some(b)) = (&out.block, &input.block) {
        // GLSL requires matching members; a pack that declares them differently (under
        // different conditions in each stage) fails to link on every driver.
        p.error(
            "xf.varying-mismatch",
            format!(
                "interface block `{pname}` has members [{}] in the {} stage but [{}] in the next stage",
                block_members(a).join("; "),
                p.stage,
                block_members(b).join("; ")
            ),
            0,
        );
        return;
    }
    let (Some(pt), Some(ct)) = (out.elem.name().and_then(GlslType::parse), input.elem.name().and_then(GlslType::parse)) else {
        p.error(
            "xf.varying-mismatch",
            format!("varying `{pname}` has different types in the {} stage and the next stage", p.stage),
            0,
        );
        return;
    };
    // Scalar/vector vs matrix classes cannot be converted; arrays need constant lengths.
    let lengths = match (out.dims.is_empty(), input.dims.is_empty()) {
        (true, true) => Some(None),
        (false, false) => array_len(&out.dims, p_env).zip(array_len(&input.dims, c_env)).map(Some),
        _ => None,
    };
    let (Some(lengths), false) = (lengths, out.per_vertex || pt.is_matrix() != ct.is_matrix()) else {
        let shape = |t: GlslType, d: &[ArrayDim]| format!("{}{}", t.glsl_name(), crate::print::array_dims(d));
        p.error(
            "xf.varying-mismatch",
            format!(
                "varying `{pname}` is declared as {} in the {} stage but as {} in the next stage",
                shape(pt, &out.dims),
                p.stage,
                shape(ct, &input.dims)
            ),
            0,
        );
        return;
    };
    let tmp = format!("sb_tmp_{pname}");
    crate::scope::walk_idents(&mut p.unit, &mut |n, occ| {
        if occ == crate::scope::Occ::GlobalRef && n == pname {
            *n = tmp.clone();
        }
    });
    // The output now has the consumer's type (and length).
    let item = item_mut(p, out);
    if let ItemKind::Decl(d) = &mut item.kind {
        d.ty.ty = TypeSpec { base: TypeBase::Named(ct.glsl_name()), array: Vec::new() };
        if let (Some((_, nc)), Some(v)) = (lengths, d.vars.first_mut()) {
            v.array = vec![ArrayDim::Sized(Expr::Int(nc as i32))];
        }
    }
    let mut stmts = Vec::new();
    match lengths {
        None => {
            // Zero-initialized: the pack may never write it (rule b).
            p.piece(Section::Late, &[&tmp], format!("{} {tmp} = {}(0);", pt.glsl_name(), pt.glsl_name()));
            stmts.push(format!("{pname} = {};", crate::compat::convert_varying(&tmp, pt, ct)));
        }
        Some((np, nc)) => {
            p.piece(Section::Late, &[&tmp], format!("{} {tmp}[{np}];", pt.glsl_name()));
            for k in 0..nc {
                let value = if k < np { crate::compat::convert_varying(&format!("{tmp}[{k}]"), pt, ct) } else { format!("{}(0)", ct.glsl_name()) };
                stmts.push(format!("{pname}[{k}] = {value};"));
            }
        }
    }
    if p.stage == ShaderStage::Geometry {
        p.emit_hooks.extend(stmts);
    } else {
        p.epilogue.extend(stmts);
    }
    let shape = |t: GlslType, n: Option<u32>| format!("{}{}", t.glsl_name(), n.map(|n| format!("[{n}]")).unwrap_or_default());
    p.warn(
        "xf.varying-type",
        format!(
            "varying `{pname}` is {} in the {} stage but {} in the next stage; converted",
            shape(pt, lengths.map(|l| l.0)),
            p.stage,
            shape(ct, lengths.map(|l| l.1))
        ),
        0,
    );
}

fn assign_locations(p: &mut StageWork, c: &mut StageWork, ctx: &Ctx) {
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
        let wanted = producer_name(&input.name, ctx);
        let pname = outputs.iter().find(|o| o.name == wanted || o.name == input.name).cloned();
        // The consumer's interpolation qualifier decides (GLSL 4.30+, Vulkan): a `flat`
        // producer output does not make a smooth fragment input flat. Integer varyings
        // must be flat on both sides.
        let flat = input.flat
            || int_like(&input.elem, &structs)
            || input.block.as_ref().is_some_and(|f| f.iter().any(|x| int_like(&x.ty, &structs)))
            || pname.as_ref().is_some_and(|o| int_like(&o.elem, &p_structs));
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
