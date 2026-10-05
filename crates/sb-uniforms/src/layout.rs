//! std140 layout of the pack-global uniform blocks `sb_Frame` and `sb_Draw`
//! (ARCHITECTURE §5.1).
//!
//! Every loose (non-opaque) uniform declared by any program of a pack, plus the
//! builtins referenced by custom-uniform expressions and the custom uniforms
//! themselves, becomes a member of one of the two blocks. Each translated program
//! declares only the members it uses, with explicit `layout(offset = N)`, so all
//! programs share one buffer per block.
//!
//! # Name and type conflicts
//!
//! Declarations are deduplicated by name. When the same name is declared with
//! different types, the **first** type keeps the plain name and every other type gets
//! its own member [`conflict_name`]`(name, type)` = `sb_as_<type>_<name>` (for example
//! `sb_as_float_worldTime`, `sb_as_vec3_8_lights` for `vec3[8]`), with a
//! `uniform.type-conflict` warning. [`MemberIndex`] maps each
//! `(declared name, declared type)` pair to its member so the transformer can rename
//! references.
//!
//! # Builtin type checks
//!
//! A declaration sourced from a builtin whose registry type is incompatible with the
//! declared type (see [`registry::is_type_compatible`]) becomes
//! [`UniformSource::Unset`] (zero-filled) with a `uniform.builtin-type-mismatch`
//! warning, because Iris disables such uniforms. Members sourced from a builtin always
//! carry the *declared* type; the host converts the builtin value to it (only
//! `bool` <-> `int` scalar conversions can occur).
//!
//! # Packing
//!
//! Members are placed in order of descending std140 alignment, then name, and a member
//! is placed into the first padding hole left by an earlier member when it fits (a
//! `float` after a `vec3` takes the vec3's fourth slot). The result is deterministic
//! for a given set of declarations. [`BlockLayout::members`] is sorted by offset, the
//! order in which GLSL requires explicitly-offset members to be declared. Block sizes
//! are rounded up to 16 bytes, with a minimum of 16 so that hosts never create an empty
//! buffer.

use crate::naming::{conflict_name, uniquified_name};
use crate::registry::{self, Frequency};
use indexmap::IndexMap;
use sb_core::model::{BlockLayout, BlockMember, UniformLayout, UniformSource};
use sb_core::{Diagnostic, Diagnostics, GlslType, ScalarKind, SourceLocation};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// GLSL name of the per-frame block.
pub const FRAME_BLOCK_NAME: &str = "sb_Frame";
/// GLSL name of the per-draw block.
pub const DRAW_BLOCK_NAME: &str = "sb_Draw";
/// Descriptor set of both uniform blocks.
pub const UNIFORM_SET: u32 = 0;
/// Binding of `sb_Frame` in [`UNIFORM_SET`].
pub const FRAME_BINDING: u32 = 0;
/// Binding of `sb_Draw` in [`UNIFORM_SET`].
pub const DRAW_BINDING: u32 = 1;
/// Largest uniform block every Vulkan implementation supports
/// (`maxUniformBufferRange` minimum). Larger blocks get a warning.
pub const PORTABLE_MAX_BLOCK_SIZE: u32 = 16384;
/// Largest single member accepted (1 MiB); larger (malformed) arrays are rejected.
pub const MAX_MEMBER_SIZE: u64 = 1 << 20;

/// One loose uniform declaration (from a program, a custom uniform or a profile).
#[derive(Debug, Clone, PartialEq)]
pub struct UniformDecl {
    /// GLSL name as declared.
    pub name: String,
    /// Declared type, including the array length.
    pub ty: GlslType,
    /// Constant initializer (`uniform T x = init;`): the component values in GLSL
    /// constructor order (column-major for matrices, element after element for arrays),
    /// *not* padded to std140. The length must equal [`component_count`]`(ty)`.
    pub default: Option<Vec<f32>>,
    /// Where the host gets the value from.
    pub source: UniformSource,
    /// Program (e.g. `world0/gbuffers_terrain`) for diagnostics.
    pub program: Option<String>,
    /// Declaration site for diagnostics.
    pub location: Option<SourceLocation>,
}

impl UniformDecl {
    /// A declaration with an explicit source.
    pub fn new(name: impl Into<String>, ty: GlslType, source: UniformSource) -> Self {
        Self {
            name: name.into(),
            ty,
            default: None,
            source,
            program: None,
            location: None,
        }
    }

    /// A declaration whose source is looked up in the registry
    /// ([`UniformSource::Builtin`] for builtins, [`UniformSource::Unset`] otherwise).
    pub fn from_registry(name: impl Into<String>, ty: GlslType) -> Self {
        let name = name.into();
        let source = registry::source_of(&name);
        Self::new(name, ty, source)
    }

    /// A custom uniform (`uniform.<type>.<name>` in `shaders.properties`).
    pub fn custom(name: impl Into<String>, ty: GlslType) -> Self {
        let name = name.into();
        let source = UniformSource::Custom(name.clone());
        Self::new(name, ty, source)
    }

    /// Set the constant initializer (see [`UniformDecl::default`]).
    pub fn with_default(mut self, values: Vec<f32>) -> Self {
        self.default = Some(values);
        self
    }

    /// Set the program used in diagnostics.
    pub fn in_program(mut self, program: impl Into<String>) -> Self {
        self.program = Some(program.into());
        self
    }

    /// Set the declaration site used in diagnostics.
    pub fn at(mut self, location: SourceLocation) -> Self {
        self.location = Some(location);
        self
    }

