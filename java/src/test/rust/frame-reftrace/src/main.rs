//! Reference frame trace of sb-runtime for a CompiledPack JSON: the pass order, flip state,
//! attachment images and main/alt binding choices of two frames, as `Executor::record_frame`
//! (crates/sb-runtime/src/frame.rs) produces them when every program prepares and draws. The
//! flip bookkeeping is sb-runtime's own `flips.rs`; the loop is a line-by-line transcription of
//! `record_frame`, `geometry_pass`, `fullscreen` and `end_of_frame` without the GPU calls.
//!
//! Usage: sb-frame-reftrace <pack.json> [folder]  (prints the trace to stdout)

#[path = "../../../../../../crates/sb-runtime/src/flips.rs"]
#[allow(dead_code)]
mod flips;

use flips::Flips;
use sb_core::PassGroup;
use sb_core::model::{CompiledPack, DimensionPipeline, Pass, ProgramKind, ResourceRef};
use sb_core::program::GeometryGroup;
use std::collections::{BTreeMap, BTreeSet};

struct Trace<'a> {
    dim: &'a DimensionPipeline,
    flips: Flips,
    color: BTreeSet<u32>,
    shadow_color: BTreeMap<u32, bool>,
    out: Vec<String>,
}

fn img(k: usize) -> &'static str {
    if k == 0 { "main" } else { "alt" }
}

fn group_name(g: PassGroup) -> String {
    serde_json::to_value(g).unwrap().as_str().unwrap().to_string()
}

fn list(v: &[u32]) -> String {
    v.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
}

impl<'a> Trace<'a> {
    fn new(dim: &'a DimensionPipeline) -> Self {
        // executor.rs `build`: which colortex / shadowcolor targets exist.
        let mut used: BTreeSet<u32> = dim.targets.colortex.iter().filter(|t| t.used).map(|t| t.index).collect();
        used.extend(dim.gbuffer_attachments.iter().copied());
        used.insert(0);
        for p in &dim.programs {
            let shadowish = matches!(p.kind, ProgramKind::Geometry { program } if program.group() == GeometryGroup::Shadow)
                || matches!(p.kind, ProgramKind::Composite { group: PassGroup::ShadowComp, .. });
            if !shadowish {
                used.extend(p.draw_buffers.iter().copied());
            }
            for b in &p.bindings_used {
                if let Some(e) = dim.bindings.get(&b.name)
                    && let ResourceRef::ColorTex(i) | ResourceRef::ColorImage(i) = e.resource
                {
                    used.insert(i);
                }
            }
        }
        for e in &dim.bindings.entries {
            if let ResourceRef::ColorTex(i) | ResourceRef::ColorImage(i) = e.resource {
                used.insert(i);
            }
        }
        used.retain(|&i| i < sb_uniforms::MAX_COLOR_TEX);
        let mut shadow_used: BTreeSet<u32> = dim.targets.shadowcolor.iter().filter(|t| t.used).map(|t| t.index).collect();
        shadow_used.extend(dim.shadow_attachments.iter().copied());
        for p in &dim.programs {
            let shadowish = matches!(p.kind, ProgramKind::Geometry { program } if program.group() == GeometryGroup::Shadow)
                || matches!(p.kind, ProgramKind::Composite { group: PassGroup::ShadowComp, .. });
            if shadowish {
                shadow_used.extend(p.draw_buffers.iter().copied());
            }
        }
        for e in &dim.bindings.entries {
            if let ResourceRef::ShadowColor(i) | ResourceRef::ShadowColorImage(i) = e.resource {
                shadow_used.insert(i);
            }
        }
        shadow_used.retain(|&i| i < sb_uniforms::MAX_SHADOW_COLOR);
        let shadow_color = shadow_used
            .into_iter()
            .map(|i| (i, dim.targets.shadowcolor.iter().find(|t| t.index == i).map_or(true, |t| t.clear)))
            .collect();
        Self { dim, flips: Flips::default(), color: used, shadow_color, out: Vec::new() }
    }

