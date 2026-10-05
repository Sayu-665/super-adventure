//! Vanilla terrain: chunk sections of a voxel heightmap world in the `vanilla_terrain`
//! vertex layout, split into solid, cutout (leaves) and translucent (water) buffers.

use super::formats::{CHUNK_INSTANCE_STRIDE, SODIUM_STRIDE, SODIUM_TERRAIN, TERRAIN_STRIDE, VANILLA_TERRAIN, VertexWriter};
use super::world::World;
use super::{Block, BlockIds, CpuMesh};
use crate::textures::Tile;
use std::collections::BTreeMap;

/// Height of a water source block's surface.
pub(crate) const WATER_SURFACE: f32 = 14.0 / 16.0;

/// The terrain buffers. Every mesh has one sub-draw per chunk section whose
/// `first_instance` selects the section's `ChunkPosition` in `instances`.
#[derive(Debug, Clone)]
pub(crate) struct TerrainMeshes {
    pub solid: CpuMesh,
    pub cutout: CpuMesh,
    pub water: CpuMesh,
    /// Per-section instance data ([`CHUNK_INSTANCE_STRIDE`] bytes each).
    pub instances: Vec<u8>,
    /// Section origins in blocks, in instance order.
    pub sections: Vec<[i32; 3]>,
    /// The same terrain in Sodium's format, when requested.
    pub sodium: Option<SodiumMeshes>,
}

/// Sections per Sodium render region along x, y and z.
pub(crate) const SODIUM_REGION_SECTIONS: [i32; 3] = [8, 4, 8];

/// A Sodium render region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SodiumRegion {
    /// Minimum corner in blocks.
    pub origin: [i32; 3],
    /// `u_RegionID`.
    pub id: u32,
}

/// The terrain in the `sodium_terrain` layout: per layer (solid, cutout, translucent)
/// one sub-draw per region, positions relative to their section, sections indexed in
/// their region.
#[derive(Debug, Clone)]
pub(crate) struct SodiumMeshes {
    pub layers: [CpuMesh; 3],
    /// Region (index into `regions`) of each sub-draw of each layer.
    pub draw_regions: [Vec<usize>; 3],
    pub regions: Vec<SodiumRegion>,
}

/// One vertex in Sodium's compact format plus the extension attributes (see
/// [`SODIUM_TERRAIN`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SodiumVertex {
    /// Section-local position (`-8..24` representable).
    pub pos: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
    /// Centre of the quad's UVs (the sign bit nudges towards it).
    pub uv_centre: [f32; 2],
    /// Block and sky light, `16 L` (0..240).
    pub light: [u8; 2],
    pub material: u8,
    /// Section index in its region: `x << 5 | z << 2 | y`.
    pub section: u8,
    /// `block.properties` id, `-1` when unmapped.
    pub block_id: i32,
    pub fluid: bool,
    pub normal: [f32; 3],
    pub mid_uv: [f32; 2],
    /// (block centre − vertex) × 64.
    pub mid_block: [i8; 3],
    pub emission: u8,
}

/// Sodium's 20-bit position quantization: `(p + 8) / 32 * 2^20`.
fn sodium_quantize(p: f32) -> u32 {
    ((f64::from(p) + 8.0) / 32.0 * f64::from(1u32 << 20)).round().clamp(0.0, f64::from(0xF_FFFF)) as u32
}

/// A texture coordinate as Sodium's `CompactChunkVertex.encodeTexture` writes it: quantized
/// to 15 bits (`Math.round(c * 32768)`), moved one unit towards the quad centre, and the
/// direction of that move in bit 15 (set when it moved down, i.e. the coordinate is at or
/// past the centre). The shader undoes the move with `u_TexCoordShrink`.
fn sodium_uv(c: f32, centre: f32) -> u16 {
    let bias: i32 = if c < centre { 1 } else { -1 };
    // Java's Math.round(float): floor(x + 0.5).
    let quantized = (c * 32768.0 + 0.5).floor() as i32 + bias;
    (quantized & 0x7FFF) as u16 | (u16::from(bias < 0) << 15)
}