    /// The block this declaration belongs to by default: the registry frequency for
    /// [`UniformSource::Builtin`] sources, [`Frequency::Frame`] for custom and unset ones.
    pub fn frequency(&self) -> Frequency {
        match &self.source {
            UniformSource::Builtin(n) => registry::frequency_of(n),
            UniformSource::Custom(_) | UniformSource::Unset => Frequency::Frame,
        }
    }

    fn diag(&self, d: Diagnostic) -> Diagnostic {
        let d = d.at_opt(self.location.clone());
        match &self.program {
            Some(p) => d.in_program(p.clone()),
            None => d,
        }
    }
}

/// Number of scalar components of `ty` (rows × columns × array length).
pub fn component_count(ty: GlslType) -> u64 {
    u64::from(ty.rows) * u64::from(ty.cols) * u64::from(ty.array.unwrap_or(1))
}

/// Where the member for a `(name, type)` declaration lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberRef {
    /// `sb_Frame` or `sb_Draw`.
    pub block: Frequency,
    /// GLSL member name (the declared name, or [`conflict_name`]`(name, type)` =
    /// `sb_as_<type>_<name>` for a conflicting type).
    pub member: String,
    /// std140 byte offset in the block.
    pub offset: u32,
    /// Member type (equal to the declared type).
    pub ty: GlslType,
    /// Value source of the member.
    pub source: UniformSource,
}

/// Lookup from `(declared name, declared type)` to the block member, produced by
/// [`LayoutBuilder::build`] / [`build_uniform_layout`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberIndex {
    by_name: IndexMap<String, Vec<MemberRef>>,
}

impl MemberIndex {
    /// The member for a declaration of `name` with type `ty`.
    pub fn get(&self, name: &str, ty: GlslType) -> Option<&MemberRef> {
        self.by_name.get(name)?.iter().find(|m| m.ty == ty)
    }

    /// The member name for a declaration of `name` with type `ty`.
    pub fn member_name(&self, name: &str, ty: GlslType) -> Option<&str> {
        self.get(name, ty).map(|m| m.member.as_str())
    }

    /// All members created for declarations of `name` (first type first).
    pub fn variants(&self, name: &str) -> &[MemberRef] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }

    /// Whether any declaration of `name` was laid out.
    pub fn contains_name(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// Number of `(name, type)` pairs.
    pub fn len(&self) -> usize {
        self.by_name.values().map(Vec::len).sum()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Every `(declared name, member)` pair, in first-declaration order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &MemberRef)> {
        self.by_name
            .iter()
            .flat_map(|(n, v)| v.iter().map(move |m| (n.as_str(), m)))
    }
}

#[derive(Debug, Clone)]
struct Variant {
    ty: GlslType,
    default: Option<Vec<f32>>,
    source: UniformSource,
}

#[derive(Debug, Clone)]
struct NameEntry {
    block: Frequency,
    /// `variants[0]` keeps the plain name.
    variants: Vec<Variant>,
}

/// Collects loose uniform declarations of a whole pack and lays out `sb_Frame` and
/// `sb_Draw` (see the [module docs](self)).
#[derive(Debug, Clone, Default)]
pub struct LayoutBuilder {
    entries: IndexMap<String, NameEntry>,
    diagnostics: Diagnostics,
}

