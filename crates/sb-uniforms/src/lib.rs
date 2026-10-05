//! ShaderBridge uniforms and resources: everything a translated pack binds.
//!
//! * [`registry`]: every builtin loose uniform the host provides (OptiFine, Iris,
//!   Distant Horizons, core-profile names and ShaderBridge's `gl_Fog` replacements),
//!   with its Iris type and its update [`Frequency`] (`sb_Frame` vs `sb_Draw`).
//! * [`layout`]: the pack-global std140 blocks `sb_Frame` (set 0, binding 0) and
//!   `sb_Draw` (set 0, binding 1) built from the loose uniforms of all programs, with
//!   per-type conflict members and a [`MemberIndex`] for the transformer
//!   (ARCHITECTURE §5.1).
//! * [`resources`]: canonicalization of sampler/image names (aliases, Iris visibility
//!   rules, custom textures and images) into a canonical binding name and a
//!   `ResourceRef` (ARCHITECTURE §5.2). Translators use [`canonicalize_with_kind`],
//!   because raw `texture.<stage>.<name>` textures apply only to declarations of a
//!   matching sampler type.
//! * [`bindings`]: the pack-global [`BindingTable`](sb_core::model::BindingTable):
//!   samplers in set 1, SSBOs and storage images in set 2.
//! * [`naming`]: the names derived from a pack name when it needs several members or
//!   bindings (`sb_as_<suffix>_<name>` conflict variants).
//!
//! Typical use by the pipeline, after analyzing every stage of a dimension:
//!
//! ```
//! use sb_core::GlslType;
//! use sb_uniforms::{
//!     BindingTableBuilder, LayoutBuilder, ProgramClass, ResourceContext, UniformDecl,
//!     canonicalize_with_kind, sampler_kind,
//! };
//!
//! // Loose uniforms of all programs.
//! let mut layout = LayoutBuilder::new();
//! layout.add(UniformDecl::from_registry("frameTimeCounter", GlslType::FLOAT).in_program("composite"));
//! layout.add(UniformDecl::from_registry("entityId", GlslType::INT).in_program("gbuffers_entities"));
//! layout.add(UniformDecl::custom("screenDark", GlslType::FLOAT));
//! let (uniforms, members, _diagnostics) = layout.build();
//! assert_eq!(members.get("entityId", GlslType::INT).unwrap().block, sb_uniforms::Frequency::Draw);
//! assert_eq!(uniforms.frame.members.len(), 2);
//!
//! // Opaque uniforms of all programs.
//! let mut bindings = BindingTableBuilder::new();
//! let gbuffers = ResourceContext::new(ProgramClass::Gbuffers);
//! let kind = sampler_kind("sampler2D").unwrap();
//! let c = canonicalize_with_kind("gcolor", &kind, &gbuffers); // gbuffers cannot read colortex0-3
//! assert_eq!(c.name, "gtexture");
//! let name = bindings.add_canonical(&c, kind);
//! let table = bindings.build();
//! assert_eq!(table.get(&name).unwrap().set, 1);
//! ```

pub mod bindings;
pub mod layout;
pub mod naming;
pub mod registry;
pub mod resources;

pub use bindings::{
    BindingTableBuilder, IMAGE_BINDING_BASE, MAX_SSBO_INDEX, PACK_UBO_BINDING_BASE, SAMPLER_SET,
    STORAGE_SET, find_binding, kinds_compatible,
};
pub use naming::{CONFLICT_PREFIX, conflict_name, is_derived_name, uniquified_name};
pub use layout::{
    DRAW_BINDING, DRAW_BLOCK_NAME, FRAME_BINDING, FRAME_BLOCK_NAME, LayoutBuilder, MAX_MEMBER_SIZE,
    MemberIndex, MemberRef, PORTABLE_MAX_BLOCK_SIZE, UNIFORM_SET, UniformDecl,
    build_uniform_layout, component_count, type_suffix, validate_block,
};
pub use registry::{
    BuiltinUniform, Frequency, all, custom_uniform_input_type, frequency_of, get, is_builtin,
    is_type_compatible, source_of, sources,
};
pub use resources::{
    ALBEDO_NAMES, CUSTOM_STAGE, Canonical, LEGACY_COLOR_NAMES, MAX_COLOR_TEX, MAX_DEPTH_TEX,
    MAX_SHADOW_COLOR, ProgramClass, ResourceContext, canonicalize, canonicalize_ssbo,
    canonicalize_ubo, canonicalize_with_kind, custom_texture_binding_name, custom_texture_id,
    image_kind, is_builtin_resource_name, is_opaque_type, parse_custom_texture_id,
    raw_texture_binding_name, raw_texture_dim, raw_texture_id, sampler_kind,
    split_custom_texture_id,
};
