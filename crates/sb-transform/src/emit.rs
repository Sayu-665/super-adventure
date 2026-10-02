//! Phase C: assemble and print one translated stage (spec §3.1).

use std::collections::{BTreeMap, BTreeSet};

use sb_core::model::{OutputTarget, VertexInput};
use sb_core::{ShaderStage, SourceLocation};

use crate::ast::*;
use crate::program::{Ctx, Piece, Section, StageWork};
use crate::transform::TransformedStage;

/// Result of emitting one stage.
pub(crate) struct Emitted {
    pub stage: TransformedStage,
    /// Binding-table names of the resources the stage declares.
    pub resources: Vec<String>,
    pub frame_members: Vec<String>,
    pub draw_members: Vec<String>,
    pub vertex_inputs: Vec<VertexInput>,
}

fn add_idents(text: &str, out: &mut BTreeSet<String>) {
    for id in crate::text::identifiers(text) {
        out.insert(id.to_string());
    }
}

/// Every name reachable from the pack code, generated interface declarations and the
/// wrapper `main`, through the generated pieces (fixpoint).
pub(crate) fn reachable(w: &StageWork, _ctx: &Ctx) -> BTreeSet<String> {
    let mut reached = crate::compat::referenced(&w.unit);
    // Type names and array sizes in declarations.
    for item in w.unit.items.iter().chain(w.gen_iface.iter()) {
        let mut p = crate::print::Printer::new();
        if matches!(item.kind, ItemKind::Decl(_) | ItemKind::Block(_)) {
            p.item(item);
            add_idents(&p.finish().0, &mut reached);
        }
    }
    for s in w.prologue.iter().chain(&w.epilogue).chain(&w.emit_hooks) {
        add_idents(s, &mut reached);
    }
    let pack = crate::compat::pack_globals(&w.unit);
    let mut included: Vec<bool> = vec![false; w.pieces.len()];
    loop {
        let mut changed = false;
        for (i, p) in w.pieces.iter().enumerate() {
            if included[i] {
                continue;
            }
            if p.provides.iter().any(|n| reached.contains(n) && !pack.contains_key(n)) {
                included[i] = true;
                changed = true;
                let before = reached.len();
                add_idents(&p.text, &mut reached);
                let _ = before;
            }
        }
        if !changed {
            break;
        }
    }
    reached
}

fn included_pieces<'p>(w: &'p StageWork, reached: &BTreeSet<String>) -> Vec<&'p Piece> {
    let pack = crate::compat::pack_globals(&w.unit);
    let mut seen: BTreeSet<(Section, &str)> = BTreeSet::new();
    let mut out: Vec<&Piece> = Vec::new();
    for p in &w.pieces {
        if p.provides.iter().any(|n| reached.contains(n) && !pack.contains_key(n)) {
            // The first piece providing a name in a section wins.
            if p.provides.iter().all(|n| seen.contains(&(p.section, n.as_str()))) {
                continue;
            }
            for n in &p.provides {
                seen.insert((p.section, n));
            }
            out.push(p);
        }
    }
    out.sort_by_key(|p| p.section);
    out
}

fn member_block(
    out: &mut String,
    name: &str,
    binding: u32,
    members: &[sb_core::model::BlockMember],
    used: &BTreeSet<String>,
    vulkan: bool,
) -> Vec<String> {
    let chosen: Vec<&sb_core::model::BlockMember> = members.iter().filter(|m| used.contains(&m.name)).collect();
    if chosen.is_empty() {
        return Vec::new();
    }
    let layout = if vulkan { format!("layout(std140, set = {}, binding = {binding})", sb_uniforms::UNIFORM_SET) } else { "layout(std140)".into() };
    out.push_str(&format!("{layout} uniform {name} {{\n"));
    for m in &chosen {
        let arr = m.ty.array.map(|n| format!("[{n}]")).unwrap_or_default();
        out.push_str(&format!("    layout(offset = {}) {} {}{arr};\n", m.offset, m.ty.glsl_name(), m.name));
    }
    out.push_str("};\n");
    chosen.iter().map(|m| m.name.clone()).collect()
}