    fn line(&mut self, s: String) {
        self.out.push(s);
    }

    fn record_frame(&mut self, frame: u32) {
        self.line(format!("frame {frame}"));
        self.flips.reset();
        let dim = self.dim;
        let geometry_groups = [PassGroup::Shadow, PassGroup::GbuffersOpaque, PassGroup::GbuffersTranslucent];
        let mut done = [false; 3];
        let mut final_done = false;
        let mut prev_group = None;
        for pass in &dim.passes {
            for (k, g) in geometry_groups.iter().enumerate() {
                if !done[k] && *g < pass.group {
                    self.line(format!("pass {} implicit", group_name(*g)));
                    self.geometry_pass(*g, None);
                    done[k] = true;
                }
            }
            self.line(format!("pass {} {}", group_name(pass.group), pass.index));
            self.adopt_flip_state(pass, prev_group != Some(pass.group));
            prev_group = Some(pass.group);
            match pass.group {
                PassGroup::Setup => {
                    if frame == 0 {
                        self.run_computes(pass);
                    }
                }
                g @ (PassGroup::Shadow | PassGroup::GbuffersOpaque | PassGroup::GbuffersTranslucent) => {
                    let k = geometry_groups.iter().position(|x| *x == g).unwrap_or(0);
                    if done[k] {
                        self.run_computes(pass);
                    } else {
                        self.geometry_pass(g, Some(pass));
                        done[k] = true;
                    }
                }
                group => {
                    self.run_computes(pass);
                    if let Some(p) = pass.program {
                        let ran = self.fullscreen(p, group);
                        if group == PassGroup::Final && ran {
                            final_done = true;
                        }
                    }
                }
            }
            if pass.group != PassGroup::ShadowComp && !pass.flips_after.is_empty() {
                self.line(format!("flip {}", list(&pass.flips_after)));
            }
            if pass.group != PassGroup::ShadowComp {
                self.flips.flip(&pass.flips_after);
            }
        }
        for (k, g) in geometry_groups.iter().enumerate() {
            if !done[k] {
                self.line(format!("pass {} implicit", group_name(*g)));
                self.geometry_pass(*g, None);
            }
        }
        if !final_done {
            let k = self.flips.read(0);
            self.line(format!("copy-to-output colortex0:{}", img(k)));
        }
        self.end_of_frame();
    }

    fn adopt_flip_state(&mut self, pass: &Pass, first_of_group: bool) {
        if pass.flip_state.is_empty() {
            return;
        }
        let color = &self.color;
        let mismatches = self.flips.adopt(&pass.flip_state, |i| color.contains(&i));
        if !mismatches.is_empty() && !first_of_group {
            self.line("warn flip_state".into());
        }
    }

    fn run_computes(&mut self, pass: &Pass) {
        if !pass.computes.is_empty() {
            let names: Vec<String> = pass.computes.iter().map(|&c| self.dim.programs[c as usize].name.clone()).collect();
            self.line(format!("computes {}", names.join(",")));
        }
    }

    fn geometry_pass(&mut self, group: PassGroup, pass: Option<&Pass>) {
        if let Some(p) = pass {
            self.run_computes(p);
        }
        let dim = self.dim;
        match group {
            PassGroup::Shadow => {
                if !dim.targets.shadow.enabled {
                    return;
                }
                let atts: Vec<String> =
                    dim.shadow_attachments.iter().map(|&t| format!("{t}:{}", img(self.flips.shadow_read(t)))).collect();
                self.line(format!("geometry shadow attachments={}", atts.join(",")));
                self.line("copy shadowtex0->shadowtex1".into());
            }
            PassGroup::GbuffersOpaque => {
                let atts: Vec<String> = dim.gbuffer_attachments.iter().map(|&t| format!("{t}:{}", img(self.flips.read(t)))).collect();
                self.line(format!("geometry gbuffers_opaque attachments={}", atts.join(",")));
                self.line("copy depthtex0->depthtex2,depthtex1 dhDepthTex0->dhDepthTex1".into());
            }
            _ => {
                let atts: Vec<String> = dim.gbuffer_attachments.iter().map(|&t| format!("{t}:{}", img(self.flips.read(t)))).collect();
                self.line(format!("geometry gbuffers_translucent attachments={}", atts.join(",")));
            }
        }
    }

