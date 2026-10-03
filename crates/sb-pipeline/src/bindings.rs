//! Spec step 10: the pack layout of one folder — per-program resource contexts, the
//! pack-global `sb_Frame` / `sb_Draw` blocks and the binding table — plus the
//! storage-image format table and reflection helpers (cross-crate item 5).

use crate::analysis::FolderAnalysis;
use crate::resolve::Unit;
use indexmap::IndexMap;
use sb_core::model::{BindingTable, CustomImage, ProgramKind, ResourceKind, ResourceRef, UniformLayout};
use sb_core::program::PassGroup;
use sb_core::{Diagnostics, ScalarKind, TextureFormat};
use sb_pack::shaders_properties::{ShadersProperties, parse_buffer_name};
use sb_transform::DrawProfile;
use sb_uniforms::{BindingTableBuilder, CUSTOM_STAGE, LayoutBuilder, MemberIndex, ProgramClass, ResourceContext, UniformDecl};
use std::collections::BTreeSet;

/// Sampler-visibility class of a unit.
pub fn class_of(unit: &Unit) -> ProgramClass {
    match unit.kind {
        // Shadow-pass computes bind like shadow programs.
        ProgramKind::GeometryCompute { program, .. } => ProgramClass::from_geometry(program),
        ref k => ProgramClass::from_program_kind(k),
    }
}

/// Texture stage of a unit (`texture.<stage>.<name>`).
pub fn texture_stage_of(unit: &Unit) -> &'static str {
    match unit.kind {
        ProgramKind::Geometry { .. } | ProgramKind::GeometryCompute { .. } => PassGroup::GbuffersOpaque.texture_stage(),
        ProgramKind::Composite { group, .. } | ProgramKind::Compute { group, .. } => group.texture_stage(),
    }
}

/// The resource context of a program: its class, `watershadow`, the custom textures of its
/// texture stage (raw ones separately) and every `customTexture.*`, and the custom images.
/// For composite-style programs, `texture.<stage>.colortexN` overrides of buffers flipped
/// at least once earlier in the frame are left out (Iris `flippedAtLeastOnce`).
pub fn resource_context(
    class: ProgramClass,
    texture_stage: &str,
    props: &ShadersProperties,
    watershadow: bool,
    flipped_at_least_once: &BTreeSet<u32>,
) -> ResourceContext {
    let composite_style = matches!(class, ProgramClass::Fullscreen | ProgramClass::Compute);
    let mut c = ResourceContext::new(class).with_watershadow(watershadow);
    for d in props.texture_directives() {
        match d.stage {
            None => c = c.with_custom_texture(CUSTOM_STAGE, d.sampler.clone()),
            Some(st) if st.as_str() == texture_stage => {
                if let Some(rt) = d.raw_type {
                    c = c.with_raw_texture(texture_stage, d.sampler.clone(), rt.target_name());
                } else {
                    if composite_style
                        && let Some(i) = parse_buffer_name(&d.sampler)
                        && flipped_at_least_once.contains(&i)
                    {
                        continue;
                    }
                    c = c.with_custom_texture(texture_stage, d.sampler.clone());
                }
            }
            Some(_) => {}
        }
    }
    for img in &props.images {
        c = c.with_custom_image(img.name.clone(), img.sampler_name.as_deref());
    }
    c
}

/// Builds the folder's uniform layout and binding table.
pub struct LayoutBuild {
    layout: LayoutBuilder,
    bindings: BindingTableBuilder,
}

impl Default for LayoutBuild {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutBuild {
    pub fn new() -> Self {
        Self { layout: LayoutBuilder::new(), bindings: BindingTableBuilder::new() }
    }

    /// Add the loose uniforms of every analyzed stage of `unit`.
    pub fn add_uniforms(&mut self, unit: &Unit, analysis: &FolderAnalysis, u: usize) {
        for s in &analysis.units[u] {
            if let Ok(a) = &s.analyzed {
                for d in a.info.uniform_decls(Some(&unit.path)) {
                    self.layout.add(d);
                }
            }
        }
    }

