//! The pack-global resource binding table (ARCHITECTURE §5.2).
//!
//! [`BindingTableBuilder`] collects every opaque uniform (after [`canonicalize`]) and
//! every pack storage/uniform block seen in any program, then assigns descriptors:
//!
//! * set [`SAMPLER_SET`] (1): combined image samplers, bindings `0..`, well-known names
//!   first in a fixed canonical order (`gtexture`, `lightmap`, `normals`, `specular`,
//!   `iris_overlay`, `colortex0..31`, `depthtex0..2`, `shadowtex0/1`, `shadowtex0HW/1HW`,
//!   `shadowcolor0..7`, `noisetex`, `dhDepthTex0/1`, `dhBlockAtlas`), then every other
//!   name sorted;
//! * set [`STORAGE_SET`] (2): shader storage blocks at binding `N` of their
//!   `bufferObject.N` ([`ResourceRef::Ssbo`]), then storage images from binding
//!   [`IMAGE_BINDING_BASE`] (16, or after the highest SSBO binding if that is higher):
//!   `colorimg0..31`, `shadowcolorimg0..7`, then the others sorted;
//! * set [`crate::layout::UNIFORM_SET`] (0): uniform blocks the pack declares itself,
//!   from binding [`PACK_UBO_BINDING_BASE`] (2), after `sb_Frame` and `sb_Draw`.
//!
//! The assignment depends only on the set of names, kinds and resources, not on the
//! order of [`BindingTableBuilder::add`] calls, except for which declaration of a
//! conflicting name keeps the plain name.
//!
//! # Conflicts
//!
//! The same canonical name declared with incompatible kinds (e.g. `sampler2D` and
//! `sampler2DShadow`, or `sampler2D` and `usampler2D`), or resolving to different
//! resources, gets separate entries: the first keeps the name, the others are named
//! [`conflict_name`]`(name, suffix)` = `sb_as_<suffix>_<name>` (`sb_as_shadow_shadowtex1`,
//! `sb_as_uint_colortex2`, `sb_as_3d_colortex0`; see [`crate::naming`]), with a
//! `binding.kind-conflict` / `binding.resource-conflict` warning. Use the name returned
//! by [`BindingTableBuilder::add`], or [`find_binding`], to find the entry for a
//! declaration. Storage-image declarations of the same name are merged: memory
//! qualifiers are kept only if every declaration has them, and the first format wins.
//!
//! Several SSBO block names may share one binding (`bufferObject.N` declared with
//! different block names in different programs): they are the same buffer.
//!
//! [`canonicalize`]: crate::resources::canonicalize

use crate::layout::UNIFORM_SET;
use crate::naming::{conflict_name, is_derived_name, uniquified_name};
use crate::resources::{Canonical, MAX_COLOR_TEX, MAX_DEPTH_TEX, MAX_SHADOW_COLOR, indexed};
use indexmap::IndexMap;
use sb_core::model::{BindingEntry, BindingTable, ResourceKind, ResourceRef};
use sb_core::{Diagnostic, Diagnostics};
use std::collections::{BTreeSet, HashSet};

/// Descriptor set of combined image samplers.
pub const SAMPLER_SET: u32 = 1;
/// Descriptor set of storage images and shader storage buffers.
pub const STORAGE_SET: u32 = 2;
/// First binding of storage images in [`STORAGE_SET`] (SSBO indices are below it).
pub const IMAGE_BINDING_BASE: u32 = 16;
/// First binding of pack-declared uniform blocks in [`UNIFORM_SET`].
pub const PACK_UBO_BINDING_BASE: u32 = 2;
/// Largest accepted `bufferObject.N` index. Real packs use single digits; drivers
/// expose at most a few dozen SSBO bindings (Iris rejects indices above
/// `GL_MAX_SHADER_STORAGE_BUFFER_BINDINGS`). Larger indices from malformed input would
/// make hosts allocate huge descriptor set layouts, so they are rejected.
pub const MAX_SSBO_INDEX: u32 = 4095;

/// The part of a [`ResourceKind`] that decides whether two declarations can share one
/// descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum KindKey<'a> {
    Sampler {
        dim: &'a str,
        shadow: bool,
        sample_type: &'a str,
    },
    Image {
        dim: &'a str,
        sample_type: &'a str,
    },
    Buffer,
    Uniform,
}