impl LayoutBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a declaration to the block chosen by [`UniformDecl::frequency`].
    pub fn add(&mut self, decl: UniformDecl) {
        let block = decl.frequency();
        self.add_to(block, decl);
    }

    /// Add a declaration to `sb_Frame`.
    pub fn add_frame(&mut self, decl: UniformDecl) {
        self.add_to(Frequency::Frame, decl);
    }

    /// Add a declaration to `sb_Draw`.
    pub fn add_draw(&mut self, decl: UniformDecl) {
        self.add_to(Frequency::Draw, decl);
    }

    /// Add a declaration to the given block. A name already placed in the other block
    /// stays there (both blocks share one GLSL namespace), with a warning.
    pub fn add_to(&mut self, block: Frequency, decl: UniformDecl) {
        if !is_valid_name(&decl.name) {
            self.diagnostics.push(decl.diag(Diagnostic::error(
                "uniform.invalid-name",
                format!(
                    "`{}` is not a valid uniform name; the declaration is ignored",
                    decl.name
                ),
            )));
            return;
        }
        if let Err(why) = check_type(decl.ty) {
            self.diagnostics.push(decl.diag(Diagnostic::error(
                "uniform.invalid-type",
                format!(
                    "uniform `{}` has unsupported type {}: {why}; the declaration is ignored",
                    decl.name, decl.ty
                ),
            )));
            return;
        }

        let source = self.checked_source(&decl);
        let default = self.checked_default(&decl);

        let Some(entry) = self.entries.get_mut(&decl.name) else {
            self.entries.insert(
                decl.name.clone(),
                NameEntry {
                    block,
                    variants: vec![Variant {
                        ty: decl.ty,
                        default,
                        source,
                    }],
                },
            );
            return;
        };

        if entry.block != block {
            self.diagnostics.push(decl.diag(Diagnostic::warning(
                "uniform.block-conflict",
                format!(
                    "uniform `{}` is requested in {} but already lives in {}; it stays in {}",
                    decl.name,
                    block.block_name(),
                    entry.block.block_name(),
                    entry.block.block_name()
                ),
            )));
        }

        if let Some(v) = entry.variants.iter_mut().find(|v| v.ty == decl.ty) {
            // Same name and type: merge.
            if source_rank(&source) > source_rank(&v.source) {
                v.source = source;
            } else if source_rank(&source) == source_rank(&v.source) && source != v.source {
                self.diagnostics.push(decl.diag(Diagnostic::warning(
                    "uniform.source-conflict",
                    format!(
                        "uniform `{}` is declared with sources {:?} and {:?}; the first one is used",
                        decl.name, v.source, source
                    ),
                )));
            }
            match (&v.default, default) {
                (None, Some(d)) => v.default = Some(d),
                (Some(a), Some(b)) if *a != b => self.diagnostics.push(decl.diag(Diagnostic::warning(
                    "uniform.default-conflict",
                    format!("uniform `{}` has different initializers in different programs; the first one is used", decl.name),
                ))),
                _ => {}
            }
            return;
        }

        let first = entry.variants[0].ty;
        entry.variants.push(Variant {
            ty: decl.ty,
            default,
            source,
        });
        self.diagnostics.push(decl.diag(Diagnostic::warning(
            "uniform.type-conflict",
            format!(
                "uniform `{}` is declared as {} here but as {} elsewhere; this declaration gets its own member `{}`",
                decl.name,
                decl.ty,
                first,
                conflict_name(&decl.name, &type_suffix(decl.ty))
            ),
        )));
    }

    /// Diagnostics collected so far.
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Lay out both blocks.
    pub fn build(self) -> (UniformLayout, MemberIndex, Diagnostics) {
        let LayoutBuilder {
            entries,
            mut diagnostics,
        } = self;

        // 1. Member names: the first type keeps the name, the others get a unique
        //    `sb_as_<type>_<name>` (both blocks share the GLSL namespace).
        let mut taken: HashSet<String> = entries.keys().cloned().collect();
        let mut member_names: Vec<Vec<String>> = Vec::with_capacity(entries.len());
        for (name, entry) in &entries {
            let mut names = Vec::with_capacity(entry.variants.len());
            names.push(name.clone());
            for v in &entry.variants[1..] {
                let base = conflict_name(name, &type_suffix(v.ty));
                let mut candidate = base.clone();
                let mut n = 2;
                while taken.contains(&candidate) {
                    candidate = uniquified_name(&base, n);
                    n += 1;
                }
                taken.insert(candidate.clone());
                names.push(candidate);
            }
            member_names.push(names);
        }

        // 2. Pack each block.
        let mut offsets: Vec<Vec<u32>> = entries
            .values()
            .map(|e| vec![0; e.variants.len()])
            .collect();
        let mut blocks = Vec::with_capacity(2);
        let mut failed: HashSet<Frequency> = HashSet::new();
        for block in [Frequency::Frame, Frequency::Draw] {
            let mut items: Vec<Item> = Vec::new();
            for (ei, entry) in entries.values().enumerate() {
                if entry.block != block {
                    continue;
                }
                for (vi, v) in entry.variants.iter().enumerate() {
                    items.push(Item {
                        entry: ei,
                        variant: vi,
                        name: member_names[ei][vi].clone(),
                        ty: v.ty,
                    });
                }
            }
            let (placed, end) = pack(items);
            let size = match u32::try_from(end.max(1).div_ceil(16) * 16) {
                Ok(s) => s,
                Err(_) => {
                    diagnostics.push(Diagnostic::error(
                        "uniform.block-too-large",
                        format!(
                            "{} would be {end} bytes; it cannot be laid out",
                            block.block_name()
                        ),
                    ));
                    failed.insert(block);
                    blocks.push(BlockLayout {
                        name: block.block_name().to_string(),
                        set: UNIFORM_SET,
                        binding: block.binding(),
                        size: 16,
                        members: Vec::new(),
                    });
                    continue;
                }
            };
            if size > PORTABLE_MAX_BLOCK_SIZE {
                diagnostics.push(Diagnostic::warning(
                    "uniform.block-too-large",
                    format!(
                        "{} is {size} bytes, more than the {PORTABLE_MAX_BLOCK_SIZE} bytes every Vulkan device supports",
                        block.block_name()
                    ),
                ));
            }
            let mut members = Vec::with_capacity(placed.len());
            for (item, offset) in placed {
                // `offset < end <= u32::MAX` was checked above.
                let offset = u32::try_from(offset).unwrap_or(u32::MAX);
                offsets[item.entry][item.variant] = offset;
                let v = &entries[item.entry].variants[item.variant];
                members.push(BlockMember {
                    name: item.name,
                    ty: v.ty,
                    offset,
                    source: v.source.clone(),
                    default: v.default.clone(),
                });
            }
            blocks.push(BlockLayout {
                name: block.block_name().to_string(),
                set: UNIFORM_SET,
                binding: block.binding(),
                size,
                members,
            });
        }
        let draw = blocks.pop().unwrap_or_default();
        let frame = blocks.pop().unwrap_or_default();

        // 3. Index (members of a block that could not be laid out are left out).
        let mut index = MemberIndex::default();
        for (ei, (name, entry)) in entries.into_iter().enumerate() {
            if failed.contains(&entry.block) {
                continue;
            }
            let refs = entry
                .variants
                .into_iter()
                .enumerate()
                .map(|(vi, v)| MemberRef {
                    block: entry.block,
                    member: member_names[ei][vi].clone(),
                    offset: offsets[ei][vi],
                    ty: v.ty,
                    source: v.source,
                })
                .collect();
            index.by_name.insert(name, refs);
        }

        (UniformLayout { frame, draw }, index, diagnostics)
    }

    fn checked_source(&mut self, decl: &UniformDecl) -> UniformSource {
        let UniformSource::Builtin(builtin) = &decl.source else {
            return decl.source.clone();
        };
        match registry::get(builtin) {
            None => {
                self.diagnostics.push(decl.diag(Diagnostic::warning(
                    "uniform.unknown-builtin",
                    format!(
                        "uniform `{}` refers to unknown builtin `{builtin}`; it is zero-filled",
                        decl.name
                    ),
                )));
                UniformSource::Unset
            }
            Some(b) if !registry::is_type_compatible(b.ty, decl.ty) => {
                self.diagnostics.push(decl.diag(Diagnostic::warning(
                    "uniform.builtin-type-mismatch",
                    format!(
                        "builtin uniform `{}` is {} but declared as {}; like Iris, the declaration is not set (zero-filled)",
                        b.name, b.ty, decl.ty
                    ),
                )));
                UniformSource::Unset
            }
            Some(_) => decl.source.clone(),
        }
    }

    fn checked_default(&mut self, decl: &UniformDecl) -> Option<Vec<f32>> {
        let values = decl.default.as_ref()?;
        let expected = component_count(decl.ty);
        if values.len() as u64 != expected {
            self.diagnostics.push(decl.diag(Diagnostic::warning(
                "uniform.default-mismatch",
                format!(
                    "initializer of uniform `{}` has {} values but {} has {expected} components; it is ignored",
                    decl.name,
                    values.len(),
                    decl.ty
                ),
            )));
            return None;
        }
        Some(values.clone())
    }
}