/// Pack one vertex in the [`SODIUM_TERRAIN`] layout.
pub(crate) fn pack_sodium_vertex(w: &mut VertexWriter, v: &SodiumVertex) {
    let q = v.pos.map(sodium_quantize);
    let hi = (q[0] >> 10 & 0x3FF) | (q[1] >> 10 & 0x3FF) << 10 | (q[2] >> 10 & 0x3FF) << 20;
    let lo = (q[0] & 0x3FF) | (q[1] & 0x3FF) << 10 | (q[2] & 0x3FF) << 20;
    let light = v.light.map(|l| l.saturating_add(8).clamp(8, 248));
    let entity = ((v.block_id.max(-1) + 1) as u32) << 1 | u32::from(v.fluid);
    let mid = v.mid_uv.map(|c| (f64::from(c) * 32768.0).round().clamp(0.0, 65535.0) as u16);
    w.u32(hi)
        .u32(lo)
        .unorm4(v.color)
        .u16(sodium_uv(v.uv[0], v.uv_centre[0]))
        .u16(sodium_uv(v.uv[1], v.uv_centre[1]))
        .u8s([light[0], light[1], v.material, v.section])
        .u32(entity)
        .snorm4([v.normal[0], v.normal[1], v.normal[2], 0.0])
        .u16(mid[0])
        .u16(mid[1])
        .u8s([v.mid_block[0] as u8, v.mid_block[1] as u8, v.mid_block[2] as u8, v.emission]);
}

/// Sodium material bits of a layer: bit 0 = mipmapped, bits 1-2 = alpha cutoff index into
/// `{0, 0.1, 0.5, 1.0}`.
fn sodium_material(layer: Layer) -> u8 {
    match layer {
        Layer::Solid => 1,
        Layer::Cutout => 1 | 2 << 1,
        Layer::Translucent => 1 | 1 << 1,
    }
}

/// Region (in region units) and section index of a section.
fn sodium_region_of(sec: [i32; 3]) -> ([i32; 3], u8) {
    let r = [0, 1, 2].map(|i| sec[i].div_euclid(SODIUM_REGION_SECTIONS[i]));
    let l = [0, 1, 2].map(|i| sec[i] - r[i] * SODIUM_REGION_SECTIONS[i]);
    (r, ((l[0] << 5) | (l[2] << 2) | l[1]) as u8)
}

/// Cube faces in DH direction order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Face {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Face {
    pub const SIDES: [Face; 4] = [Face::North, Face::South, Face::West, Face::East];
    pub const ALL: [Face; 6] = [Face::Down, Face::Up, Face::North, Face::South, Face::West, Face::East];

    /// Outward normal.
    pub fn normal(self) -> [f32; 3] {
        match self {
            Face::Down => [0.0, -1.0, 0.0],
            Face::Up => [0.0, 1.0, 0.0],
            Face::North => [0.0, 0.0, -1.0],
            Face::South => [0.0, 0.0, 1.0],
            Face::West => [-1.0, 0.0, 0.0],
            Face::East => [1.0, 0.0, 0.0],
        }
    }

    /// Horizontal neighbour offset of a side face.
    pub fn offset(self) -> [i32; 3] {
        let n = self.normal();
        [n[0] as i32, n[1] as i32, n[2] as i32]
    }

    /// Tangent (direction of increasing texture `s`), w = handedness.
    pub fn tangent(self) -> [f32; 4] {
        match self {
            Face::West | Face::East => [0.0, 0.0, 1.0, 1.0],
            _ => [1.0, 0.0, 0.0, 1.0],
        }
    }

    /// The four corners of the face of the unit cube, counter-clockwise seen from
    /// outside (GL front faces).
    pub fn corners(self) -> [[f32; 3]; 4] {
        match self {
            Face::Up => [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]],
            Face::Down => [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
            Face::East => [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]],
            Face::West => [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]],
            Face::South => [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]],
            Face::North => [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
        }
    }

    /// Texture coordinates (0..1 inside the sprite) of a corner.
    pub fn sprite_uv(self, c: [f32; 3]) -> [f32; 2] {
        match self {
            Face::Up | Face::Down => [c[0], c[2]],
            Face::West | Face::East => [c[2], 1.0 - c[1]],
            Face::North | Face::South => [c[0], 1.0 - c[1]],
        }
    }

    /// DH `irisNormal` index.
    pub fn dh_index(self) -> u8 {
        self as u8
    }
}

