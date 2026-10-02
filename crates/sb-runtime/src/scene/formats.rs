//! Vertex formats of the synthetic scene: the byte-exact layouts behind the draw profiles
//! in `crates/sb-transform/profiles/*.toml` (contract with the Java mod, which feeds the
//! same programs from Minecraft 26.3 and Distant Horizons buffers).
//!
//! Translated programs declare their vertex inputs **by name** (the profile's `inputs`),
//! so the runtime matches shader inputs to [`VertexElement::name`] and never relies on
//! locations. The element formats follow Mojang's `DefaultVertexFormat` (26.3) and DH's
//! `BlazeVertexFormatBuilder`. The terrain extension attributes (`sb_Normal`, `sb_Entity`,
//! `sb_MidTexCoord`, `sb_Tangent`, `sb_MidBlock`) are appended by the host after the
//! vanilla `BLOCK` elements, with the encodings Iris uses for the same data.

use ash::vk;

/// One attribute of a vertex format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexElement {
    /// Attribute name (equals the profile input name).
    pub name: &'static str,
    /// Vertex buffer binding the element is read from.
    pub binding: u32,
    /// Byte offset inside the binding's vertex.
    pub offset: u32,
    /// Vulkan format of the element.
    pub format: vk::Format,
}

/// A vertex buffer binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexBinding {
    /// Binding index.
    pub binding: u32,
    /// Byte stride.
    pub stride: u32,
    /// Per-instance (`true`) or per-vertex data.
    pub per_instance: bool,
}

/// A complete vertex layout (one or more bindings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexLayout {
    /// Name of the draw profile the layout feeds.
    pub profile: &'static str,
    /// Bindings.
    pub bindings: &'static [VertexBinding],
    /// Elements.
    pub elements: &'static [VertexElement],
}

impl VertexLayout {
    /// The element called `name`.
    pub fn element(&self, name: &str) -> Option<&VertexElement> {
        self.elements.iter().find(|e| e.name == name)
    }

    /// The stride of `binding`.
    pub fn stride(&self, binding: u32) -> Option<u32> {
        self.bindings.iter().find(|b| b.binding == binding).map(|b| b.stride)
    }
}

const fn el(name: &'static str, binding: u32, offset: u32, format: vk::Format) -> VertexElement {
    VertexElement { name, binding, offset, format }
}

const fn per_vertex(binding: u32, stride: u32) -> VertexBinding {
    VertexBinding { binding, stride, per_instance: false }
}

/// Bytes per terrain vertex (binding 0 of [`VANILLA_TERRAIN`]).
pub const TERRAIN_STRIDE: u32 = 52;
/// Bytes per chunk-section instance (binding 1 of [`VANILLA_TERRAIN`]).
pub const CHUNK_INSTANCE_STRIDE: u32 = 16;

/// `vanilla_terrain`: Mojang 26.3 `BLOCK` (MultiDrawIndirect path) + `CHUNK_DATA_INSTANCED`
/// + the Iris extension attributes.
///
/// | offset | element          | format               | GLSL  | meaning |
/// |-------:|------------------|----------------------|-------|---------|
/// | 0      | `Position`       | `R32G32B32_SFLOAT`   | vec3  | section-relative position |
/// | 12     | `Color`          | `R8G8B8A8_UNORM`     | vec4  | biome tint × AO |
/// | 16     | `UV0`            | `R32G32_SFLOAT`      | vec2  | atlas coordinates |
/// | 24     | `UV2`            | `R16G16_SINT`        | ivec2 | lightmap, 0..240 (x = block, y = sky) |
/// | 28     | `sb_Normal`      | `R8G8B8A8_SNORM`     | vec3  | face normal (w unused) |
/// | 32     | `sb_Entity`      | `R16G16_SINT`        | ivec2 | `block.properties` id (-1 if unmapped), render type |
/// | 36     | `sb_MidTexCoord` | `R32G32_SFLOAT`      | vec2  | sprite centre |
/// | 44     | `sb_Tangent`     | `R8G8B8A8_SNORM`     | vec4  | tangent, w = handedness |
/// | 48     | `sb_MidBlock`    | `R8G8B8A8_SINT`      | ivec4 | (block centre − vertex) × 64, w = light emission |
///
/// Binding 1 (per instance, 16 bytes): `ChunkPosition` `R32G32B32_SINT` at 0 (section
/// origin in blocks), `ChunkVisibility` `R32_SFLOAT` at 12 (fade-in, 1 = fully visible).
pub const VANILLA_TERRAIN: VertexLayout = VertexLayout {
    profile: "vanilla_terrain",
    bindings: &[per_vertex(0, TERRAIN_STRIDE), VertexBinding { binding: 1, stride: CHUNK_INSTANCE_STRIDE, per_instance: true }],
    elements: &[
        el("Position", 0, 0, vk::Format::R32G32B32_SFLOAT),
        el("Color", 0, 12, vk::Format::R8G8B8A8_UNORM),
        el("UV0", 0, 16, vk::Format::R32G32_SFLOAT),
        el("UV2", 0, 24, vk::Format::R16G16_SINT),
        el("sb_Normal", 0, 28, vk::Format::R8G8B8A8_SNORM),
        el("sb_Entity", 0, 32, vk::Format::R16G16_SINT),
        el("sb_MidTexCoord", 0, 36, vk::Format::R32G32_SFLOAT),
        el("sb_Tangent", 0, 44, vk::Format::R8G8B8A8_SNORM),
        el("sb_MidBlock", 0, 48, vk::Format::R8G8B8A8_SINT),
        el("ChunkPosition", 1, 0, vk::Format::R32G32B32_SINT),
        el("ChunkVisibility", 1, 12, vk::Format::R32_SFLOAT),
    ],
};

