//! Program-level orchestration: per-stage rewriting, linking, emission.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use sb_core::model::ResourceKind;
use sb_core::{Diagnostic, Diagnostics, ShaderStage, SourceLocation};
use sb_uniforms::ResourceContext;

use crate::analyze::AnalyzedStage;
use crate::ast::*;
use crate::profiles::DrawProfile;
use crate::transform::{PackContext, TransformOptions, TransformedProgram};

/// Shared, read-only state of one program translation.
pub(crate) struct Ctx<'a> {
    pub profile: &'a DrawProfile,
    pub pack: &'a PackContext<'a>,
    pub opts: &'a TransformOptions,
    /// Resource context for the program's class.
    pub res_ctx: ResourceContext,
    /// Stages present, in pipeline order.
    pub stages: Vec<ShaderStage>,
    /// The last stage before rasterization (gets the depth remap), if any.
    pub last_pre_raster: Option<ShaderStage>,
    /// Size of the `sb_v_TexCoord` array (max `gl_TexCoord` index + 1 over all stages).
    pub texcoord_count: u32,
    /// Varying profile globals: name -> GLSL type.
    pub varying_globals: BTreeMap<String, String>,
    /// All profile global names.
    pub profile_globals: BTreeMap<String, String>,
    /// Names the profile declares (inputs, blocks, samplers, helpers); pack declarations
    /// with these names are renamed.
    pub profile_names: BTreeSet<String>,
}

/// A generated global-scope declaration that is emitted only if referenced.
#[derive(Debug, Clone)]
pub(crate) struct Piece {
    /// Names the piece declares.
    pub provides: Vec<String>,
    /// GLSL text (one or more external declarations).
    pub text: String,
    /// Emission section (lower first).
    pub section: Section,
}

/// Emission order of generated declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Section {
    /// Host uniform blocks.
    HostBlocks,
    /// Host and pack samplers, images, pack SSBOs/UBOs.
    Resources,
    /// Profile vertex inputs.
    Inputs,
    /// `const int` profile constants.
    Constants,
    /// Profile helper functions.
    Helpers,
    /// Semantic globals (`sb_ModelView`, `sb_gl_Vertex`, ...).
    Semantics,
    /// Globals derived from semantics (`sb_ModelViewInverse`, attribute globals, ...).
    Derived,
    /// Profile globals (`mc_chunkFade`, `entityColor`, ...).
    ProfileGlobals,
    /// Stage-specific globals (gl_Fog, fragment data arrays, ...).
    Late,
    /// Function definitions emitted after the pack code (they reference pack globals).
    Tail,
}

/// A canonicalized opaque resource declared by the stage.
#[derive(Debug, Clone)]
pub(crate) struct ResourceUse {
    /// GLSL identifier in the translated code.
    pub glsl_name: String,
    /// Binding-table entry name.
    pub binding_name: String,
    /// Declared by pack code that is always emitted (blocks), not a pruned piece.
    pub always: bool,
}

/// Mutable state of one stage during translation.
pub(crate) struct StageWork<'a> {
    pub stage: ShaderStage,
    pub src: &'a AnalyzedStage,
    /// Pack code (rewritten in place).
    pub unit: TranslationUnit,
    /// Generated interface declarations (fixed-function varyings, fragment outputs,
    /// forwarded profile globals, linker additions). Linked with the pack's own.
    pub gen_iface: Vec<Item>,
    /// Generated pieces (emitted when referenced).
    pub pieces: Vec<Piece>,
    /// Statements run before `sb_user_main()`.
    pub prologue: Vec<String>,
    /// Statements run after `sb_user_main()`.
    pub epilogue: Vec<String>,
    /// Geometry stage: statements run before every `EmitVertex()`.
    pub emit_hooks: Vec<String>,
    /// Geometry stage: `gl_Position` remap (depth convention, Y flip) applied to the
    /// emitted vertex only: `gl_Position` is saved before and restored after every
    /// `EmitVertex()`, so a pack that keeps modifying it between emits (as lenient
    /// drivers allow) never sees the remap twice.
    pub emit_position: Vec<String>,
    /// Extensions to enable.
    pub extensions: BTreeSet<String>,
    /// Resources declared (by GLSL name).
    pub resources: Vec<ResourceUse>,
    /// Fragment outputs (physical location, base type, GLSL name).
    pub frag_outputs: Vec<(u32, String, String)>,
    /// GLSL type of every declared opaque resource (by GLSL name), after lowering
    /// (rectangle samplers are 2D samplers here).
    pub opaque_types: HashMap<String, String>,
    /// Declared rectangle samplers (by GLSL name), lowered to 2D samplers.
    pub rect_samplers: HashSet<String>,
    /// The program needs raw Vulkan (1D/3D samplers, images, SSBOs).
    pub requires_raw_vulkan: bool,
    /// The pack's `main` stays the entry point (a tessellation control `main` calling
    /// `barrier()`, which must stay in `main`): the prologue and epilogue run from
    /// `sb_prologue()` / `sb_epilogue()` calls inserted into it instead of a wrapper.
    pub inline_main: bool,
    pub diags: Diagnostics,
}

