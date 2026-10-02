//! Building the pack-global uniform layout and binding table from analyzed stages.

use sb_core::Diagnostics;
use sb_core::model::{BindingTable, ResourceKind, UniformLayout};
use sb_uniforms::{BindingTableBuilder, LayoutBuilder, MemberIndex, ProgramClass, ResourceContext, UniformDecl};

use crate::analyze::AnalyzedStage;
use crate::profiles::DrawProfile;
use crate::transform::PackContext;

/// Collects the loose uniforms and resources of every program of a dimension (and of
/// the draw profiles used) and builds the pack layout and binding table.
///
/// ```
/// use sb_transform::{PackBuilder, profile};
/// use sb_uniforms::{ProgramClass, ResourceContext};
///
/// let mut b = PackBuilder::new(ResourceContext::new(ProgramClass::Gbuffers));
/// b.add_profile(profile("vanilla_terrain").unwrap(), ProgramClass::Gbuffers);
/// let data = b.finish();
/// assert!(data.bindings.get("gtexture").is_some());
/// assert!(data.layout.frame.member("gbufferProjection").is_some());
/// ```
#[derive(Debug, Clone)]
pub struct PackBuilder {
    layout: LayoutBuilder,
    bindings: BindingTableBuilder,
    resources: ResourceContext,
}

/// The pack-global layout and binding table (see [`PackBuilder`]).
#[derive(Debug, Clone)]
pub struct PackData {
    /// `sb_Frame` / `sb_Draw`.
    pub layout: UniformLayout,
    /// `(name, type)` -> member.
    pub members: MemberIndex,
    /// Resource bindings.
    pub bindings: BindingTable,
    /// Layout and binding diagnostics (type conflicts, ...).
    pub diagnostics: Diagnostics,
}

impl PackData {
    /// A [`PackContext`] over this data.
    pub fn context<'a>(&'a self, resources: &'a ResourceContext) -> PackContext<'a> {
        PackContext { layout: &self.layout, members: &self.members, bindings: &self.bindings, resources }
    }
}

impl PackBuilder {
    /// A builder canonicalizing resources with `resources` (its class is replaced by each
    /// program's class).
    pub fn new(resources: ResourceContext) -> Self {
        Self { layout: LayoutBuilder::new(), bindings: BindingTableBuilder::new(), resources }
    }

    /// Add an extra uniform declaration (custom uniforms, host additions).
    pub fn add_uniform(&mut self, decl: UniformDecl) {
        self.layout.add(decl);
    }

    /// Add the uniforms and resources of one analyzed stage of a program of `class`.
    pub fn add_stage(&mut self, stage: &AnalyzedStage, class: ProgramClass, program: Option<&str>) {
        for d in stage.info.uniform_decls(program) {
            self.layout.add(d);
        }
        let ctx = self.resources.for_class(class);
        for (o, kind) in stage.resources() {
            let c = sb_uniforms::canonicalize_with_kind(&o.name, &kind, &ctx);
            self.bindings.add_canonical(&c, kind);
        }
        for b in &stage.info.storage_blocks {
            if let Some(n) = b.binding {
                let c = sb_uniforms::canonicalize_ssbo(&b.name, n);
                self.bindings.add_canonical(&c, ResourceKind::StorageBuffer);
            }
        }
        for b in &stage.info.uniform_blocks {
            let c = sb_uniforms::canonicalize_ubo(&b.name);
            self.bindings.add_canonical(&c, ResourceKind::UniformBuffer);
        }
    }

    /// Add what a draw profile needs: the builtins its semantics and code reference and
    /// its host blocks and samplers.
    pub fn add_profile(&mut self, profile: &DrawProfile, class: ProgramClass) {
        for name in profile.referenced_builtins() {
            if let Some(b) = sb_uniforms::get(&name) {
                self.layout.add(UniformDecl::from_registry(name, b.ty));
            }
        }
        // Shadow-pass semantics of world-space profiles.
        if profile.world_space {
            for name in ["shadowModelView", "shadowProjection"] {
                if let Some(b) = sb_uniforms::get(name) {
                    self.layout.add(UniformDecl::from_registry(name, b.ty));
                }
            }
        }
        let ctx = self.resources.for_class(class);
        crate::compat::register_profile_resources(profile, &mut self.bindings, &ctx);
    }

    /// Build the layout and binding table.
    pub fn finish(self) -> PackData {
        let (layout, members, mut diagnostics) = self.layout.build();
        let (bindings, d) = self.bindings.finish();
        diagnostics.extend(d);
        PackData { layout, members, bindings, diagnostics }
    }
}