fn kind_key(k: &ResourceKind) -> KindKey<'_> {
    match k {
        ResourceKind::Sampler {
            dim,
            shadow,
            sample_type,
        } => KindKey::Sampler {
            dim,
            shadow: *shadow,
            sample_type,
        },
        ResourceKind::StorageImage {
            dim, sample_type, ..
        } => KindKey::Image { dim, sample_type },
        ResourceKind::StorageBuffer => KindKey::Buffer,
        ResourceKind::UniformBuffer => KindKey::Uniform,
    }
}

/// Whether two declarations can share one binding.
pub fn kinds_compatible(a: &ResourceKind, b: &ResourceKind) -> bool {
    kind_key(a) == kind_key(b)
}

#[derive(Debug, Clone)]
struct Slot {
    binding_name: String,
    kind: ResourceKind,
    resource: ResourceRef,
}

/// Collects opaque uniforms of a whole pack and assigns descriptor sets and bindings
/// (see the [module docs](self)).
#[derive(Debug, Clone, Default)]
pub struct BindingTableBuilder {
    /// canonical name → declarations with distinct (kind, resource); `[0]` keeps the name.
    groups: IndexMap<String, Vec<Slot>>,
    used_names: HashSet<String>,
    diagnostics: Diagnostics,
}

impl BindingTableBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a resource under its canonical `name` and return the binding name the
    /// declaration must use (`name`, or [`conflict_name`]`(name, suffix)` =
    /// `sb_as_<suffix>_<name>` on a conflict). Invalid names
    /// and SSBO indices above [`MAX_SSBO_INDEX`] are rejected with an error diagnostic
    /// (the name is returned unchanged and gets no binding).
    pub fn add(&mut self, name: &str, kind: ResourceKind, resource: ResourceRef) -> String {
        if !is_identifier(name) || name.starts_with("gl_") {
            self.diagnostics.push(Diagnostic::error(
                "binding.invalid-name",
                format!("`{name}` is not a valid resource name; it gets no binding"),
            ));
            return name.to_string();
        }
        if let ResourceRef::Ssbo(index) = resource
            && index > MAX_SSBO_INDEX
        {
            self.diagnostics.push(Diagnostic::error(
                "binding.ssbo-index",
                format!(
                    "storage block `{name}` uses bufferObject.{index}; indices above {MAX_SSBO_INDEX} are not supported, so it gets no binding"
                ),
            ));
            return name.to_string();
        }
        let Self {
            groups,
            used_names,
            diagnostics,
        } = self;
        let group = groups.entry(name.to_string()).or_default();
        let key = kind_key(&kind);
        if let Some(slot) = group
            .iter_mut()
            .find(|s| kind_key(&s.kind) == key && s.resource == resource)
        {
            merge_kind(&mut slot.kind, kind, &slot.binding_name, diagnostics);
            return slot.binding_name.clone();
        }

        let base = match group.first() {
            None => name.to_string(),
            Some(primary) => {
                let variant = conflict_name(name, &variant_suffix(primary, &kind, &resource));
                let kind_conflict = kind_key(&primary.kind) != key;
                diagnostics.push(Diagnostic::warning(
                    if kind_conflict { "binding.kind-conflict" } else { "binding.resource-conflict" },
                    if kind_conflict {
                        format!(
                            "`{name}` is declared as {} and as {}; the second gets its own binding `{variant}`",
                            describe(&primary.kind),
                            describe(&kind)
                        )
                    } else {
                        format!(
                            "`{name}` refers to {:?} and to {:?}; the second gets its own binding `{variant}`",
                            primary.resource, resource
                        )
                    },
                ));
                variant
            }
        };
        let mut binding_name = base.clone();
        let mut n = 2;
        while used_names.contains(&binding_name) {
            binding_name = uniquified_name(&base, n);
            n += 1;
        }
        used_names.insert(binding_name.clone());
        group.push(Slot {
            binding_name: binding_name.clone(),
            kind,
            resource,
        });
        binding_name
    }

    /// [`BindingTableBuilder::add`] for a [`Canonical`] resource.
    pub fn add_canonical(&mut self, canonical: &Canonical, kind: ResourceKind) -> String {
        self.add(&canonical.name, kind, canonical.resource.clone())
    }

    /// Diagnostics collected so far (conflicts, invalid names).
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Number of distinct bindings registered.
    pub fn len(&self) -> usize {
        self.groups.values().map(Vec::len).sum()
    }

    /// Whether nothing was registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Assign sets and bindings. Entries are sorted by (set, binding, name).
    pub fn build(&self) -> BindingTable {
        let mut samplers = Vec::new();
        let mut images = Vec::new();
        let mut buffers = Vec::new();
        let mut ubos = Vec::new();
        for (canonical, slots) in &self.groups {
            for (vi, slot) in slots.iter().enumerate() {
                let item = (rank(canonical), canonical.as_str(), vi, slot);
                match slot.kind {
                    ResourceKind::Sampler { .. } => samplers.push(item),
                    ResourceKind::StorageImage { .. } => images.push(item),
                    ResourceKind::StorageBuffer => buffers.push(item),
                    ResourceKind::UniformBuffer => ubos.push(item),
                }
            }
        }
        for list in [&mut samplers, &mut images, &mut buffers, &mut ubos] {
            list.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
        }

        let mut entries = Vec::with_capacity(self.len());
        let mut push = |set: u32, binding: u32, slot: &Slot| {
            entries.push(BindingEntry {
                name: slot.binding_name.clone(),
                set,
                binding,
                kind: slot.kind.clone(),
                resource: slot.resource.clone(),
            });
        };

        for (binding, item) in (0u32..).zip(&samplers) {
            push(SAMPLER_SET, binding, item.3);
        }

        // SSBOs: `bufferObject.N` → binding N; blocks without an index take free slots.
        let fixed: BTreeSet<u32> = buffers
            .iter()
            .filter_map(|i| match i.3.resource {
                ResourceRef::Ssbo(n) => Some(n),
                _ => None,
            })
            .collect();
        let mut next_free = 0u32;
        let mut highest: Option<u32> = fixed.last().copied();
        for item in &buffers {
            let binding = match item.3.resource {
                ResourceRef::Ssbo(n) => n,
                _ => {
                    while fixed.contains(&next_free) {
                        next_free += 1;
                    }
                    next_free += 1;
                    next_free - 1
                }
            };
            highest = highest.max(Some(binding));
            push(STORAGE_SET, binding, item.3);
        }

        // `add` bounds SSBO indices by MAX_SSBO_INDEX, so these cannot overflow.
        let image_base = highest.map_or(IMAGE_BINDING_BASE, |h| IMAGE_BINDING_BASE.max(h + 1));
        for (offset, item) in (0u32..).zip(&images) {
            push(STORAGE_SET, image_base + offset, item.3);
        }

        for (offset, item) in (0u32..).zip(&ubos) {
            push(UNIFORM_SET, PACK_UBO_BINDING_BASE + offset, item.3);
        }

        entries.sort_by(|a, b| (a.set, a.binding, &a.name).cmp(&(b.set, b.binding, &b.name)));
        BindingTable { entries }
    }

    /// Build the table and return it with the collected diagnostics.
    pub fn finish(self) -> (BindingTable, Diagnostics) {
        let table = self.build();
        (table, self.diagnostics)
    }
}