/// Atlas tile of a block face.
pub(crate) fn face_tile(block: Block, face: Face) -> Tile {
    match (block, face) {
        (Block::GrassBlock, Face::Up) => Tile::GrassTop,
        (Block::GrassBlock, Face::Down) => Tile::Dirt,
        (Block::GrassBlock, _) => Tile::GrassSide,
        (Block::Dirt, _) => Tile::Dirt,
        (Block::Stone, _) => Tile::Stone,
        (Block::Sand, _) => Tile::Sand,
        (Block::Water, _) => Tile::Water,
        (Block::Leaves, _) => Tile::Leaves,
        (Block::Log, Face::Up | Face::Down) => Tile::LogTop,
        (Block::Log, _) => Tile::LogSide,
        (Block::Planks, _) => Tile::Planks,
    }
}

/// Biome tint of a block face (plains colours).
pub(crate) fn face_tint(block: Block, face: Face) -> [f32; 3] {
    match (block, face) {
        (Block::GrassBlock, Face::Up) => [0.57, 0.74, 0.35],
        (Block::Leaves, _) => [0.47, 0.65, 0.25],
        (Block::Water, _) => [0.25, 0.46, 0.89],
        _ => [1.0, 1.0, 1.0],
    }
}

/// Which buffer a block's faces go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    Solid = 0,
    Cutout = 1,
    Translucent = 2,
}

/// One face to emit.
struct FaceDesc {
    block: Block,
    face: Face,
    /// World block position.
    pos: [i32; 3],
    /// Per-corner ambient occlusion factor.
    ao: [f32; 4],
    /// Height of the block (1 for full blocks, [`WATER_SURFACE`] for water).
    top: f32,
}

struct Builder<'a> {
    ids: &'a BlockIds,
    sections: BTreeMap<[i32; 3], [Vec<u8>; 3]>,
    /// Sodium vertices per region (region units) and layer, when requested.
    sodium: Option<BTreeMap<[i32; 3], [Vec<u8>; 3]>>,
}

impl Builder<'_> {
    fn emit(&mut self, layer: Layer, f: &FaceDesc) {
        let sec = [f.pos[0].div_euclid(16), f.pos[1].div_euclid(16), f.pos[2].div_euclid(16)];
        let local = [f.pos[0] - sec[0] * 16, f.pos[1] - sec[1] * 16, f.pos[2] - sec[2] * 16].map(|v| v as f32);
        let tile = face_tile(f.block, f.face);
        let (u0, v0, u1, v1) = tile.uv_rect();
        let mid = [(u0 + u1) * 0.5, (v0 + v1) * 0.5];
        let tint = face_tint(f.block, f.face);
        let normal = f.face.normal();
        let block_id = self.ids.get(f.block);
        // Iris' Sodium path: `mc_Entity.y` is 1 for fluids and 0 for other blocks.
        let fluid = f.block == Block::Water;
        let (region, section) = sodium_region_of(sec);
        let mut w = VertexWriter::default();
        let mut sw = VertexWriter::default();
        for (i, c) in f.face.corners().into_iter().enumerate() {
            let c = [c[0], c[1] * f.top, c[2]];
            let p = [local[0] + c[0], local[1] + c[1], local[2] + c[2]];
            let [s, t] = f.face.sprite_uv(c);
            let ao = f.ao[i];
            let uv = [u0 + (u1 - u0) * s, v0 + (v1 - v0) * t];
            let color = [tint[0] * ao, tint[1] * ao, tint[2] * ao, 1.0];
            let mid_block = [(0.5 - c[0]) * 64.0, (0.5 - c[1]) * 64.0, (0.5 - c[2]) * 64.0].map(|v| v.round() as i8);
            w.f32s(&p)
                .unorm4(color)
                .f32s(&uv)
                .i16(0)
                .i16(240)
                .snorm4([normal[0], normal[1], normal[2], 0.0])
                .i16(block_id.clamp(i16::MIN.into(), i16::MAX.into()) as i16)
                .i16(i16::from(fluid))
                .f32s(&mid)
                .snorm4(f.face.tangent())
                .u8s([mid_block[0] as u8, mid_block[1] as u8, mid_block[2] as u8, f.block.emission()]);
            if self.sodium.is_some() {
                pack_sodium_vertex(
                    &mut sw,
                    &SodiumVertex {
                        pos: p,
                        color,
                        uv,
                        uv_centre: mid,
                        light: [0, 240],
                        material: sodium_material(layer),
                        section,
                        block_id,
                        fluid,
                        normal,
                        mid_uv: mid,
                        mid_block,
                        emission: f.block.emission(),
                    },
                );
            }
        }
        debug_assert_eq!(w.bytes.len(), 4 * TERRAIN_STRIDE as usize);
        self.sections.entry(sec).or_default()[layer as usize].extend_from_slice(&w.bytes);
        if let Some(regions) = self.sodium.as_mut() {
            debug_assert_eq!(sw.bytes.len(), 4 * SODIUM_STRIDE as usize);
            regions.entry(region).or_default()[layer as usize].extend_from_slice(&sw.bytes);
        }
    }
}