/// Assemble the final text of a stage.
pub(crate) fn emit_stage(w: &mut StageWork, ctx: &Ctx) -> Emitted {
    let vulkan = ctx.opts.target == OutputTarget::Vulkan;
    let reached = reachable(w, ctx);
    let pack_globals = crate::compat::pack_globals(&w.unit);
    let pieces: Vec<Piece> = included_pieces(w, &reached).into_iter().cloned().collect();

    // Header text (generated, unmapped).
    let mut head = String::from("#version 460\n");
    let mut exts: BTreeSet<String> = w.extensions.clone();
    for e in &w.src.extensions {
        if crate::names::is_kept_extension(&e.name) {
            if e.behavior != "disable" {
                exts.insert(e.name.clone());
            }
        } else if !crate::names::CORE_EXTENSIONS.contains(&e.name.as_str()) && e.behavior != "disable" {
            w.diags.push(
                sb_core::Diagnostic::warning("xf.extension", format!("extension `{}` is not supported for Vulkan and was dropped", e.name))
                    .in_stage(w.stage),
            );
        }
    }
    for e in &exts {
        head.push_str(&format!("#extension {e} : enable\n"));
    }
    // Members are shadowed by pack globals, generated pieces and interface variables.
    let mut shadowing: BTreeSet<String> = pieces.iter().flat_map(|p| p.provides.iter().cloned()).collect();
    for item in &w.gen_iface {
        if let ItemKind::Decl(d) = &item.kind {
            shadowing.extend(d.vars.iter().map(|v| v.name.clone()));
        }
    }
    let used_members: BTreeSet<String> =
        reached.iter().filter(|n| !pack_globals.contains_key(*n) && !shadowing.contains(*n)).cloned().collect();
    let frame = member_block(&mut head, sb_uniforms::FRAME_BLOCK_NAME, sb_uniforms::FRAME_BINDING, &ctx.pack.layout.frame.members, &used_members, vulkan);
    let draw = member_block(&mut head, sb_uniforms::DRAW_BLOCK_NAME, sb_uniforms::DRAW_BINDING, &ctx.pack.layout.draw.members, &used_members, vulkan);

    let mut printer = crate::print::Printer::new();
    printer.text(&head, 0);
    let mut emitted_late = false;
    for p in pieces.iter().filter(|p| p.section < Section::Constants) {
        printer.text(&p.text, 0);
    }
    for item in &w.gen_iface {
        printer.item(item);
    }
    for p in pieces.iter().filter(|p| p.section >= Section::Constants && p.section != Section::Tail) {
        printer.text(&p.text, 0);
        emitted_late = true;
    }
    let _ = emitted_late;
    let redeclares_per_vertex = w.unit.items.iter().any(|i| matches!(&i.kind, ItemKind::Block(b) if b.name == "gl_PerVertex"));
    if ctx.last_pre_raster == Some(w.stage) && !redeclares_per_vertex {
        printer.line("invariant gl_Position;", 0);
    }
    for item in &w.unit.items {
        printer.item(item);
    }
    for p in pieces.iter().filter(|p| p.section == Section::Tail) {
        printer.text(&p.text, 0);
    }
    // Renderpearl: outputs are renumbered by first use; touch them in location order.
    let mut main = String::from("void main() {\n");
    if !vulkan && w.stage == ShaderStage::Fragment && !w.frag_outputs.is_empty() {
        let mut touch = String::from("void sb_touchOutputs() {\n");
        let mut by_loc: BTreeMap<u32, (String, String)> = BTreeMap::new();
        for (loc, _, name) in &w.frag_outputs {
            by_loc.entry(*loc).or_insert_with(|| (name.clone(), String::new()));
        }
        let max = by_loc.keys().max().copied().unwrap_or(0);
        let mut decls = String::new();
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for loc in 0..=max {
            match by_loc.get(&loc) {
                Some((name, _)) => {
                    let k = counts.entry(name.clone()).or_default();
                    let ty = output_type(w, name).unwrap_or_else(|| "vec4".into());
                    let is_array = w.frag_outputs.iter().filter(|o| &o.2 == name).count() > 1;
                    let target = if is_array { format!("{name}[{k}]") } else { name.clone() };
                    *k += 1;
                    touch.push_str(&format!("    {target} = {ty}(0);\n"));
                }
                None => {
                    decls.push_str(&format!("layout(location = {loc}) out vec4 sb_Unused{loc};\n"));
                    touch.push_str(&format!("    sb_Unused{loc} = vec4(0.0);\n"));
                }
            }
        }
        touch.push_str("}\n");
        printer.text(&decls, 0);
        printer.text(&touch, 0);
        main.push_str("    sb_touchOutputs();\n");
    }
    for s in &w.prologue {
        main.push_str(&format!("    {s}\n"));
    }
    main.push_str("    sb_user_main();\n");
    for s in &w.epilogue {
        main.push_str(&format!("    {s}\n"));
    }
    main.push_str("}\n");
    printer.text(&main, 0);

    let (glsl, lines) = printer.finish();
    let line_map: Vec<Option<SourceLocation>> = lines.iter().map(|&l| w.loc(l)).collect();

    // Usage reports.
    let mut resources: BTreeSet<String> = BTreeSet::new();
    let piece_names: BTreeSet<&str> = pieces.iter().flat_map(|p| p.provides.iter().map(String::as_str)).collect();
    for r in &w.resources {
        if r.always || piece_names.contains(r.glsl_name.as_str()) {
            resources.insert(r.binding_name.clone());
        }
    }
    for s in &ctx.profile.samplers {
        if piece_names.contains(s.name.as_str())
            && let Some(name) = host_sampler_entry(ctx, s)
        {
            resources.insert(name);
        }
    }
    for b in &ctx.profile.blocks {
        if piece_names.contains(b.instance.as_str())
            && let Some(e) = ctx.pack.bindings.entries.iter().find(|e| e.name == b.name)
        {
            resources.insert(e.name.clone());
        }
    }
    let mut vertex_inputs = Vec::new();
    if w.stage == ShaderStage::Vertex {
        for i in &ctx.profile.inputs {
            if piece_names.contains(i.name.as_str()) {
                let semantic = crate::profiles::SEMANTIC_KEYS
                    .iter()
                    .find(|(k, _)| crate::text::identifiers(ctx.profile.semantic(k)).any(|id| id == i.name))
                    .map(|(k, _)| (*k).to_string());
                vertex_inputs.push(VertexInput { location: i.location, name: i.name.clone(), ty: i.ty.clone(), semantic });
            }
        }
        vertex_inputs.sort_by_key(|v| v.location);
    }
    Emitted {
        stage: TransformedStage { stage: w.stage, glsl, line_map },
        resources: resources.into_iter().collect(),
        frame_members: frame,
        draw_members: draw,
        vertex_inputs,
    }
}

fn output_type(w: &StageWork, name: &str) -> Option<String> {
    for item in w.gen_iface.iter().chain(&w.unit.items) {
        if let ItemKind::Decl(d) = &item.kind
            && d.vars.first().is_some_and(|v| v.name == name)
        {
            return d.ty.ty.name().map(str::to_string);
        }
    }
    None
}

fn host_sampler_entry(ctx: &Ctx, s: &crate::profiles::ProfileSampler) -> Option<String> {
    let kind = sb_uniforms::sampler_kind(&s.ty)?;
    for provided in &s.provides {
        let c = sb_uniforms::canonicalize_with_kind(provided, &kind, &ctx.res_ctx);
        if let Some(e) = sb_uniforms::find_binding(ctx.pack.bindings, &c, &kind) {
            return Some(e.name.clone());
        }
    }
    ctx.pack.bindings.get(&s.name).map(|e| e.name.clone())
}