/// Find the entry for a declaration of `canonical` with `kind`: the entry with the
/// canonical name, or one of the names [`BindingTableBuilder::add`] derives from it
/// (`sb_as_<suffix>_<name>` conflict variants and `_<n>` uniquified names, see
/// [`is_derived_name`]), that has the same resource and a compatible kind.
pub fn find_binding<'a>(
    table: &'a BindingTable,
    canonical: &Canonical,
    kind: &ResourceKind,
) -> Option<&'a BindingEntry> {
    let key = kind_key(kind);
    table
        .entries
        .iter()
        .filter(|e| {
            e.resource == canonical.resource
                && kind_key(&e.kind) == key
                && is_derived_name(&e.name, &canonical.name)
        })
        .min_by_key(|e| (e.name != canonical.name, e.name.len(), e.name.as_str()))
}

/// Position of a well-known resource name in the canonical binding order.
fn rank(name: &str) -> u32 {
    match name {
        "gtexture" => return 0,
        "lightmap" => return 1,
        "normals" => return 2,
        "specular" => return 3,
        "iris_overlay" => return 4,
        "shadowtex0" => return 100,
        "shadowtex1" => return 101,
        "shadowtex0HW" => return 102,
        "shadowtex1HW" => return 103,
        "noisetex" => return 120,
        "dhDepthTex0" => return 130,
        "dhDepthTex1" => return 131,
        "dhBlockAtlas" => return 132,
        _ => {}
    }
    if let Some(i) = indexed(name, "colortex", MAX_COLOR_TEX) {
        return 10 + i;
    }
    if let Some(i) = indexed(name, "depthtex", MAX_DEPTH_TEX) {
        return 50 + i;
    }
    if let Some(i) = indexed(name, "shadowcolorimg", MAX_SHADOW_COLOR) {
        return 200 + MAX_COLOR_TEX + i;
    }
    if let Some(i) = indexed(name, "shadowcolor", MAX_SHADOW_COLOR) {
        return 110 + i;
    }
    if let Some(i) = indexed(name, "colorimg", MAX_COLOR_TEX) {
        return 200 + i;
    }
    u32::MAX
}