/// Block of column `(x, z)` with surface height `h` at level `y`.
fn column_block(world: &World, h: i32, y: i32) -> Block {
    let beach = world.is_beach(h);
    if y == h {
        if beach { Block::Sand } else { Block::GrassBlock }
    } else if y >= h - 3 {
        if beach { Block::Sand } else { Block::Dirt }
    } else {
        Block::Stone
    }
}

/// Vertex AO of an up face at height `h`: each corner darkens with the number of
/// higher neighbour columns touching it (Minecraft's smooth-lighting rule).
fn top_ao(world: &World, x: i32, z: i32, h: i32) -> [f32; 4] {
    let higher = |dx: i32, dz: i32| world.height(x + dx, z + dz) > h;
    let level = |dx: i32, dz: i32| {
        let s1 = higher(dx, 0);
        let s2 = higher(0, dz);
        if s1 && s2 {
            0
        } else {
            3 - (u8::from(s1) + u8::from(s2) + u8::from(higher(dx, dz)))
        }
    };
    let f = |l: u8| [0.5, 0.65, 0.8, 1.0][usize::from(l)];
    // Corner order of Face::Up: (0,0), (0,1), (1,1), (1,0) in (x, z).
    [f(level(-1, -1)), f(level(-1, 1)), f(level(1, 1)), f(level(1, -1))]
}

/// Generate the terrain within `rd` chunks of `cam_chunk` (also in Sodium's format with
/// `sodium`).
pub(crate) fn build(world: &World, ids: &BlockIds, cam_chunk: [i32; 2], rd: i32, sodium: bool) -> TerrainMeshes {
    let mut b = Builder { ids, sections: BTreeMap::new(), sodium: sodium.then(BTreeMap::new) };
    let sea = world.sea_level();
    let x0 = (cam_chunk[0] - rd) * 16;
    let x1 = (cam_chunk[0] + rd + 1) * 16;
    let z0 = (cam_chunk[1] - rd) * 16;
    let z1 = (cam_chunk[1] + rd + 1) * 16;
    for x in x0..x1 {
        for z in z0..z1 {
            let h = world.height(x, z);
            let top = column_block(world, h, h);
            b.emit(Layer::Solid, &FaceDesc { block: top, face: Face::Up, pos: [x, h, z], ao: top_ao(world, x, z, h), top: 1.0 });
            for face in Face::SIDES {
                let o = face.offset();
                let hn = world.height(x + o[0], z + o[2]);
                // Sides below the water surface of a neighbouring water column are still
                // emitted (visible through the water).
                for y in (hn + 1).max(h - 24)..=h {
                    let block = column_block(world, h, y);
                    b.emit(Layer::Solid, &FaceDesc { block, face, pos: [x, y, z], ao: [1.0; 4], top: 1.0 });
                }
            }
            if h < sea {
                b.emit(Layer::Translucent, &FaceDesc { block: Block::Water, face: Face::Up, pos: [x, sea, z], ao: [1.0; 4], top: WATER_SURFACE });
            }
            if world.tree_at(x, z) {
                tree(&mut b, x, h, z);
            }
        }
    }
    let sodium = b.sodium.take().map(|regions| {
        let mut out = SodiumMeshes {
            layers: [CpuMesh::new(&SODIUM_TERRAIN), CpuMesh::new(&SODIUM_TERRAIN), CpuMesh::new(&SODIUM_TERRAIN)],
            draw_regions: Default::default(),
            regions: Vec::new(),
        };
        for (region, layers) in regions {
            let index = out.regions.len();
            let origin = [0, 1, 2].map(|i| region[i] * SODIUM_REGION_SECTIONS[i] * 16);
            out.regions.push(SodiumRegion { origin, id: index as u32 });
            for (k, bytes) in layers.iter().enumerate() {
                let before = out.layers[k].draws.len();
                out.layers[k].push_draw(bytes, 0);
                if out.layers[k].draws.len() > before {
                    out.draw_regions[k].push(index);
                }
            }
        }
        out
    });
    let mut meshes = TerrainMeshes {
        solid: CpuMesh::new(&VANILLA_TERRAIN),
        cutout: CpuMesh::new(&VANILLA_TERRAIN),
        water: CpuMesh::new(&VANILLA_TERRAIN),
        instances: Vec::new(),
        sections: Vec::new(),
        sodium,
    };
    for (sec, layers) in b.sections {
        let instance = meshes.sections.len() as u32;
        let origin = [sec[0] * 16, sec[1] * 16, sec[2] * 16];
        let mut w = VertexWriter::default();
        w.i32(origin[0]).i32(origin[1]).i32(origin[2]).f32(1.0);
        debug_assert_eq!(w.bytes.len(), CHUNK_INSTANCE_STRIDE as usize);
        meshes.instances.extend_from_slice(&w.bytes);
        meshes.sections.push(origin);
        let [solid, cutout, water] = layers;
        meshes.solid.push_draw(&solid, instance);
        meshes.cutout.push_draw(&cutout, instance);
        meshes.water.push_draw(&water, instance);
    }
    meshes
}