impl<'a> StageWork<'a> {
    fn new(src: &'a AnalyzedStage) -> Self {
        Self {
            stage: src.stage,
            src,
            unit: (*src.unit).clone(),
            gen_iface: Vec::new(),
            pieces: Vec::new(),
            prologue: Vec::new(),
            epilogue: Vec::new(),
            emit_hooks: Vec::new(),
            emit_position: Vec::new(),
            extensions: BTreeSet::new(),
            resources: Vec::new(),
            frag_outputs: Vec::new(),
            opaque_types: HashMap::new(),
            rect_samplers: HashSet::new(),
            requires_raw_vulkan: false,
            inline_main: false,
            diags: Diagnostics::new(),
        }
    }

    /// Original location of a parsed-text line.
    pub fn loc(&self, line: Line) -> Option<SourceLocation> {
        if line == 0 { None } else { self.src.location_of(line) }
    }

    pub fn warn(&mut self, code: &str, msg: impl Into<String>, line: Line) {
        let d = Diagnostic::warning(code, msg).at_opt(self.loc(line)).in_stage(self.stage);
        self.diags.push(d);
    }

    pub fn error(&mut self, code: &str, msg: impl Into<String>, line: Line) {
        let d = Diagnostic::error(code, msg).at_opt(self.loc(line)).in_stage(self.stage);
        self.diags.push(d);
    }

    pub fn info(&mut self, code: &str, msg: impl Into<String>, line: Line) {
        let d = Diagnostic::info(code, msg).at_opt(self.loc(line)).in_stage(self.stage);
        self.diags.push(d);
    }

    /// Add a generated piece.
    pub fn piece(&mut self, section: Section, provides: &[&str], text: impl Into<String>) {
        self.pieces.push(Piece { provides: provides.iter().map(|s| (*s).to_string()).collect(), text: text.into(), section });
    }

    /// Whether a piece providing `name` exists.
    pub fn has_piece(&self, name: &str) -> bool {
        self.pieces.iter().any(|p| p.provides.iter().any(|n| n == name))
    }

    /// Add a generated interface declaration (parsed from `text`).
    pub fn add_iface(&mut self, text: &str) {
        match crate::parse::parse_glsl(text, 460) {
            Ok(u) => self.gen_iface.extend(u.items.into_iter().map(|mut i| {
                i.line = 0;
                i
            })),
            Err(e) => self.error("xf.internal", format!("generated declaration `{text}` does not parse: {}", e.message), 0),
        }
    }
}

fn stage_order(s: ShaderStage) -> u8 {
    match s {
        ShaderStage::Vertex => 0,
        ShaderStage::TessControl => 1,
        ShaderStage::TessEval => 2,
        ShaderStage::Geometry => 3,
        ShaderStage::Fragment => 4,
        ShaderStage::Compute => 5,
    }
}

/// Max literal `gl_TexCoord[i]` index + 1 over the stages (8 when indexed dynamically).
fn texcoord_count(stages: &[&AnalyzedStage]) -> u32 {
    let mut n = 0u32;
    for s in stages {
        let uses = ["gl_TexCoord", "gl_TexCoordIn"].iter().any(|b| s.info.compat_builtins.contains(*b));
        if !uses {
            continue;
        }
        s.unit.walk_exprs(&mut |e| {
            let Expr::Index(base, idx) = e else { return Walk::Children };
            // `gl_TexCoord[i]`, and `gl_TexCoordIn[vertex][i]` in geometry shaders.
            let texcoord_index = match (base.as_ident(), base.as_ref()) {
                (Some("gl_TexCoord"), _) => Some(idx.as_ref()),
                (_, Expr::Index(inner, _)) if inner.as_ident() == Some("gl_TexCoordIn") => Some(idx.as_ref()),
                _ => None,
            };
            match texcoord_index {
                Some(Expr::Int(i)) if *i >= 0 && *i < 32 => n = n.max(*i as u32 + 1),
                Some(Expr::UInt(i)) if *i < 32 => n = n.max(*i + 1),
                Some(_) => n = n.max(8),
                None => {}
            }
            Walk::Children
        });
        if n == 0 {
            n = 8;
        }
    }
    n
}