fn merge_kind(
    existing: &mut ResourceKind,
    new: ResourceKind,
    name: &str,
    diagnostics: &mut Diagnostics,
) {
    if let (
        ResourceKind::StorageImage {
            format: fa,
            readonly: ra,
            writeonly: wa,
            ..
        },
        ResourceKind::StorageImage {
            format: fb,
            readonly: rb,
            writeonly: wb,
            ..
        },
    ) = (existing, new)
    {
        *ra &= rb;
        *wa &= wb;
        match (fa.as_ref(), fb) {
            (None, Some(b)) => *fa = Some(b),
            (Some(a), Some(b)) if !a.eq_ignore_ascii_case(&b) => diagnostics.push(Diagnostic::warning(
                "binding.format-conflict",
                format!("image `{name}` is declared with formats `{a}` and `{b}`; the binding uses `{a}`"),
            )),
            _ => {}
        }
    }
}

fn describe(k: &ResourceKind) -> String {
    match k {
        ResourceKind::Sampler {
            dim,
            shadow,
            sample_type,
        } => {
            format!(
                "a {sample_type} {dim} sampler{}",
                if *shadow { " (shadow)" } else { "" }
            )
        }
        ResourceKind::StorageImage {
            dim, sample_type, ..
        } => format!("a {sample_type} {dim} image"),
        ResourceKind::StorageBuffer => "a storage buffer".into(),
        ResourceKind::UniformBuffer => "a uniform buffer".into(),
    }
}

/// Suffix for a conflicting declaration: the parts of its kind that differ from the
/// primary declaration, or a resource slug when only the resource differs.
fn variant_suffix(primary: &Slot, kind: &ResourceKind, resource: &ResourceRef) -> String {
    let dims = |a: &str, b: &str, sa: &str, sb: &str, parts: &mut Vec<String>| {
        if a != b {
            parts.push(b.to_string());
        }
        if sa != sb {
            parts.push(sb.to_string());
        }
    };
    let mut parts: Vec<String> = Vec::new();
    match (&primary.kind, kind) {
        (
            ResourceKind::Sampler {
                dim: da,
                shadow: sha,
                sample_type: sa,
            },
            ResourceKind::Sampler {
                dim: db,
                shadow: shb,
                sample_type: sb,
            },
        ) => {
            dims(da, db, sa, sb, &mut parts);
            if sha != shb {
                parts.push(if *shb {
                    "shadow".into()
                } else {
                    "noshadow".into()
                });
            }
        }
        (
            ResourceKind::StorageImage {
                dim: da,
                sample_type: sa,
                ..
            },
            ResourceKind::StorageImage {
                dim: db,
                sample_type: sb,
                ..
            },
        ) => {
            dims(da, db, sa, sb, &mut parts);
        }
        (_, ResourceKind::Sampler { .. }) => parts.push("sampler".into()),
        (_, ResourceKind::StorageImage { .. }) => parts.push("image".into()),
        (_, ResourceKind::StorageBuffer) => parts.push("ssbo".into()),
        (_, ResourceKind::UniformBuffer) => parts.push("ubo".into()),
    }
    if parts.is_empty() {
        parts.push(resource_slug(resource));
    }
    parts.join("_")
}

