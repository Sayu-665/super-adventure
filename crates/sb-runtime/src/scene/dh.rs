//! Distant Horizons LOD terrain: the world heightmap at 4x coarser resolution out to the
//! DH distance (see [`LodCoverage`] for the area under the vanilla chunks), in DH's
//! 16-byte vertex layout (one buffer per 128x128-block region, as DH keeps one buffer per
//! LOD container with its own `uModelOffset`).

use super::formats::{DH_STRIDE, DH_TERRAIN, VertexWriter};
use super::terrain::Face;
use super::world::World;
use super::CpuMesh;
use crate::textures::DhTile;

/// LOD cell size in blocks.
pub(crate) const CELL: i32 = 4;
/// Region size in blocks.
pub(crate) const REGION: i32 = 128;
/// Y of the buffers' minimum corner (`vPosition.y` is relative to it).
pub(crate) const REGION_MIN_Y: i32 = -64;

/// DH `EDhApiBlockMaterial` ids (`DH_BLOCK_*`) used by the scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(dead_code)] // the complete set of materials the LOD palette knows
pub(crate) enum DhMaterial {
    Leaves = 1,
    Stone = 2,
    Dirt = 5,
    Sand = 9,
    Water = 12,
    Grass = 13,
}

/// Which cells of the DH square get LODs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LodCoverage {
    /// Every cell, the vanilla area included. DH keeps LODs for all loaded terrain and,
    /// while an Iris shader pack is active, only clips them at a near plane of 20 % of the
    /// vanilla render distance ("overdraw prevention" 0.2, `RenderUtil.
    /// getNearClipPlaneInBlocks`), so LODs overlap the vanilla chunks and packs hide them
    /// with their own distance discards (`dh_terrain` and `gbuffers_terrain` fade at about
    /// `far`). Without the overlap those discards leave holes at the transition.
    Full,
    /// Only the ring outside the vanilla area: LODs that share the vanilla depth buffer and
    /// projection (synthesized DH programs) would poke through vanilla terrain.
    Ring,
}

/// One LOD region.
#[derive(Debug, Clone)]
pub(crate) struct DhRegion {
    /// Absolute minimum corner in blocks (`vertUniqueUniformBlock.uModelOffset`).
    pub origin: [i32; 3],
    pub opaque: CpuMesh,
    pub water: CpuMesh,
}

/// All LOD regions.
#[derive(Debug, Clone, Default)]
pub(crate) struct DhMeshes {
    pub regions: Vec<DhRegion>,
}

/// One LOD vertex.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DhVertex {
    /// Position relative to the region's minimum corner.
    pub pos: [u16; 3],
    pub sky_light: u8,
    pub block_light: u8,
    /// Micro-offset bits `0b00zzyyxx`.
    pub micro: u8,
    pub color: [u8; 4],
    pub material: u8,
    pub normal: u8,
    pub tile: u16,
}

/// Pack a vertex exactly as DH's `LodQuadBuilder.putVertex` does.
pub(crate) fn pack_vertex(w: &mut VertexWriter, v: &DhVertex) {
    let meta = u16::from(v.sky_light & 15) | (u16::from(v.block_light & 15) << 4) | (u16::from(v.micro & 0x3f) << 8);
    w.u16(v.pos[0]).u16(v.pos[1]).u16(v.pos[2]).u16(meta).u8s(v.color).u8(v.material).u8(v.normal).u16(v.tile);
}

fn shade(face: Face) -> f32 {
    match face {
        Face::Up => 1.0,
        Face::Down => 0.5,
        Face::North | Face::South => 0.8,
        Face::West | Face::East => 0.6,
    }
}

/// Cell description.
#[derive(Debug, Clone, Copy)]
struct Cell {
    height: i32,
    material: DhMaterial,
}

fn cell_at(world: &World, cx: i32, cz: i32) -> Cell {
    let h = world.height(cx * CELL + CELL / 2, cz * CELL + CELL / 2);
    let material = if world.is_beach(h) { DhMaterial::Sand } else { DhMaterial::Grass };
    Cell { height: h, material }
}

fn material_look(material: DhMaterial, face: Face) -> ([f32; 3], DhTile) {
    match (material, face) {
        (DhMaterial::Grass, Face::Up) => ([0.57, 0.74, 0.35], DhTile::GrassTop),
        (DhMaterial::Grass, _) => ([1.0, 1.0, 1.0], DhTile::GrassSide),
        (DhMaterial::Sand, _) => ([1.0, 1.0, 1.0], DhTile::Sand),
        (DhMaterial::Stone, _) => ([1.0, 1.0, 1.0], DhTile::Stone),
        (DhMaterial::Dirt, _) => ([1.0, 1.0, 1.0], DhTile::Dirt),
        (DhMaterial::Leaves, _) => ([0.47, 0.65, 0.25], DhTile::Leaves),
        (DhMaterial::Water, _) => ([0.25, 0.46, 0.89], DhTile::Water),
    }
}