    /// Add the resources of every analyzed stage of a unit, canonicalized with `ctx`.
    pub fn add_resources(&mut self, analysis: &FolderAnalysis, u: usize, ctx: &ResourceContext) {
        for s in &analysis.units[u] {
            let Ok(a) = &s.analyzed else { continue };
            for (o, kind) in a.resources() {
                let c = sb_uniforms::canonicalize_with_kind(&o.name, &kind, ctx);
                self.bindings.add_canonical(&c, kind);
            }
            for b in &a.info.storage_blocks {
                if let Some(n) = b.binding {
                    let c = sb_uniforms::canonicalize_ssbo(&b.name, n);
                    self.bindings.add_canonical(&c, ResourceKind::StorageBuffer);
                }
            }
            for b in &a.info.uniform_blocks {
                let c = sb_uniforms::canonicalize_ubo(&b.name);
                self.bindings.add_canonical(&c, ResourceKind::UniformBuffer);
            }
        }
    }

    /// Add what a draw profile needs: referenced builtins (and the shadow matrices of
    /// world-space profiles) and its host blocks and samplers.
    pub fn add_profile(&mut self, profile: &DrawProfile, ctx: &ResourceContext) {
        for name in profile.referenced_builtins() {
            if let Some(b) = sb_uniforms::get(&name) {
                self.layout.add(UniformDecl::from_registry(name, b.ty));
            }
        }
        if profile.world_space {
            for name in ["shadowModelView", "shadowProjection"] {
                if let Some(b) = sb_uniforms::get(name) {
                    self.layout.add(UniformDecl::from_registry(name, b.ty));
                }
            }
        }
        sb_transform::register_profile_resources(profile, &mut self.bindings, ctx);
    }

    /// Add an extra uniform declaration (custom uniforms and their inputs).
    pub fn add_uniform(&mut self, decl: UniformDecl) {
        self.layout.add(decl);
    }

    /// Lay out the blocks and build the table.
    pub fn finish(self) -> (UniformLayout, MemberIndex, BindingTable, Diagnostics) {
        let (layout, members, mut diags) = self.layout.build();
        let (bindings, d) = self.bindings.finish();
        diags.extend(d);
        (layout, members, bindings, diags)
    }
}

/// Format qualifiers for storage images declared without one: `colorimgN` takes the
/// colortex format, `shadowcolorimgN` the shadowcolor format, custom images their own.
pub fn image_formats(
    colortex: &dyn Fn(u32) -> TextureFormat,
    shadowcolor: &dyn Fn(u32) -> TextureFormat,
    images: &[CustomImage],
) -> IndexMap<String, String> {
    let mut m = IndexMap::new();
    for i in 0..sb_uniforms::MAX_COLOR_TEX {
        if let Some(f) = colortex(i).glsl_image_format() {
            m.insert(format!("colorimg{i}"), f.to_string());
        }
    }
    for i in 0..sb_uniforms::MAX_SHADOW_COLOR {
        if let Some(f) = shadowcolor(i).glsl_image_format() {
            m.insert(format!("shadowcolorimg{i}"), f.to_string());
        }
    }
    for img in images {
        if let Some(f) = img.format.glsl_image_format() {
            m.insert(img.name.clone(), f.to_string());
        }
    }
    m
}

/// The model's sample-type spelling (`float`, `int`, `uint`) of a reflected scalar kind
/// (cross-crate item 5: sb-compile reports [`ScalarKind`], the model uses strings).
pub fn sample_type_name(k: ScalarKind) -> &'static str {
    match k {
        ScalarKind::Int => "int",
        ScalarKind::Uint | ScalarKind::Bool => "uint",
        ScalarKind::Float | ScalarKind::Double => "float",
    }
}