fn resource_slug(r: &ResourceRef) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect()
    };
    match r {
        ResourceRef::ColorTex(i) => format!("colortex{i}"),
        ResourceRef::DepthTex(i) => format!("depthtex{i}"),
        ResourceRef::ShadowTex(i) => format!("shadowtex{i}"),
        ResourceRef::ShadowTexHw(i) => format!("shadowtex{i}hw"),
        ResourceRef::ShadowColor(i) => format!("shadowcolor{i}"),
        ResourceRef::Noise => "noise".into(),
        ResourceRef::Atlas => "atlas".into(),
        ResourceRef::Lightmap => "lightmap".into(),
        ResourceRef::Normals => "normals".into(),
        ResourceRef::Specular => "specular".into(),
        ResourceRef::Overlay => "overlay".into(),
        ResourceRef::DhDepthTex(i) => format!("dhdepthtex{i}"),
        ResourceRef::DhBlockAtlas => "dhblockatlas".into(),
        ResourceRef::White => "white".into(),
        ResourceRef::CustomTexture(id) => format!("tex_{}", clean(id)),
        ResourceRef::Image(n) => format!("img_{}", clean(n)),
        ResourceRef::ColorImage(i) => format!("colorimg{i}"),
        ResourceRef::ShadowColorImage(i) => format!("shadowcolorimg{i}"),
        ResourceRef::Ssbo(i) => format!("ssbo{i}"),
        ResourceRef::UniformBlock(n) => format!("ubo_{}", clean(n)),
        ResourceRef::Unknown(_) => "unknown".into(),
    }
}