    /// `fullscreen`: attachments by location, written targets, bindings (descriptors.rs
    /// `color_read`: composite-style programs read `use_alt`; shadowcolor reads follow the
    /// shadow flip state).
    fn fullscreen(&mut self, index: u32, group: PassGroup) -> bool {
        let dim = self.dim;
        let program = &dim.programs[index as usize];
        let shadow = group == PassGroup::ShadowComp;
        let mut writes = Vec::new();
        let mut written = Vec::new();
        if group == PassGroup::Final {
            writes.push("output".to_string());
        } else {
            let mut assigned: Vec<u32> = Vec::new();
            for (i, &t) in program.draw_buffers.iter().enumerate() {
                let loc = program.output_slots.get(i).copied().unwrap_or(i as u32);
                let exists = if shadow { self.shadow_color.contains_key(&t) } else { self.color.contains(&t) };
                if exists && !assigned.contains(&t) {
                    assigned.push(t);
                    let k = if shadow { self.flips.shadow_write(t) } else { self.flips.write(t) };
                    writes.push(format!("{loc}={t}:{}", img(k)));
                    written.push(t);
                } else {
                    writes.push(format!("{loc}=sink"));
                }
            }
        }
        if writes.is_empty() {
            self.line(format!("draw {} nothing", program.name));
            return false;
        }
        let mut reads = Vec::new();
        for b in &program.bindings_used {
            let Some(e) = dim.bindings.get(&b.name) else { continue };
            match e.resource {
                ResourceRef::ColorTex(i) | ResourceRef::ColorImage(i) => {
                    if !self.color.contains(&i) {
                        continue;
                    }
                    let state = self.flips.read(i);
                    if usize::from(b.use_alt) != state {
                        self.out.push(format!("warn use_alt {}", b.name));
                    }
                    reads.push(format!("{}={i}:{}", b.name, img(usize::from(b.use_alt))));
                }
                ResourceRef::ShadowColor(i) | ResourceRef::ShadowColorImage(i) => {
                    if self.shadow_color.contains_key(&i) {
                        reads.push(format!("{}=shadowcolor{i}:{}", b.name, img(self.flips.shadow_read(i))));
                    }
                }
                _ => {}
            }
        }
        self.line(format!("draw {} writes={} reads={}", program.name, writes.join(","), reads.join(",")));
        if shadow {
            self.flips.flip_shadow(&written);
            if !written.is_empty() {
                self.line(format!("shadow-flip {}", list(&written)));
            }
        }
        true
    }

    fn end_of_frame(&mut self) {
        let dim = self.dim;
        for i in self.flips.end_of_frame_copies(&dim.end_of_frame_copies) {
            if self.color.contains(&i) {
                self.line(format!("eof colortex{i}"));
            }
        }
        let keep: Vec<u32> = self.shadow_color.iter().filter(|(_, clear)| !**clear).map(|(i, _)| *i).collect();
        for i in self.flips.shadow_copies(&keep) {
            self.line(format!("eof shadowcolor{i}"));
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let json = std::fs::read_to_string(&args[1]).expect("read pack.json");
    let pack = CompiledPack::from_json(&json).expect("parse pack.json");
    let folder = args.get(2).cloned().unwrap_or_else(|| pack.dimensions[0].folder.clone());
    let dim = pack.dimensions.iter().find(|d| d.folder == folder).expect("folder");
    let mut t = Trace::new(dim);
    t.record_frame(0);
    t.record_frame(1);
    for l in t.out {
        println!("{l}");
    }
}