/// Emit one axis-aligned quad covering `[min, max]` (one of the extents is zero) on
/// `face`, relative to `origin`.
fn quad(w: &mut VertexWriter, origin: [i32; 3], min: [i32; 3], max: [i32; 3], face: Face, material: DhMaterial, alpha: u8) {
    let (tint, tile) = material_look(material, face);
    let s = shade(face);
    let color = [tint[0] * s, tint[1] * s, tint[2] * s].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    for c in face.corners() {
        let p = [0, 1, 2].map(|i| {
            let v = if c[i] > 0.5 { max[i] } else { min[i] };
            (v - origin[i]).clamp(0, i32::from(u16::MAX)) as u16
        });
        pack_vertex(
            w,
            &DhVertex {
                pos: p,
                sky_light: 15,
                block_light: 0,
                micro: 0,
                color: [color[0], color[1], color[2], alpha],
                material: material as u8,
                normal: face.dh_index(),
                tile: tile as u16,
            },
        );
    }
}

/// Generate LODs out to `dh_distance` chunks around `cam_chunk`; `coverage` says whether
/// the vanilla area (`rd` chunks) gets LODs too.
pub(crate) fn build(world: &World, cam_chunk: [i32; 2], rd: i32, dh_distance: i32, coverage: LodCoverage) -> DhMeshes {
    let sea = world.sea_level();
    // Vanilla area (block coordinates, half-open) and LOD area.
    let inner = [(cam_chunk[0] - rd) * 16, (cam_chunk[0] + rd + 1) * 16, (cam_chunk[1] - rd) * 16, (cam_chunk[1] + rd + 1) * 16];
    let outer = [(cam_chunk[0] - dh_distance) * 16, (cam_chunk[0] + dh_distance + 1) * 16, (cam_chunk[1] - dh_distance) * 16, (cam_chunk[1] + dh_distance + 1) * 16];
    let in_vanilla = |x: i32, z: i32| x >= inner[0] && x < inner[1] && z >= inner[2] && z < inner[3];
    let in_lod = |cx: i32, cz: i32| {
        let (x, z) = (cx * CELL, cz * CELL);
        x >= outer[0] && x < outer[1] && z >= outer[2] && z < outer[3] && (coverage == LodCoverage::Full || !in_vanilla(x, z))
    };
    let mut regions = Vec::new();
    let r0x = outer[0].div_euclid(REGION);
    let r1x = (outer[1] - 1).div_euclid(REGION);
    let r0z = outer[2].div_euclid(REGION);
    let r1z = (outer[3] - 1).div_euclid(REGION);
    for rx in r0x..=r1x {
        for rz in r0z..=r1z {
            let origin = [rx * REGION, REGION_MIN_Y, rz * REGION];
            let mut opaque = VertexWriter::default();
            let mut water = VertexWriter::default();
            for cx in (origin[0] / CELL)..((origin[0] + REGION) / CELL) {
                for cz in (origin[2] / CELL)..((origin[2] + REGION) / CELL) {
                    if !in_lod(cx, cz) {
                        continue;
                    }
                    let cell = cell_at(world, cx, cz);
                    let (x0, z0) = (cx * CELL, cz * CELL);
                    let top = cell.height + 1;
                    quad(&mut opaque, origin, [x0, top, z0], [x0 + CELL, top, z0 + CELL], Face::Up, cell.material, 255);
                    for face in Face::SIDES {
                        let o = face.offset();
                        let (nx, nz) = (cx + o[0], cz + o[2]);
                        let n_top = if in_lod(nx, nz) { cell_at(world, nx, nz).height + 1 } else { top - CELL.min(top - REGION_MIN_Y) };
                        if n_top >= top {
                            continue;
                        }
                        let (mut min, mut max) = ([x0, n_top, z0], [x0 + CELL, top, z0 + CELL]);
                        match face {
                            Face::North => max[2] = z0,
                            Face::South => min[2] = z0 + CELL,
                            Face::West => max[0] = x0,
                            Face::East => min[0] = x0 + CELL,
                            _ => {}
                        }
                        // Tall cliffs show stone, like DH's per-column colour sampling.
                        let material = if top - n_top > 6 { DhMaterial::Stone } else { cell.material };
                        quad(&mut opaque, origin, min, max, face, material, 255);
                    }
                    if cell.height < sea {
                        let y = sea + 1;
                        quad(&mut water, origin, [x0, y, z0], [x0 + CELL, y, z0 + CELL], Face::Up, DhMaterial::Water, 180);
                    }
                }
            }
            if opaque.bytes.is_empty() && water.bytes.is_empty() {
                continue;
            }
            let mut o = CpuMesh::new(&DH_TERRAIN);
            o.push_draw(&opaque.bytes, 0);
            let mut wm = CpuMesh::new(&DH_TERRAIN);
            wm.push_draw(&water.bytes, 0);
            regions.push(DhRegion { origin, opaque: o, water: wm });
        }
    }
    debug_assert!(regions.iter().all(|r| r.opaque.vertices.len() % DH_STRIDE as usize == 0));
    DhMeshes { regions }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dh_vertex_byte_layout() {
        let mut w = VertexWriter::default();
        pack_vertex(
            &mut w,
            &DhVertex { pos: [1, 0x203, 0xffff], sky_light: 15, block_light: 3, micro: 0b11_0001, color: [10, 20, 30, 40], material: 13, normal: 1, tile: 0x0102 },
        );
        assert_eq!(w.bytes.len(), DH_STRIDE as usize);
        assert_eq!(&w.bytes[0..6], &[1, 0, 3, 2, 0xff, 0xff]);
        // meta: sky | block << 4 | micro << 8
        assert_eq!(u16::from_le_bytes([w.bytes[6], w.bytes[7]]), 15 | (3 << 4) | (0b11_0001 << 8));
        assert_eq!(&w.bytes[8..12], &[10, 20, 30, 40]);
        assert_eq!(w.bytes[12], 13);
        assert_eq!(w.bytes[13], 1);
        assert_eq!(u16::from_le_bytes([w.bytes[14], w.bytes[15]]), 0x0102);
    }

    #[test]
    fn lods_cover_only_the_ring() {
        let world = World::new(5);
        let m = build(&world, [0, 0], 1, 4, LodCoverage::Ring);
        assert!(!m.regions.is_empty());
        let mut tops = 0;
        for r in &m.regions {
            assert_eq!(r.origin[1], REGION_MIN_Y);
            for v in r.opaque.vertices.chunks_exact(DH_STRIDE as usize) {
                let x = i32::from(u16::from_le_bytes([v[0], v[1]])) + r.origin[0];
                let y = i32::from(u16::from_le_bytes([v[2], v[3]])) + r.origin[1];
                let z = i32::from(u16::from_le_bytes([v[4], v[5]])) + r.origin[2];
                assert!((-64..=80).contains(&x) && (-64..=80).contains(&z), "{x} {z}");
                assert!(y > 0 && y < 200);
                if v[13] == 1 {
                    tops += 1;
                    // Top quads never lie strictly inside the vanilla area.
                    let inside = |c: i32| c > -16 && c < 32;
                    assert!(!(inside(x) && inside(z)), "{x} {z}");
                }
                assert_eq!(u16::from_le_bytes([v[6], v[7]]) & 0xff, 15);
            }
            assert_eq!(r.opaque.indices.len() % 6, 0);
        }
        assert!(tops > 0);
    }

    /// Full coverage adds the cells under the vanilla chunks (DH with a shader pack) and
    /// keeps the ring identical.
    #[test]
    fn full_coverage_includes_the_vanilla_area() {
        let world = World::new(5);
        // Minimum corner (x, y, z) of every top quad (4 vertices each).
        let tops = |m: &DhMeshes| -> Vec<[i32; 3]> {
            let mut v: Vec<[i32; 3]> = m
                .regions
                .iter()
                .flat_map(|r| {
                    let verts: Vec<&[u8]> = r.opaque.vertices.chunks_exact(DH_STRIDE as usize).collect();
                    verts
                        .chunks_exact(4)
                        .filter(|q| q[0][13] == 1)
                        .map(|q| {
                            let c = |v: &[u8], i: usize| i32::from(u16::from_le_bytes([v[i], v[i + 1]]));
                            let min = |i: usize| q.iter().map(|v| c(v, i)).min().unwrap_or(0);
                            [min(0) + r.origin[0], min(2) + r.origin[1], min(4) + r.origin[2]]
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            v.sort_unstable();
            v
        };
        let ring = tops(&build(&world, [0, 0], 1, 4, LodCoverage::Ring));
        let full = tops(&build(&world, [0, 0], 1, 4, LodCoverage::Full));
        // The DH square is 9x9 chunks (36x36 cells), the vanilla area 3x3 chunks.
        assert_eq!(full.len(), 36 * 36);
        assert_eq!(ring.len(), 36 * 36 - 12 * 12);
        let inside = |p: &[i32; 3]| (-16..32).contains(&p[0]) && (-16..32).contains(&p[2]);
        assert_eq!(full.iter().filter(|p| inside(p)).count(), 12 * 12);
        let full_ring: Vec<[i32; 3]> = full.iter().filter(|p| !inside(p)).copied().collect();
        assert_eq!(full_ring, ring);
    }
}