fn is_identifier(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphabetic() || f == '_')
        && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::{ProgramClass, ResourceContext, canonicalize, image_kind, sampler_kind};
    use ResourceRef as R;
    use pretty_assertions::assert_eq;

    fn s2d() -> ResourceKind {
        sampler_kind("sampler2D").unwrap()
    }

    fn layout(t: &BindingTable) -> Vec<(String, u32, u32)> {
        t.entries
            .iter()
            .map(|e| (e.name.clone(), e.set, e.binding))
            .collect()
    }

    fn sample() -> Vec<(&'static str, ResourceKind, ResourceRef)> {
        vec![
            ("myLut", s2d(), R::Unknown("myLut".into())),
            ("colortex5", s2d(), R::ColorTex(5)),
            ("noisetex", s2d(), R::Noise),
            ("depthtex1", s2d(), R::DepthTex(1)),
            ("colortex0", s2d(), R::ColorTex(0)),
            ("gtexture", s2d(), R::Atlas),
            ("shadowcolor0", s2d(), R::ShadowColor(0)),
            ("shadowtex1", s2d(), R::ShadowTex(1)),
            ("colortex12", s2d(), R::ColorTex(12)),
            ("aLut", s2d(), R::Unknown("aLut".into())),
            (
                "colorimg3",
                image_kind("image2D", Some("rgba16f"), false, true).unwrap(),
                R::ColorImage(3),
            ),
            (
                "voxel_img",
                image_kind("uimage3D", Some("r16ui"), false, false).unwrap(),
                R::Image("voxel_img".into()),
            ),
            (
                "colorimg1",
                image_kind("image2D", None, false, false).unwrap(),
                R::ColorImage(1),
            ),
            ("LightBuffer", ResourceKind::StorageBuffer, R::Ssbo(2)),
            ("VoxelBuffer", ResourceKind::StorageBuffer, R::Ssbo(0)),
            (
                "Params",
                ResourceKind::UniformBuffer,
                R::UniformBlock("Params".into()),
            ),
        ]
    }

    #[test]
    fn canonical_order_and_sets() {
        let mut b = BindingTableBuilder::new();
        for (n, k, r) in sample() {
            assert_eq!(b.add(n, k, r), n);
        }
        assert!(b.diagnostics().is_empty());
        let t = b.build();
        let expected: Vec<(String, u32, u32)> = [
            ("Params", 0, 2),
            ("gtexture", 1, 0),
            ("colortex0", 1, 1),
            ("colortex5", 1, 2),
            ("colortex12", 1, 3),
            ("depthtex1", 1, 4),
            ("shadowtex1", 1, 5),
            ("shadowcolor0", 1, 6),
            ("noisetex", 1, 7),
            ("aLut", 1, 8),
            ("myLut", 1, 9),
            ("VoxelBuffer", 2, 0),
            ("LightBuffer", 2, 2),
            ("colorimg1", 2, 16),
            ("colorimg3", 2, 17),
            ("voxel_img", 2, 18),
        ]
        .into_iter()
        .map(|(n, s, b)| (n.to_string(), s, b))
        .collect();
        assert_eq!(layout(&t), expected);
        assert_eq!(
            t.get("voxel_img").unwrap().resource,
            R::Image("voxel_img".into())
        );
    }

    #[test]
    fn deterministic_regardless_of_order() {
        let mut fwd = BindingTableBuilder::new();
        let mut rev = BindingTableBuilder::new();
        let items = sample();
        for (n, k, r) in items.clone() {
            fwd.add(n, k, r);
        }
        for (n, k, r) in items.into_iter().rev() {
            rev.add(n, k, r);
        }
        // Repeated declarations do not change anything.
        for (n, k, r) in sample() {
            rev.add(n, k, r);
        }
        assert_eq!(fwd.build(), rev.build());
        assert_eq!(fwd.len(), 16);
    }

    #[test]
    fn shadow_sampler_conflict() {
        let mut b = BindingTableBuilder::new();
        let shadow = sampler_kind("sampler2DShadow").unwrap();
        assert_eq!(b.add("shadowtex1", s2d(), R::ShadowTex(1)), "shadowtex1");
        assert_eq!(
            b.add("shadowtex1", shadow.clone(), R::ShadowTex(1)),
            "sb_as_shadow_shadowtex1"
        );
        assert_eq!(
            b.add("shadowtex1", shadow.clone(), R::ShadowTex(1)),
            "sb_as_shadow_shadowtex1"
        );
        assert_eq!(b.add("shadowtex1", s2d(), R::ShadowTex(1)), "shadowtex1");
        assert_eq!(b.diagnostics().len(), 1);
        assert_eq!(b.diagnostics().0[0].code, "binding.kind-conflict");
        let (t, d) = b.finish();
        assert_eq!(d.len(), 1);
        assert_eq!(
            layout(&t),
            vec![
                ("shadowtex1".into(), 1, 0),
                ("sb_as_shadow_shadowtex1".into(), 1, 1)
            ]
        );
        let c = Canonical::new("shadowtex1", R::ShadowTex(1));
        assert_eq!(
            find_binding(&t, &c, &shadow).unwrap().name,
            "sb_as_shadow_shadowtex1"
        );
        assert_eq!(find_binding(&t, &c, &s2d()).unwrap().name, "shadowtex1");
        assert!(find_binding(&t, &c, &sampler_kind("usampler2D").unwrap()).is_none());
        assert!(find_binding(&t, &Canonical::new("shadowtex1", R::ShadowTex(0)), &s2d()).is_none());
    }

    #[test]
    fn conflict_suffixes() {
        let mut b = BindingTableBuilder::new();
        assert_eq!(b.add("colortex2", s2d(), R::ColorTex(2)), "colortex2");
        assert_eq!(
            b.add(
                "colortex2",
                sampler_kind("usampler2D").unwrap(),
                R::ColorTex(2)
            ),
            "sb_as_uint_colortex2"
        );
        assert_eq!(
            b.add(
                "colortex2",
                sampler_kind("sampler3D").unwrap(),
                R::ColorTex(2)
            ),
            "sb_as_3d_colortex2"
        );
        assert_eq!(
            b.add(
                "colortex2",
                image_kind("image2D", None, false, false).unwrap(),
                R::ColorTex(2)
            ),
            "sb_as_image_colortex2"
        );
        assert_eq!(b.add("colortex2", s2d(), R::Atlas), "sb_as_atlas_colortex2");
        assert_eq!(
            b.add(
                "colortex2",
                sampler_kind("isampler3D").unwrap(),
                R::ColorTex(2)
            ),
            "sb_as_3d_int_colortex2"
        );
        let codes: Vec<&str> = b.diagnostics().iter().map(|d| d.code.as_str()).collect();
        assert_eq!(
            codes,
            vec![
                "binding.kind-conflict",
                "binding.kind-conflict",
                "binding.kind-conflict",
                "binding.resource-conflict",
                "binding.kind-conflict"
            ]
        );
        // A canonical name that equals an existing variant name is made unique.
        assert_eq!(
            b.add(
                "sb_as_uint_colortex2",
                s2d(),
                R::Unknown("sb_as_uint_colortex2".into())
            ),
            "sb_as_uint_colortex2_2"
        );
        let t = b.build();
        let names: HashSet<&str> = t.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names.len(), t.entries.len());
        // Variants stay next to their primary in the canonical order.
        assert_eq!(t.get("colortex2").unwrap().binding, 0);
        assert_eq!(t.get("sb_as_uint_colortex2").unwrap().binding, 1);
        assert_eq!(t.get("sb_as_image_colortex2").unwrap().set, STORAGE_SET);
    }

    #[test]
    fn image_qualifiers_merge() {
        let mut b = BindingTableBuilder::new();
        b.add(
            "colorimg4",
            image_kind("image2D", None, false, true).unwrap(),
            R::ColorImage(4),
        );
        b.add(
            "colorimg4",
            image_kind("image2D", Some("rgba16f"), false, true).unwrap(),
            R::ColorImage(4),
        );
        b.add(
            "colorimg4",
            image_kind("image2D", Some("rgba8"), true, false).unwrap(),
            R::ColorImage(4),
        );
        let (t, d) = b.finish();
        assert_eq!(
            d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
            vec!["binding.format-conflict"]
        );
        assert_eq!(t.entries.len(), 1);
        assert_eq!(
            t.entries[0].kind,
            ResourceKind::StorageImage {
                dim: "2d".into(),
                format: Some("rgba16f".into()),
                sample_type: "float".into(),
                readonly: false,
                writeonly: false
            }
        );
    }

    #[test]
    fn ssbo_bindings() {
        let mut b = BindingTableBuilder::new();
        b.add("A", ResourceKind::StorageBuffer, R::Ssbo(0));
        b.add("B", ResourceKind::StorageBuffer, R::Ssbo(0));
        b.add("C", ResourceKind::StorageBuffer, R::Unknown("C".into()));
        b.add("D", ResourceKind::StorageBuffer, R::Ssbo(1));
        b.add(
            "img",
            image_kind("image3D", None, false, false).unwrap(),
            R::Image("img".into()),
        );
        let t = b.build();
        assert_eq!(
            layout(&t),
            vec![
                ("A".into(), 2, 0),
                ("B".into(), 2, 0),
                ("D".into(), 2, 1),
                ("C".into(), 2, 2),
                ("img".into(), 2, 16)
            ]
        );
        // SSBO indices at or above the image base push the images up.
        let mut b = BindingTableBuilder::new();
        b.add("Big", ResourceKind::StorageBuffer, R::Ssbo(20));
        b.add(
            "img",
            image_kind("image3D", None, false, false).unwrap(),
            R::Image("img".into()),
        );
        assert_eq!(
            layout(&b.build()),
            vec![("Big".into(), 2, 20), ("img".into(), 2, 21)]
        );
    }

    /// Regression: `find_binding` must find primaries that `add` had to uniquify with
    /// `_<n>` (a canonical name equal to another name's conflict variant).
    #[test]
    fn find_binding_follows_uniquified_names() {
        let mut b = BindingTableBuilder::new();
        let u2d = sampler_kind("usampler2D").unwrap();
        assert_eq!(b.add("foo", s2d(), R::Unknown("foo".into())), "foo");
        assert_eq!(
            b.add("foo", u2d.clone(), R::Unknown("foo".into())),
            "sb_as_uint_foo"
        );
        // Pack samplers keep `sb_*` names (they are the interface), so one can spell a
        // variant name.
        let odd = Canonical::new("sb_as_uint_foo", R::Unknown("sb_as_uint_foo".into()));
        assert_eq!(b.add_canonical(&odd, s2d()), "sb_as_uint_foo_2");
        let odd_variant = b.add_canonical(&odd, u2d.clone());
        assert_eq!(odd_variant, "sb_as_uint_sb_as_uint_foo");
        let t = b.build();
        assert_eq!(find_binding(&t, &odd, &s2d()).unwrap().name, "sb_as_uint_foo_2");
        assert_eq!(find_binding(&t, &odd, &u2d).unwrap().name, odd_variant);
        let foo = Canonical::new("foo", R::Unknown("foo".into()));
        assert_eq!(find_binding(&t, &foo, &s2d()).unwrap().name, "foo");
        assert_eq!(find_binding(&t, &foo, &u2d).unwrap().name, "sb_as_uint_foo");
        // Other names that merely share the prefix are not derived names.
        let mut b = BindingTableBuilder::new();
        b.add("foo", u2d.clone(), R::Unknown("foo".into()));
        b.add("foo_a", s2d(), R::Unknown("foo".into()));
        b.add("foox", s2d(), R::Unknown("foo".into()));
        b.add("sb_as_uint_foo_x", s2d(), R::Unknown("foo".into()));
        assert!(find_binding(&b.build(), &foo, &s2d()).is_none());
    }

    /// Regression (#52): conflict variants were named `<name>__<suffix>`, and GLSL
    /// reserves identifiers containing `__` (glslang warns about each one).
    #[test]
    fn conflict_variants_avoid_reserved_double_underscores() {
        let mut b = BindingTableBuilder::new();
        let shadow = sampler_kind("sampler2DShadow").unwrap();
        let mut declared = Vec::new();
        for name in ["shadowtex1", "_tmp", "tmp_", "a"] {
            let resource = if name == "shadowtex1" { R::ShadowTex(1) } else { R::Unknown(name.into()) };
            let c = Canonical::new(name, resource);
            b.add_canonical(&c, s2d());
            let variant = b.add_canonical(&c, shadow.clone());
            assert!(variant.starts_with(crate::CONFLICT_PREFIX), "{variant}");
            assert!(!variant.contains("__"), "{variant}");
            declared.push((c, variant));
        }
        let t = b.build();
        for (c, variant) in &declared {
            assert_eq!(&find_binding(&t, c, &shadow).unwrap().name, variant);
            assert_eq!(find_binding(&t, c, &s2d()).unwrap().name, c.name);
        }
        assert!(t.entries.iter().all(|e| !e.name.contains("__")));
    }

    /// Regression: huge `bufferObject` indices used to saturate the image bindings
    /// (several images at binding u32::MAX).
    #[test]
    fn huge_ssbo_indices_are_rejected() {
        let mut b = BindingTableBuilder::new();
        b.add("Big", ResourceKind::StorageBuffer, R::Ssbo(u32::MAX));
        b.add("Edge", ResourceKind::StorageBuffer, R::Ssbo(MAX_SSBO_INDEX));
        for name in ["img_a", "img_b"] {
            b.add(
                name,
                image_kind("image2D", None, false, false).unwrap(),
                R::Image(name.into()),
            );
        }
        let (t, d) = b.finish();
        assert_eq!(
            d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
            vec!["binding.ssbo-index"]
        );
        assert!(d.has_errors());
        assert_eq!(
            layout(&t),
            vec![
                ("Edge".into(), 2, MAX_SSBO_INDEX),
                ("img_a".into(), 2, MAX_SSBO_INDEX + 1),
                ("img_b".into(), 2, MAX_SSBO_INDEX + 2)
            ]
        );
    }

    #[test]
    fn invalid_names_are_rejected() {
        let mut b = BindingTableBuilder::new();
        assert_eq!(b.add("", s2d(), R::Atlas), "");
        assert_eq!(b.add("gl_Foo", s2d(), R::Atlas), "gl_Foo");
        assert_eq!(b.add("a-b", s2d(), R::Atlas), "a-b");
        assert!(b.is_empty());
        assert_eq!(b.diagnostics().errors().count(), 3);
    }

    #[test]
    fn end_to_end_with_canonicalize() {
        let mut b = BindingTableBuilder::new();
        let gb = ResourceContext::new(ProgramClass::Gbuffers);
        let fs = ResourceContext::new(ProgramClass::Fullscreen);
        let mut declared = Vec::new();
        for (name, ctx) in [
            ("texture", &gb),
            ("colortex1", &gb),
            ("gaux1", &gb),
            ("colortex1", &fs),
            ("gdepthtex", &fs),
            ("tex", &fs),
        ] {
            let c = canonicalize(name, ctx);
            let binding = b.add_canonical(&c, s2d());
            declared.push((name, binding, c));
        }
        let t = b.build();
        assert_eq!(
            layout(&t),
            vec![
                ("gtexture".into(), 1, 0),
                ("colortex0".into(), 1, 1),
                ("colortex1".into(), 1, 2),
                ("colortex4".into(), 1, 3),
                ("depthtex0".into(), 1, 4)
            ]
        );
        for (_, binding, c) in &declared {
            assert_eq!(&find_binding(&t, c, &s2d()).unwrap().name, binding);
        }
    }

    #[test]
    fn ranks() {
        assert!(rank("gtexture") < rank("colortex0"));
        assert!(rank("colortex31") < rank("depthtex0"));
        assert!(rank("depthtex2") < rank("shadowtex0"));
        assert!(rank("shadowtex1HW") < rank("shadowcolor0"));
        assert!(rank("shadowcolor7") < rank("noisetex"));
        assert!(rank("noisetex") < rank("dhDepthTex0"));
        assert!(rank("dhBlockAtlas") < rank("colorimg0"));
        assert!(rank("colorimg31") < rank("shadowcolorimg0"));
        assert!(rank("shadowcolorimg7") < rank("anything"));
        assert!(kinds_compatible(&s2d(), &s2d()));
        assert!(!kinds_compatible(
            &s2d(),
            &sampler_kind("sampler2DShadow").unwrap()
        ));
        assert!(kinds_compatible(
            &image_kind("image2D", Some("rgba8"), true, false).unwrap(),
            &image_kind("image2D", None, false, true).unwrap()
        ));
    }
}