pub(crate) fn run(
    stages: &[AnalyzedStage],
    profile: &DrawProfile,
    pack: &PackContext,
    opts: &TransformOptions,
) -> Result<TransformedProgram, Diagnostics> {
    let mut diags = Diagnostics::new();
    if stages.is_empty() {
        diags.push(Diagnostic::error("xf.no-stages", "the program has no stages"));
        return Err(diags);
    }
    let mut ordered: Vec<&AnalyzedStage> = stages.iter().collect();
    ordered.sort_by_key(|s| stage_order(s.stage));
    for w in ordered.windows(2) {
        if w[0].stage == w[1].stage {
            diags.push(Diagnostic::error("xf.stages", format!("the {} stage is given twice", w[0].stage)));
            return Err(diags);
        }
    }
    let is_compute = ordered.iter().any(|s| s.stage == ShaderStage::Compute);
    if is_compute && ordered.len() > 1 {
        diags.push(Diagnostic::error("xf.stages", "a compute stage must be translated alone"));
        return Err(diags);
    }
    let stage_list: Vec<ShaderStage> = ordered.iter().map(|s| s.stage).collect();
    let last_pre_raster = stage_list.iter().rev().copied().find(|s| s.is_pre_raster() && *s != ShaderStage::TessControl);

    let ctx = Ctx {
        profile,
        pack,
        opts,
        res_ctx: pack.resources.for_class(opts.program_class),
        stages: stage_list,
        last_pre_raster,
        texcoord_count: texcoord_count(&ordered).max(1),
        varying_globals: profile.globals.iter().filter(|g| g.varying).map(|g| (g.name.clone(), g.ty.clone())).collect(),
        profile_globals: profile.globals.iter().map(|g| (g.name.clone(), g.ty.clone())).collect(),
        profile_names: {
            let mut n = profile.declared_names();
            for g in &profile.globals {
                n.remove(&g.name);
            }
            n
        },
    };

    let mut works: Vec<StageWork> = ordered.iter().map(|s| StageWork::new(s)).collect();
    for w in &mut works {
        crate::rewrite::rewrite_stage(w, &ctx);
    }
    for w in &works {
        diags.extend(w.diags.iter().cloned());
    }
    if diags.has_errors() {
        return Err(diags);
    }
    for w in &mut works {
        w.diags = Diagnostics::new();
    }

    crate::link::link(&mut works, &ctx);
    for w in &mut works {
        crate::rewrite::finish_stage(w, &ctx);
    }
    for w in &mut works {
        // Taken, so that the emission phase below reports only its own diagnostics.
        diags.extend(std::mem::take(&mut w.diags).0);
    }
    if diags.has_errors() {
        return Err(diags);
    }

    let mut out = TransformedProgram {
        stages: Vec::new(),
        vertex_inputs: Vec::new(),
        fragment_outputs: Vec::new(),
        resources_used: Vec::new(),
        frame_members_used: Vec::new(),
        draw_members_used: Vec::new(),
        requires_raw_vulkan: false,
        diagnostics: Diagnostics::new(),
    };
    let mut resources: BTreeMap<String, Vec<ShaderStage>> = BTreeMap::new();
    let mut frame_used = BTreeSet::new();
    let mut draw_used = BTreeSet::new();
    for w in &mut works {
        let emitted = crate::emit::emit_stage(w, &ctx);
        diags.extend(w.diags.iter().cloned());
        out.requires_raw_vulkan |= w.requires_raw_vulkan;
        for r in &emitted.resources {
            resources.entry(r.clone()).or_default().push(w.stage);
        }
        frame_used.extend(emitted.frame_members);
        draw_used.extend(emitted.draw_members);
        if w.stage == ShaderStage::Vertex {
            out.vertex_inputs = emitted.vertex_inputs;
        }
        if w.stage == ShaderStage::Fragment {
            out.fragment_outputs = w.frag_outputs.iter().map(|(l, t, _)| (*l, t.clone())).collect();
            out.fragment_outputs.sort();
        }
        out.stages.push(emitted.stage);
    }
    if diags.has_errors() {
        return Err(diags);
    }
    if matches!(ctx.stages.as_slice(), [ShaderStage::Compute])
        || ctx.stages.iter().any(|s| matches!(s, ShaderStage::Geometry | ShaderStage::TessControl | ShaderStage::TessEval))
    {
        out.requires_raw_vulkan = true;
    }
    out.resources_used = resources.into_iter().collect();
    out.frame_members_used = frame_used.into_iter().collect();
    out.draw_members_used = draw_used.into_iter().collect();
    out.diagnostics = diags;
    Ok(out)
}

/// The resource kind of a canonicalized declaration (for `requires_raw_vulkan`).
pub(crate) fn needs_raw_vulkan(kind: &ResourceKind) -> bool {
    match kind {
        ResourceKind::Sampler { dim, .. } => matches!(dim.as_str(), "1d" | "1d_array" | "3d"),
        ResourceKind::StorageImage { .. } | ResourceKind::StorageBuffer => true,
        ResourceKind::UniformBuffer => false,
    }
}