/// An oak tree on the column `(x, z)` whose ground is at `h`.
fn tree(b: &mut Builder<'_>, x: i32, h: i32, z: i32) {
    let trunk_top = h + 5;
    for y in h + 1..=trunk_top {
        for face in Face::SIDES {
            b.emit(Layer::Solid, &FaceDesc { block: Block::Log, face, pos: [x, y, z], ao: [1.0; 4], top: 1.0 });
        }
    }
    let mut leaves = Vec::new();
    for y in trunk_top - 2..=trunk_top + 1 {
        let r: i32 = if y >= trunk_top { 1 } else { 2 };
        for dx in -r..=r {
            for dz in -r..=r {
                let corner = dx.abs() == r && dz.abs() == r;
                if (corner && r == 2) || (dx == 0 && dz == 0 && y <= trunk_top) {
                    continue;
                }
                leaves.push([x + dx, y, z + dz]);
            }
        }
    }
    let set: std::collections::HashSet<[i32; 3]> = leaves.iter().copied().collect();
    for p in &leaves {
        for face in Face::ALL {
            let o = face.offset();
            if set.contains(&[p[0] + o[0], p[1] + o[1], p[2] + o[2]]) {
                continue;
            }
            b.emit(Layer::Cutout, &FaceDesc { block: Block::Leaves, face, pos: *p, ao: [1.0; 4], top: 1.0 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_core::model::IdMaps;

    fn vec3(b: &[u8]) -> [f32; 3] {
        [0, 4, 8].map(|o| f32::from_le_bytes(b[o..o + 4].try_into().unwrap()))
    }

    #[test]
    fn faces_are_counter_clockwise_from_outside() {
        for face in Face::ALL {
            let c = face.corners();
            let e1 = [c[1][0] - c[0][0], c[1][1] - c[0][1], c[1][2] - c[0][2]];
            let e2 = [c[2][0] - c[0][0], c[2][1] - c[0][1], c[2][2] - c[0][2]];
            let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            assert_eq!(n, face.normal(), "{face:?}");
        }
        assert_eq!(Face::East.dh_index(), 5);
        assert_eq!(Face::North.dh_index(), 2);
    }

    /// Byte-exact Sodium compact vertex + extension attributes (`sodium_terrain`).
    #[test]
    fn sodium_vertex_byte_layout() {
        let mut w = VertexWriter::default();
        let v = SodiumVertex {
            pos: [1.0, 16.0, 0.5],
            color: [1.0, 0.5, 0.0, 1.0],
            uv: [0.25, 0.75],
            uv_centre: [0.5, 0.5],
            light: [32, 240],
            material: 5,
            section: (3 << 5) | (6 << 2) | 2,
            block_id: 41,
            fluid: true,
            normal: [0.0, -1.0, 0.0],
            mid_uv: [0.5, 0.125],
            mid_block: [32, -32, 0],
            emission: 7,
        };
        pack_sodium_vertex(&mut w, &v);
        assert_eq!(w.bytes.len(), SODIUM_STRIDE as usize);
        let u32_at = |o: usize| u32::from_le_bytes(w.bytes[o..o + 4].try_into().unwrap());
        let u16_at = |o: usize| u16::from_le_bytes([w.bytes[o], w.bytes[o + 1]]);
        // 20-bit quantization (p + 8) * 32768, interleaved hi/lo 10-bit halves.
        let q = [9.0f64, 24.0, 8.5].map(|p| (p * 32768.0) as u32);
        let hi = (q[0] >> 10) | (q[1] >> 10) << 10 | (q[2] >> 10) << 20;
        let lo = (q[0] & 0x3FF) | (q[1] & 0x3FF) << 10 | (q[2] & 0x3FF) << 20;
        assert_eq!((u32_at(0), u32_at(4)), (hi, lo));
        // Decoding as the profile does: ((hi << 10) | lo) * 32 / 2^20 - 8.
        let dec = |shift: u32| f64::from(((u32_at(0) >> shift & 0x3FF) << 10) | (u32_at(4) >> shift & 0x3FF)) * 32.0 / f64::from(1u32 << 20) - 8.0;
        assert_eq!([dec(0), dec(10), dec(20)], [1.0, 16.0, 0.5]);
        assert_eq!(&w.bytes[8..12], &[255, 128, 0, 255]);
        // UV as Sodium's encodeTexture: round(c * 32768) moved one unit towards the centre,
        // bit 15 set when it moved down (the coordinate is at or past the centre).
        assert_eq!(u16_at(12), 8193);
        assert_eq!(u16_at(14), 24575 | 0x8000);
        // Light 16 L + 8, material, section.
        assert_eq!(&w.bytes[16..20], &[40, 248, 5, (3 << 5) | (6 << 2) | 2]);
        assert_eq!(u32_at(20), (42 << 1) | 1);
        assert_eq!(&w.bytes[24..28], &[0, 0x81, 0, 0]);
        assert_eq!((u16_at(28), u16_at(30)), (16384, 4096));
        assert_eq!(&w.bytes[32..36], &[32, (-32i8) as u8, 0, 7]);
        // Unmapped blocks encode id -1 as 0.
        let mut w2 = VertexWriter::default();
        pack_sodium_vertex(&mut w2, &SodiumVertex { block_id: -1, fluid: false, ..v });
        assert_eq!(u32::from_le_bytes(w2.bytes[20..24].try_into().unwrap()), 0);
        // Region of a section: 8x4x8 sections, index x << 5 | z << 2 | y.
        assert_eq!(sodium_region_of([-1, 5, 17]), ([-1, 1, 2], ((7 << 5) | (1 << 2) | 1) as u8));
    }

    #[test]
    fn terrain_vertex_packing() {
        let mut maps = IdMaps::default();
        maps.blocks.insert(2, vec!["minecraft:grass_block".into(), "minecraft:sand".into()]);
        maps.blocks.insert(8, vec!["minecraft:water".into()]);
        let ids = BlockIds::from_id_maps(&maps);
        let world = World::new(3);
        let m = build(&world, &ids, [0, 0], 0, false);
        assert!(!m.solid.is_empty());
        assert_eq!(m.solid.vertices.len() % TERRAIN_STRIDE as usize, 0);
        assert_eq!(m.instances.len(), m.sections.len() * CHUNK_INSTANCE_STRIDE as usize);
        // Every vertex is section-relative and every draw's instance points at its section.
        for d in &m.solid.draws {
            let origin = m.sections[d.first_instance as usize];
            let inst = &m.instances[d.first_instance as usize * 16..][..16];
            assert_eq!(i32::from_le_bytes(inst[0..4].try_into().unwrap()), origin[0]);
            assert_eq!(f32::from_le_bytes(inst[12..16].try_into().unwrap()), 1.0);
            let first = d.vertex_offset as usize * TERRAIN_STRIDE as usize;
            let p = vec3(&m.solid.vertices[first..]);
            assert!(p.iter().all(|v| (0.0..=16.0).contains(v)), "{p:?}");
        }
        // The first vertex of the first draw: lightmap (0, 240), normal, mapped block id.
        let v = &m.solid.vertices[..TERRAIN_STRIDE as usize];
        assert_eq!(i16::from_le_bytes([v[24], v[25]]), 0);
        assert_eq!(i16::from_le_bytes([v[26], v[27]]), 240);
        let id = i16::from_le_bytes([v[32], v[33]]);
        assert!(id == 2 || id == -1, "{id}");
        assert_eq!(v[31], 0); // normal.w
        // Water faces carry the water id and render type 1.
        if !m.water.is_empty() {
            let v = &m.water.vertices[..TERRAIN_STRIDE as usize];
            assert_eq!(i16::from_le_bytes([v[32], v[33]]), 8);
            assert_eq!(i16::from_le_bytes([v[34], v[35]]), 1);
        }
    }
}