/// Lay out `sb_Frame` from `frame_decls` and `sb_Draw` from `draw_decls`
/// (see [`LayoutBuilder`]). Use [`registry::frequency_of`] (or
/// [`UniformDecl::frequency`]) to split declarations.
pub fn build_uniform_layout(
    frame_decls: impl IntoIterator<Item = UniformDecl>,
    draw_decls: impl IntoIterator<Item = UniformDecl>,
) -> (UniformLayout, MemberIndex, Diagnostics) {
    let mut b = LayoutBuilder::new();
    for d in frame_decls {
        b.add_frame(d);
    }
    for d in draw_decls {
        b.add_draw(d);
    }
    b.build()
}

/// Check that a block layout is a valid explicit-offset std140 block: members sorted by
/// offset, offsets aligned, no overlaps, unique names, size a multiple of 16 covering
/// every member. Returns one error diagnostic per problem (empty when valid). Useful
/// for hosts that receive layouts as JSON.
pub fn validate_block(block: &BlockLayout) -> Diagnostics {
    let mut out = Diagnostics::new();
    let mut err = |msg: String| {
        out.push(Diagnostic::error(
            "uniform.invalid-layout",
            format!("{}: {msg}", block.name),
        ))
    };
    if !block.size.is_multiple_of(16) {
        err(format!("size {} is not a multiple of 16", block.size));
    }
    let mut names = HashSet::new();
    let mut prev_end: u64 = 0;
    let mut prev_offset: Option<u32> = None;
    for m in &block.members {
        if !names.insert(m.name.as_str()) {
            err(format!("duplicate member `{}`", m.name));
        }
        if let Err(why) = check_type(m.ty) {
            err(format!(
                "member `{}` has unsupported type {}: {why}",
                m.name, m.ty
            ));
            continue;
        }
        let (align, size) = align_size(m.ty);
        if !u64::from(m.offset).is_multiple_of(align) {
            err(format!(
                "member `{}` at offset {} is not aligned to {align}",
                m.name, m.offset
            ));
        }
        if prev_offset.is_some_and(|p| m.offset < p) {
            err(format!("member `{}` is not sorted by offset", m.name));
        }
        if u64::from(m.offset) < prev_end {
            err(format!(
                "member `{}` at offset {} overlaps the previous member",
                m.name, m.offset
            ));
        }
        prev_offset = Some(m.offset);
        prev_end = u64::from(m.offset) + size;
    }
    if prev_end > u64::from(block.size) {
        err(format!(
            "size {} does not cover the members (end {prev_end})",
            block.size
        ));
    }
    out
}

/// Member-name suffix for a type: `float`, `vec3`, `mat3x4`, `vec3_8` for `vec3[8]`.
pub fn type_suffix(ty: GlslType) -> String {
    match ty.array {
        Some(n) => format!("{}_{n}", ty.glsl_name()),
        None => ty.glsl_name(),
    }
}

fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with("gl_")
}

/// Validate a member type, including that its std140 size is sane.
fn check_type(ty: GlslType) -> Result<(), &'static str> {
    if !(1..=4).contains(&ty.rows) || !(1..=4).contains(&ty.cols) {
        return Err("vectors and matrices have 1 to 4 rows and columns");
    }
    if ty.cols > 1 && (ty.rows < 2 || !matches!(ty.scalar, ScalarKind::Float | ScalarKind::Double))
    {
        return Err("matrices are float or double with at least 2 rows");
    }
    if ty.array == Some(0) {
        return Err("arrays in uniform blocks need a positive length");
    }
    if align_size(ty).1 > MAX_MEMBER_SIZE {
        return Err("the member is larger than 1 MiB");
    }
    Ok(())
}

/// (std140 alignment, std140 size) computed without overflow. For valid
/// (`check_type`-approved) shapes this equals sb-core's `std140_align`/`std140_size`.
fn align_size(ty: GlslType) -> (u64, u64) {
    let align = u64::from(ty.std140_align());
    match ty.array {
        // sb-core's `std140_size` multiplies in u32 and overflows for huge lengths.
        Some(len) => (align, u64::from(ty.std140_array_stride()) * u64::from(len)),
        None => (align, u64::from(ty.std140_size())),
    }
}