/// Bytes per entity vertex.
pub const ENTITY_STRIDE: u32 = 36;

/// `vanilla_entity`: Mojang 26.3 `ENTITY` — `Position` vec3 (0), `Color` RGBA8 (12), `UV0`
/// vec2 (16), `UV1` overlay `R16G16_SINT` (24), `UV2` lightmap `R16G16_SINT` (28), `Normal`
/// `R8G8B8A8_SNORM` (32).
pub const VANILLA_ENTITY: VertexLayout = VertexLayout {
    profile: "vanilla_entity",
    bindings: &[per_vertex(0, ENTITY_STRIDE)],
    elements: &[
        el("Position", 0, 0, vk::Format::R32G32B32_SFLOAT),
        el("Color", 0, 12, vk::Format::R8G8B8A8_UNORM),
        el("UV0", 0, 16, vk::Format::R32G32_SFLOAT),
        el("UV1", 0, 24, vk::Format::R16G16_SINT),
        el("UV2", 0, 28, vk::Format::R16G16_SINT),
        el("Normal", 0, 32, vk::Format::R8G8B8A8_SNORM),
    ],
};

/// `vanilla_position`: Mojang `POSITION` (sky disc), 12 bytes.
pub const VANILLA_POSITION: VertexLayout = VertexLayout {
    profile: "vanilla_position",
    bindings: &[per_vertex(0, 12)],
    elements: &[el("Position", 0, 0, vk::Format::R32G32B32_SFLOAT)],
};

/// `vanilla_position_tex`: Mojang `POSITION_TEX` (sun, moon), 20 bytes.
pub const VANILLA_POSITION_TEX: VertexLayout = VertexLayout {
    profile: "vanilla_position_tex",
    bindings: &[per_vertex(0, 20)],
    elements: &[el("Position", 0, 0, vk::Format::R32G32B32_SFLOAT), el("UV0", 0, 12, vk::Format::R32G32_SFLOAT)],
};

/// Bytes per DH LOD vertex.
pub const DH_STRIDE: u32 = 16;

/// `dh_terrain`: Distant Horizons 3.3 `BLAZE_3D` LOD vertex, 16 bytes, native byte order
/// (little-endian here):
///
/// | offset | element        | format              | meaning |
/// |-------:|----------------|---------------------|---------|
/// | 0      | `vPosition`    | `R16G16B16_UINT`    | block position relative to the buffer's minimum corner |
/// | 6      | `meta`         | `R16_UINT`          | bits 0-3 sky light, 4-7 block light, 8-13 micro offset `0b00zzyyxx` |
/// | 8      | `vColor`       | `R8G8B8A8_UNORM`    | colour |
/// | 12     | `irisMaterial` | `R8_UINT`           | `DH_BLOCK_*` material |
/// | 13     | `irisNormal`   | `R8_UINT`           | face: 0 down, 1 up, 2 north, 3 south, 4 west, 5 east |
/// | 14     | `textureTile`  | `R16_UINT`          | DH atlas tile (0 = flat colour) |
pub const DH_TERRAIN: VertexLayout = VertexLayout {
    profile: "dh_terrain",
    bindings: &[per_vertex(0, DH_STRIDE)],
    elements: &[
        el("vPosition", 0, 0, vk::Format::R16G16B16_UINT),
        el("meta", 0, 6, vk::Format::R16_UINT),
        el("vColor", 0, 8, vk::Format::R8G8B8A8_UNORM),
        el("irisMaterial", 0, 12, vk::Format::R8_UINT),
        el("irisNormal", 0, 13, vk::Format::R8_UINT),
        el("textureTile", 0, 14, vk::Format::R16_UINT),
    ],
};