/// The model [`ResourceKind`] of a reflected descriptor, or `None` for descriptor types
/// the model does not describe (separate samplers, input attachments, ...).
pub fn resource_kind_of(kind: &sb_compile::DescriptorKind) -> Option<ResourceKind> {
    use sb_compile::{Access, DescriptorKind as D};
    Some(match kind {
        D::CombinedImageSampler { shadow, sample_type, .. } => ResourceKind::Sampler {
            dim: kind.core_dim()?,
            shadow: *shadow,
            sample_type: sample_type_name(*sample_type).into(),
        },
        D::UniformTexelBuffer { sample_type } => {
            ResourceKind::Sampler { dim: "buffer".into(), shadow: false, sample_type: sample_type_name(*sample_type).into() }
        }
        D::StorageImage { format, access, sample_type, .. } => ResourceKind::StorageImage {
            dim: kind.core_dim()?,
            format: format.clone(),
            sample_type: sample_type_name(*sample_type).into(),
            readonly: *access == Access::ReadOnly,
            writeonly: *access == Access::WriteOnly,
        },
        D::StorageTexelBuffer { format, access, sample_type } => ResourceKind::StorageImage {
            dim: "buffer".into(),
            format: format.clone(),
            sample_type: sample_type_name(*sample_type).into(),
            readonly: *access == Access::ReadOnly,
            writeonly: *access == Access::WriteOnly,
        },
        D::StorageBuffer { .. } => ResourceKind::StorageBuffer,
        D::UniformBuffer { .. } => ResourceKind::UniformBuffer,
        _ => return None,
    })
}

/// Fill in what the binding table leaves open from what the compiled programs declare:
/// storage images declared without a format qualifier get the format they were compiled
/// with (and the reflected sample type). Returns the number of entries changed.
pub fn refine_storage_images(table: &mut BindingTable, reflections: &[&sb_compile::Reflection]) -> usize {
    let mut changed = 0;
    for e in &mut table.entries {
        let ResourceKind::StorageImage { format, sample_type, .. } = &mut e.kind else { continue };
        let reflected = reflections
            .iter()
            .flat_map(|r| r.descriptors.iter())
            .find(|d| d.set == e.set && d.binding == e.binding && matches!(d.kind, sb_compile::DescriptorKind::StorageImage { .. }));
        let Some(ResourceKind::StorageImage { format: f, sample_type: st, .. }) = reflected.and_then(|d| resource_kind_of(&d.kind))
        else {
            continue;
        };
        if format.is_none() && f.is_some() {
            *format = f;
            changed += 1;
        }
        if *sample_type != st {
            *sample_type = st;
            changed += 1;
        }
    }
    changed
}