fn source_rank(s: &UniformSource) -> u8 {
    match s {
        UniformSource::Unset => 0,
        UniformSource::Builtin(_) => 1,
        UniformSource::Custom(_) => 2,
    }
}

#[derive(Debug)]
struct Item {
    entry: usize,
    variant: usize,
    name: String,
    ty: GlslType,
}

/// Place items (descending alignment, then name; first-fit into padding holes).
/// Returns the items with their offsets, sorted by offset, and the end offset.
fn pack(mut items: Vec<Item>) -> (Vec<(Item, u64)>, u64) {
    items.sort_by(|a, b| {
        align_size(b.ty)
            .0
            .cmp(&align_size(a.ty).0)
            .then_with(|| a.name.cmp(&b.name))
    });
    let mut holes: Vec<(u64, u64)> = Vec::new(); // [start, end), sorted by start
    let mut end: u64 = 0;
    let mut placed = Vec::with_capacity(items.len());
    for item in items {
        let (align, size) = align_size(item.ty);
        let hole = holes
            .iter()
            .position(|&(s, e)| s.next_multiple_of(align) + size <= e);
        let offset = if let Some(i) = hole {
            let (s, e) = holes.remove(i);
            let offset = s.next_multiple_of(align);
            let mut insert_at = i;
            if offset > s {
                holes.insert(insert_at, (s, offset));
                insert_at += 1;
            }
            if offset + size < e {
                holes.insert(insert_at, (offset + size, e));
            }
            offset
        } else {
            let offset = end.next_multiple_of(align);
            if offset > end {
                holes.push((end, offset));
            }
            end = offset + size;
            offset
        };
        placed.push((item, offset));
    }
    placed.sort_by_key(|(_, o)| *o);
    (placed, end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn unset(name: &str, ty: GlslType) -> UniformDecl {
        UniformDecl::new(name, ty, UniformSource::Unset)
    }

    fn offsets(block: &BlockLayout) -> Vec<(&str, u32)> {
        block
            .members
            .iter()
            .map(|m| (m.name.as_str(), m.offset))
            .collect()
    }

    fn assert_valid(layout: &UniformLayout) {
        for b in [&layout.frame, &layout.draw] {
            let d = validate_block(b);
            assert!(d.is_empty(), "{d:?}");
        }
    }

    /// sb-core's std140 helpers against hand-computed std140 values.
    #[test]
    fn sb_core_std140_rules() {
        let t = |s: &str| GlslType::parse(s).unwrap();
        // (type, align, size, array stride of type[2])
        let cases: &[(GlslType, u32, u32, u32)] = &[
            (t("float"), 4, 4, 16),
            (t("int"), 4, 4, 16),
            (t("uint"), 4, 4, 16),
            (t("bool"), 4, 4, 16),
            (t("vec2"), 8, 8, 16),
            (t("ivec2"), 8, 8, 16),
            (t("vec3"), 16, 12, 16),
            (t("bvec3"), 16, 12, 16),
            (t("vec4"), 16, 16, 16),
            (t("uvec4"), 16, 16, 16),
            (t("mat2"), 16, 32, 32),
            (t("mat3"), 16, 48, 48),
            (t("mat4"), 16, 64, 64),
            (t("mat2x3"), 16, 32, 32),
            (t("mat3x2"), 16, 48, 48),
            (t("mat3x4"), 16, 48, 48),
            (t("mat4x2"), 16, 64, 64),
            (t("mat4x3"), 16, 64, 64),
            (t("double"), 8, 8, 16),
            (t("dvec2"), 16, 16, 16),
            (t("dvec3"), 32, 24, 32),
            (t("dvec4"), 32, 32, 32),
            (t("dmat2"), 16, 32, 32),
            (t("dmat3"), 32, 96, 96),
            (t("dmat4"), 32, 128, 128),
        ];
        for &(ty, align, size, stride) in cases {
            assert_eq!(ty.std140_align(), align, "align of {ty}");
            assert_eq!(ty.std140_size(), size, "size of {ty}");
            let arr = ty.with_array(2);
            assert_eq!(arr.std140_array_stride(), stride, "stride of {arr}");
            assert_eq!(arr.std140_align(), align.max(16), "align of {arr}");
            assert_eq!(arr.std140_size(), stride * 2, "size of {arr}");
            assert_eq!(
                align_size(arr),
                (u64::from(align.max(16)), u64::from(stride * 2))
            );
            assert_eq!(align_size(ty), (u64::from(align), u64::from(size)));
        }
    }

    #[test]
    fn vec3_then_float_shares_the_slot() {
        let (l, idx, d) = build_uniform_layout(
            [unset("a", GlslType::VEC3), unset("b", GlslType::FLOAT)],
            [],
        );
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(offsets(&l.frame), vec![("a", 0), ("b", 12)]);
        assert_eq!(l.frame.size, 16);
        assert_eq!(idx.get("b", GlslType::FLOAT).unwrap().offset, 12);
        assert_valid(&l);
    }

    #[test]
    fn descending_alignment_then_name_with_hole_filling() {
        let decls = [
            unset("x", GlslType::FLOAT),
            unset("m", GlslType::MAT4),
            unset("v", GlslType::VEC2),
            unset("p", GlslType::VEC3),
            unset("q", GlslType::VEC3),
            unset("y", GlslType::FLOAT),
            unset("n", GlslType::MAT3),
        ];
        let (l, _, d) = build_uniform_layout(decls, []);
        assert!(d.is_empty(), "{d:?}");
        // align 16: m(mat4) 0..64, n(mat3) 64..112, p(vec3) 112..124, q(vec3) 128..140
        // align 8: v at 144 (holes of 4 bytes are too small)
        // align 4: x into the hole at 124, y into the hole at 140
        assert_eq!(
            offsets(&l.frame),
            vec![
                ("m", 0),
                ("n", 64),
                ("p", 112),
                ("x", 124),
                ("q", 128),
                ("y", 140),
                ("v", 144)
            ]
        );
        assert_eq!(l.frame.size, 160);
        assert_valid(&l);
    }

    #[test]
    fn arrays_and_matrices() {
        let decls = [
            unset("f4", GlslType::FLOAT.with_array(4)),
            unset("a", GlslType::FLOAT),
            unset("v3", GlslType::VEC3.with_array(2)),
            unset("m3", GlslType::MAT3),
            unset("b", GlslType::INT),
        ];
        let (l, _, d) = build_uniform_layout(decls, []);
        assert!(d.is_empty(), "{d:?}");
        // all of f4 (64), m3 (48), v3 (32) have alignment 16; names: f4 < m3 < v3
        assert_eq!(
            offsets(&l.frame),
            vec![("f4", 0), ("m3", 64), ("v3", 112), ("a", 144), ("b", 148)]
        );
        assert_eq!(l.frame.size, 160);
        assert_valid(&l);
    }

    #[test]
    fn dvec3_hole_takes_vec2() {
        let decls = [
            unset("d", GlslType::parse("dvec3").unwrap()),
            unset("e", GlslType::parse("dvec4").unwrap()),
            unset("v", GlslType::VEC2),
        ];
        let (l, _, _) = build_uniform_layout(decls, []);
        assert_eq!(offsets(&l.frame), vec![("d", 0), ("v", 24), ("e", 32)]);
        assert_eq!(l.frame.size, 64);
        assert_valid(&l);
    }

    #[test]
    fn blocks_have_fixed_names_and_bindings() {
        let (l, _, _) = build_uniform_layout([], []);
        assert_eq!(
            (
                l.frame.name.as_str(),
                l.frame.set,
                l.frame.binding,
                l.frame.size
            ),
            ("sb_Frame", 0, 0, 16)
        );
        assert_eq!(
            (
                l.draw.name.as_str(),
                l.draw.set,
                l.draw.binding,
                l.draw.size
            ),
            ("sb_Draw", 0, 1, 16)
        );
        assert!(l.frame.members.is_empty() && l.draw.members.is_empty());
    }

    #[test]
    fn dedup_same_name_and_type() {
        let decls = [
            UniformDecl::from_registry("frameTimeCounter", GlslType::FLOAT).in_program("composite"),
            UniformDecl::from_registry("frameTimeCounter", GlslType::FLOAT).in_program("final"),
        ];
        let (l, idx, d) = build_uniform_layout(decls, []);
        assert!(d.is_empty());
        assert_eq!(l.frame.members.len(), 1);
        assert_eq!(
            l.frame.members[0].source,
            UniformSource::Builtin("frameTimeCounter".into())
        );
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn type_conflict_creates_a_variant() {
        let decls = [
            UniformDecl::from_registry("worldTime", GlslType::INT).in_program("gbuffers_terrain"),
            UniformDecl::from_registry("worldTime", GlslType::FLOAT).in_program("composite"),
            UniformDecl::from_registry("worldTime", GlslType::FLOAT).in_program("final"),
        ];
        let (l, idx, d) = build_uniform_layout(decls, []);
        let codes: Vec<&str> = d.iter().map(|d| d.code.as_str()).collect();
        // The float declarations do not match the builtin type (zero-filled, like Iris),
        // and get their own member.
        assert_eq!(
            codes,
            vec![
                "uniform.builtin-type-mismatch",
                "uniform.type-conflict",
                "uniform.builtin-type-mismatch"
            ]
        );
        assert!(d.iter().all(|d| d.severity == sb_core::Severity::Warning));
        assert_eq!(d.0[1].program.as_deref(), Some("composite"));
        let int = idx.get("worldTime", GlslType::INT).unwrap();
        let float = idx.get("worldTime", GlslType::FLOAT).unwrap();
        assert_eq!(int.member, "worldTime");
        assert_eq!(int.source, UniformSource::Builtin("worldTime".into()));
        assert_eq!(float.member, "sb_as_float_worldTime");
        assert_eq!(float.source, UniformSource::Unset);
        assert_eq!(
            idx.member_name("worldTime", GlslType::FLOAT),
            Some("sb_as_float_worldTime")
        );
        assert!(idx.get("worldTime", GlslType::VEC2).is_none());
        assert_eq!(idx.variants("worldTime").len(), 2);
        assert_eq!(l.frame.members.len(), 2);
        assert_eq!(
            l.frame.member("sb_as_float_worldTime").unwrap().ty,
            GlslType::FLOAT
        );
        assert_valid(&l);
    }

    #[test]
    fn first_declared_type_keeps_the_name() {
        let decls = [
            unset("foo", GlslType::VEC2),
            unset("foo", GlslType::VEC3.with_array(8)),
        ];
        let (_, idx, _) = build_uniform_layout(decls, []);
        assert_eq!(idx.member_name("foo", GlslType::VEC2), Some("foo"));
        assert_eq!(
            idx.member_name("foo", GlslType::VEC3.with_array(8)),
            Some("sb_as_vec3_8_foo")
        );
    }

    #[test]
    fn variant_names_never_collide() {
        // Pack uniforms keep their names even when they use our `sb_as_` prefix (the
        // translator only renames non-uniform `sb_*` identifiers); variants then get a
        // counter.
        let decls = [
            unset("sb_as_float_worldTime", GlslType::VEC4),
            UniformDecl::from_registry("worldTime", GlslType::INT),
            UniformDecl::from_registry("worldTime", GlslType::FLOAT),
            unset("sb_as_float_worldTime_2", GlslType::INT),
        ];
        let (l, idx, _) = build_uniform_layout(decls, []);
        assert_eq!(
            idx.member_name("worldTime", GlslType::FLOAT),
            Some("sb_as_float_worldTime_3")
        );
        assert_eq!(
            idx.member_name("sb_as_float_worldTime", GlslType::VEC4),
            Some("sb_as_float_worldTime")
        );
        assert_valid(&l);
    }

    /// Regression (#52): conflict members used `<name>__<type>`, and GLSL reserves
    /// identifiers containing `__` (glslang warns about every one).
    #[test]
    fn conflict_members_avoid_reserved_double_underscores() {
        let decls = [
            unset("tint_", GlslType::VEC3),
            unset("tint_", GlslType::VEC4),
            unset("_tint", GlslType::VEC3),
            unset("_tint", GlslType::VEC4),
            unset("tint", GlslType::FLOAT.with_array(4)),
            unset("tint", GlslType::INT),
        ];
        let (l, idx, d) = build_uniform_layout(decls, []);
        assert_eq!(d.iter().filter(|d| d.code == "uniform.type-conflict").count(), 3);
        for (name, ty) in [("tint_", GlslType::VEC4), ("_tint", GlslType::VEC4), ("tint", GlslType::INT)] {
            let member = idx.member_name(name, ty).unwrap();
            assert!(member.starts_with(crate::CONFLICT_PREFIX), "{member}");
            assert!(!member.contains("__"), "{member}");
            assert!(crate::is_derived_name(member, name), "{member}");
        }
        // Underscores at the edges of the pack name fold into the separators.
        assert_eq!(idx.member_name("_tint", GlslType::VEC4), Some("sb_as_vec4_tint"));
        assert_eq!(idx.member_name("tint_", GlslType::VEC4), Some("sb_as_vec4_tint_"));
        assert_eq!(idx.member_name("tint", GlslType::INT), Some("sb_as_int_tint"));
        assert_valid(&l);
    }

    #[test]
    fn bool_and_int_builtins_are_compatible() {
        let decls = [
            UniformDecl::from_registry("hideGUI", GlslType::INT),
            UniformDecl::from_registry("hideGUI", GlslType::BOOL),
        ];
        let (_, idx, d) = build_uniform_layout(decls, []);
        assert_eq!(
            d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
            vec!["uniform.type-conflict"]
        );
        assert_eq!(
            idx.get("hideGUI", GlslType::INT).unwrap().source,
            UniformSource::Builtin("hideGUI".into())
        );
        let b = idx.get("hideGUI", GlslType::BOOL).unwrap();
        assert_eq!(b.member, "sb_as_bool_hideGUI");
        assert_eq!(b.source, UniformSource::Builtin("hideGUI".into()));
    }

    #[test]
    fn unknown_builtin_source_becomes_unset() {
        let decls = [UniformDecl::new(
            "foo",
            GlslType::FLOAT,
            UniformSource::Builtin("noSuchBuiltin".into()),
        )];
        let (l, _, d) = build_uniform_layout(decls, []);
        assert_eq!(d.0[0].code, "uniform.unknown-builtin");
        assert_eq!(l.frame.members[0].source, UniformSource::Unset);
    }

    #[test]
    fn source_precedence_custom_over_builtin_over_unset() {
        let decls = [
            unset("x", GlslType::FLOAT),
            UniformDecl::new(
                "x",
                GlslType::FLOAT,
                UniformSource::Builtin("frameTime".into()),
            ),
            UniformDecl::custom("x", GlslType::FLOAT),
            UniformDecl::new(
                "x",
                GlslType::FLOAT,
                UniformSource::Builtin("frameTime".into()),
            ),
        ];
        let (l, _, d) = build_uniform_layout(decls, []);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(l.frame.members[0].source, UniformSource::Custom("x".into()));
    }

    #[test]
    fn defaults() {
        let decls = [
            unset("a", GlslType::VEC2).with_default(vec![1.0, 2.0]),
            unset("a", GlslType::VEC2).with_default(vec![1.0, 2.0]),
            unset("b", GlslType::FLOAT),
            unset("b", GlslType::FLOAT).with_default(vec![0.5]),
            unset("c", GlslType::VEC3).with_default(vec![1.0]),
            unset("d", GlslType::FLOAT).with_default(vec![1.0]),
            unset("d", GlslType::FLOAT).with_default(vec![2.0]),
            unset("m", GlslType::MAT2.with_array(2)).with_default(vec![0.0; 8]),
        ];
        let (l, _, d) = build_uniform_layout(decls, []);
        let codes: Vec<&str> = d.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(
            codes,
            vec!["uniform.default-mismatch", "uniform.default-conflict"]
        );
        assert_eq!(l.frame.member("a").unwrap().default, Some(vec![1.0, 2.0]));
        assert_eq!(l.frame.member("b").unwrap().default, Some(vec![0.5]));
        assert_eq!(l.frame.member("c").unwrap().default, None);
        assert_eq!(l.frame.member("d").unwrap().default, Some(vec![1.0]));
        assert_eq!(
            l.frame.member("m").unwrap().default.as_ref().map(Vec::len),
            Some(8)
        );
    }

    #[test]
    fn frame_and_draw_blocks() {
        let mut b = LayoutBuilder::new();
        for (n, t) in [
            ("entityId", GlslType::INT),
            ("frameCounter", GlslType::INT),
            ("normalMatrix", GlslType::MAT3),
            ("alphaTestRef", GlslType::FLOAT),
        ] {
            b.add(UniformDecl::from_registry(n, t));
        }
        b.add(UniformDecl::custom("myCustom", GlslType::VEC3));
        b.add(unset("unknownThing", GlslType::FLOAT));
        let (l, idx, d) = b.build();
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(
            offsets(&l.draw),
            vec![("normalMatrix", 0), ("alphaTestRef", 48), ("entityId", 52)]
        );
        assert_eq!(l.draw.size, 64);
        assert_eq!(
            offsets(&l.frame),
            vec![("myCustom", 0), ("frameCounter", 12), ("unknownThing", 16)]
        );
        assert_eq!(
            idx.get("entityId", GlslType::INT).unwrap().block,
            Frequency::Draw
        );
        assert_eq!(
            idx.get("myCustom", GlslType::VEC3).unwrap().block,
            Frequency::Frame
        );
        assert_valid(&l);
    }

    #[test]
    fn same_name_in_both_blocks_stays_in_the_first() {
        let (l, idx, d) = build_uniform_layout(
            [unset("x", GlslType::FLOAT)],
            [unset("x", GlslType::FLOAT), unset("y", GlslType::INT)],
        );
        assert_eq!(d.0[0].code, "uniform.block-conflict");
        assert_eq!(
            idx.get("x", GlslType::FLOAT).unwrap().block,
            Frequency::Frame
        );
        assert!(l.draw.member("x").is_none());
        assert!(l.draw.member("y").is_some());
        assert_valid(&l);
    }

    #[test]
    fn invalid_declarations_are_rejected_without_panicking() {
        let bad_matrix = GlslType {
            scalar: ScalarKind::Int,
            rows: 3,
            cols: 3,
            array: None,
        };
        let bad_rows = GlslType {
            scalar: ScalarKind::Float,
            rows: 7,
            cols: 1,
            array: None,
        };
        let decls = [
            unset("", GlslType::FLOAT),
            unset("1abc", GlslType::FLOAT),
            unset("gl_Foo", GlslType::FLOAT),
            unset("a b", GlslType::FLOAT),
            unset("m", bad_matrix),
            unset("r", bad_rows),
            unset("z", GlslType::FLOAT.with_array(0)),
            unset("huge", GlslType::MAT4.with_array(u32::MAX)),
            unset("ok", GlslType::FLOAT),
        ];
        let (l, idx, d) = build_uniform_layout(decls, []);
        assert_eq!(d.errors().count(), 8);
        assert_eq!(offsets(&l.frame), vec![("ok", 0)]);
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn large_blocks_warn() {
        let decls = (0..300).map(|i| unset(&format!("m{i}"), GlslType::MAT4));
        let (l, _, d) = build_uniform_layout(decls, []);
        assert_eq!(l.frame.size, 300 * 64);
        assert!(d.iter().any(
            |d| d.code == "uniform.block-too-large" && d.severity == sb_core::Severity::Warning
        ));
        assert_valid(&l);
    }

    #[test]
    fn layout_is_independent_of_declaration_order() {
        let types = [
            "float", "vec3", "vec2", "mat4", "int", "ivec3", "bool", "vec4", "mat3", "uvec2",
            "dvec3", "double",
        ];
        let decls: Vec<UniformDecl> = (0..48)
            .map(|i| {
                unset(
                    &format!("u{i:02}"),
                    GlslType::parse(types[(i * 7) % types.len()]).unwrap(),
                )
            })
            .collect();
        let (a, _, _) = build_uniform_layout(decls.clone(), []);
        let mut rev = decls.clone();
        rev.reverse();
        let (b, _, _) = build_uniform_layout(rev, []);
        let mut rot = decls;
        rot.rotate_left(17);
        let (c, _, _) = build_uniform_layout(rot, []);
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_valid(&a);
        // Packing is tight: at most a few bytes of padding per alignment class.
        let used: u32 = a.frame.members.iter().map(|m| m.ty.std140_size()).sum();
        assert!(
            a.frame.size - used < 64,
            "size {} used {used}",
            a.frame.size
        );
    }

    #[test]
    fn validate_block_detects_problems() {
        let m = |name: &str, ty, offset| BlockMember {
            name: name.into(),
            ty,
            offset,
            source: UniformSource::Unset,
            default: None,
        };
        let bad = BlockLayout {
            name: "sb_Frame".into(),
            set: 0,
            binding: 0,
            size: 20,
            members: vec![
                m("a", GlslType::VEC4, 0),
                m("b", GlslType::VEC3, 8),
                m("a", GlslType::FLOAT, 4),
            ],
        };
        let d = validate_block(&bad);
        assert!(d.len() >= 4, "{d:?}");
        assert!(d.iter().all(|d| d.code == "uniform.invalid-layout"));
    }

    #[test]
    fn member_index_serde_roundtrip() {
        let (_, idx, _) = build_uniform_layout(
            [
                UniformDecl::from_registry("worldTime", GlslType::INT),
                unset("worldTime", GlslType::FLOAT),
            ],
            [],
        );
        let json = serde_json::to_string(&idx).unwrap();
        let back: MemberIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(back, idx);
        assert_eq!(idx.iter().count(), 2);
    }

    #[test]
    fn type_suffixes() {
        assert_eq!(type_suffix(GlslType::FLOAT), "float");
        assert_eq!(type_suffix(GlslType::parse("mat3x4").unwrap()), "mat3x4");
        assert_eq!(type_suffix(GlslType::VEC3.with_array(8)), "vec3_8");
    }
}