/// Every layout of the scene.
pub const ALL_LAYOUTS: [&VertexLayout; 5] = [&VANILLA_TERRAIN, &VANILLA_ENTITY, &VANILLA_POSITION, &VANILLA_POSITION_TEX, &DH_TERRAIN];

/// Byte size of a vertex element format (0 for unknown formats).
pub fn format_size(format: vk::Format) -> u32 {
    match format {
        vk::Format::R8_UINT => 1,
        vk::Format::R16_UINT => 2,
        vk::Format::R16G16_SINT
        | vk::Format::R8G8B8A8_UNORM
        | vk::Format::R8G8B8A8_SNORM
        | vk::Format::R8G8B8A8_SINT
        | vk::Format::R32_SFLOAT => 4,
        vk::Format::R16G16B16_UINT => 6,
        vk::Format::R32G32_SFLOAT | vk::Format::R16G16B16A16_UINT => 8,
        vk::Format::R32G32B32_SFLOAT | vk::Format::R32G32B32_SINT => 12,
        vk::Format::R32G32B32A32_SFLOAT | vk::Format::R32G32B32A32_SINT | vk::Format::R32G32B32A32_UINT => 16,
        _ => 0,
    }
}

/// Little-endian vertex writer.
#[derive(Debug, Default, Clone)]
pub(crate) struct VertexWriter {
    pub bytes: Vec<u8>,
}

impl VertexWriter {
    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn f32s(&mut self, v: &[f32]) -> &mut Self {
        for x in v {
            self.f32(*x);
        }
        self
    }
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i16(&mut self, v: i16) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u8s(&mut self, v: [u8; 4]) -> &mut Self {
        self.bytes.extend_from_slice(&v);
        self
    }
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.bytes.push(v);
        self
    }
    /// Signed-normalized byte vector (`R8G8B8A8_SNORM`).
    pub fn snorm4(&mut self, v: [f32; 4]) -> &mut Self {
        self.u8s(v.map(|x| (x.clamp(-1.0, 1.0) * 127.0).round() as i8 as u8))
    }
    /// Unsigned-normalized byte colour (`R8G8B8A8_UNORM`).
    pub fn unorm4(&mut self, v: [f32; 4]) -> &mut Self {
        self.u8s(v.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_are_packed_without_overlap() {
        for layout in ALL_LAYOUTS {
            for b in layout.bindings {
                let mut elems: Vec<_> = layout.elements.iter().filter(|e| e.binding == b.binding).collect();
                elems.sort_by_key(|e| e.offset);
                let mut end = 0;
                for e in elems {
                    assert!(e.offset >= end, "{}: {} overlaps", layout.profile, e.name);
                    let size = format_size(e.format);
                    assert!(size > 0, "{}: {} has unknown size", layout.profile, e.name);
                    end = e.offset + size;
                }
                assert_eq!(end, b.stride, "{} binding {} is not tightly packed", layout.profile, b.binding);
            }
        }
    }

    #[test]
    fn dh_layout_offsets() {
        let offs: Vec<(&str, u32)> = DH_TERRAIN.elements.iter().map(|e| (e.name, e.offset)).collect();
        assert_eq!(offs, [("vPosition", 0), ("meta", 6), ("vColor", 8), ("irisMaterial", 12), ("irisNormal", 13), ("textureTile", 14)]);
        assert_eq!(DH_TERRAIN.stride(0), Some(16));
    }

    #[test]
    fn layouts_cover_profile_inputs() {
        // Input names of the profiles in crates/sb-transform/profiles.
        let terrain = ["Position", "Color", "UV0", "UV2", "ChunkPosition", "ChunkVisibility", "sb_Normal", "sb_Entity", "sb_MidTexCoord", "sb_Tangent", "sb_MidBlock"];
        for n in terrain {
            assert!(VANILLA_TERRAIN.element(n).is_some(), "{n}");
        }
        for n in ["Position", "Color", "UV0", "UV1", "UV2", "Normal"] {
            assert!(VANILLA_ENTITY.element(n).is_some(), "{n}");
        }
        for n in ["vPosition", "meta", "vColor", "irisMaterial", "irisNormal", "textureTile"] {
            assert!(DH_TERRAIN.element(n).is_some(), "{n}");
        }
        assert!(VANILLA_POSITION_TEX.element("UV0").is_some());
    }

    #[test]
    fn writer_encodings() {
        let mut w = VertexWriter::default();
        w.snorm4([1.0, -1.0, 0.0, 0.5]).unorm4([1.0, 0.0, 0.5, 2.0]).u16(0x1234).i16(-2);
        assert_eq!(w.bytes, vec![127, 0x81, 0, 64, 255, 0, 128, 255, 0x34, 0x12, 0xfe, 0xff]);
    }
}