/// Whether a resource reads a shadow map (decides the default of `shadow.enabled`).
pub fn is_shadow_resource(r: &ResourceRef) -> bool {
    matches!(r, ResourceRef::ShadowTex(_) | ResourceRef::ShadowTexHw(_) | ResourceRef::ShadowColor(_) | ResourceRef::ShadowColorImage(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_compile::{CompileOptions, compile_glsl, reflect};
    use sb_core::ShaderStage;
    use sb_core::model::BindingEntry;

    #[test]
    fn sample_types_map_to_model_strings() {
        assert_eq!(sample_type_name(ScalarKind::Float), "float");
        assert_eq!(sample_type_name(ScalarKind::Int), "int");
        assert_eq!(sample_type_name(ScalarKind::Uint), "uint");
    }

    /// Cross-crate item 5: reflected storage images map to `ResourceKind::StorageImage`
    /// with string sample types, and refine table entries declared without a format.
    #[test]
    fn storage_image_reflection_maps_to_model() {
        let src = "#version 460\nlayout(set = 2, binding = 16, r32ui) uniform uimage2D colorimg5;\nlayout(set = 2, binding = 17, rgba16f) uniform writeonly image3D voxels;\nlayout(local_size_x = 1) in;\nvoid main() { imageAtomicAdd(colorimg5, ivec2(0), 1u); imageStore(voxels, ivec3(0), vec4(1.0)); }\n";
        let spirv = compile_glsl(src, ShaderStage::Compute, "t.csh", &CompileOptions::default(), None).unwrap();
        let refl = reflect(&spirv).unwrap();
        let kinds: Vec<ResourceKind> = refl.descriptors.iter().filter_map(|d| resource_kind_of(&d.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                ResourceKind::StorageImage { dim: "2d".into(), format: Some("r32ui".into()), sample_type: "uint".into(), readonly: false, writeonly: false },
                ResourceKind::StorageImage { dim: "3d".into(), format: Some("rgba16f".into()), sample_type: "float".into(), readonly: false, writeonly: true },
            ]
        );
        let mut table = BindingTable {
            entries: vec![BindingEntry {
                name: "colorimg5".into(),
                set: 2,
                binding: 16,
                kind: ResourceKind::StorageImage { dim: "2d".into(), format: None, sample_type: "float".into(), readonly: false, writeonly: false },
                resource: ResourceRef::ColorImage(5),
            }],
        };
        assert_eq!(refine_storage_images(&mut table, &[&refl]), 2);
        assert_eq!(
            table.entries[0].kind,
            ResourceKind::StorageImage { dim: "2d".into(), format: Some("r32ui".into()), sample_type: "uint".into(), readonly: false, writeonly: false }
        );
    }

    #[test]
    fn contexts_follow_texture_stages() {
        let text = "texture.composite.colortex4=tex/a.png\ntexture.composite.noisetex=tex/n.png\ntexture.gbuffers.gaux1=tex/g.png\ncustomTexture.myTex=tex/c.png\ntexture.deferred.lut=tex/lut.dat TEXTURE_3D RGBA8 16 16 16 RGBA UNSIGNED_BYTE\n";
        let e = sb_pack::properties::parse(text);
        let (props, d) = sb_pack::shaders_properties::parse(&e, &e);
        assert!(!d.has_errors(), "{d:?}");
        let c = resource_context(ProgramClass::Fullscreen, "composite", &props, false, &BTreeSet::new());
        assert_eq!(c.custom_textures.get("colortex4").map(String::as_str), Some("composite"));
        assert_eq!(c.custom_textures.get("myTex").map(String::as_str), Some("custom"));
        assert!(!c.custom_textures.contains_key("gaux1"));
        // colortex4 was flipped before this composite pass: the override no longer applies.
        let c = resource_context(ProgramClass::Fullscreen, "composite", &props, false, &[4].into());
        assert!(!c.custom_textures.contains_key("colortex4"));
        assert!(c.custom_textures.contains_key("noisetex"));
        // Raw textures are keyed by dimension.
        let c = resource_context(ProgramClass::Fullscreen, "deferred", &props, true, &BTreeSet::new());
        assert_eq!(c.raw_textures.get(&("lut".to_string(), "3d".to_string())).map(String::as_str), Some("deferred"));
        assert!(c.watershadow_declared);
        // Geometry programs never drop overrides.
        let c = resource_context(ProgramClass::Gbuffers, "gbuffers", &props, false, &[4].into());
        assert!(c.custom_textures.contains_key("gaux1"));
    }

    #[test]
    fn image_format_table() {
        let images = vec![CustomImage {
            name: "voxelImg".into(),
            sampler_name: Some("voxelSampler".into()),
            format: TextureFormat::R32UI,
            pixel_format: "RED_INTEGER".into(),
            pixel_type: "UNSIGNED_INT".into(),
            clear: true,
            size: sb_core::model::ImageSize::Absolute3D { width: 4, height: 4, depth: 4 },
        }];
        let m = image_formats(&|i| if i == 2 { TextureFormat::RGBA16F } else { TextureFormat::RGBA }, &|_| TextureFormat::RGB8, &images);
        assert_eq!(m.get("colorimg2").map(String::as_str), Some("rgba16f"));
        assert_eq!(m.get("colorimg0").map(String::as_str), Some("rgba8"));
        assert_eq!(m.get("shadowcolorimg1").map(String::as_str), Some("rgba8"));
        assert_eq!(m.get("voxelImg").map(String::as_str), Some("r32ui"));
    }
}
