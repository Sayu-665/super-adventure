//! The synthetic Minecraft-like scene: voxel heightmap terrain with trees and water,
//! a few entities, the sky, and Distant Horizons LOD terrain beyond the vanilla render
//! distance. Everything is deterministic for a given [`SceneParams::seed`].

pub mod formats;
pub(crate) mod dh;
pub(crate) mod entity;
pub(crate) mod sky;
pub(crate) mod terrain;
pub(crate) mod world;

use crate::math::{self, Vec3};
use formats::VertexLayout;
use sb_core::model::IdMaps;

/// Parameters of the synthetic scene and camera.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneParams {
    /// World seed: terrain, trees and noise depend only on it.
    pub seed: u64,
    /// World time in ticks (`0` = sunrise, `6000` = noon, `18000` = midnight).
    pub world_time: i64,
    /// Rain strength, 0..1 (also drives `wetness`).
    pub rain: f32,
    /// Thunder strength, 0..1.
    pub thunder: f32,
    /// Camera position (x, z) in blocks. The camera height is the terrain height there
    /// plus [`SceneParams::camera_height`].
    pub camera_xz: [f64; 2],
    /// Height of the camera above the terrain.
    pub camera_height: f64,
    /// Camera yaw in degrees (Minecraft convention: 0 = south, 90 = west).
    pub yaw: f64,
    /// Camera pitch in degrees (positive looks down).
    pub pitch: f64,
    /// Vertical field of view in degrees.
    pub fov: f64,
    /// Vanilla render distance in chunks (terrain is generated within it).
    pub render_distance: u32,
    /// Distant Horizons LOD distance in chunks (LODs cover the ring between the vanilla
    /// render distance and this distance; `0` disables LODs).
    pub dh_render_distance: u32,
    /// Seconds per frame (`frameTime`), also advancing `frameTimeCounter`.
    pub frame_time: f32,
    /// Include the entity cubes.
    pub entities: bool,
}

impl Default for SceneParams {
    fn default() -> Self {
        Self {
            seed: 1,
            world_time: 2500,
            rain: 0.0,
            thunder: 0.0,
            camera_xz: [8.5, 8.5],
            camera_height: 22.0,
            yaw: -40.0,
            pitch: 14.0,
            fov: 70.0,
            render_distance: 4,
            dh_render_distance: 16,
            frame_time: 1.0 / 60.0,
            entities: true,
        }
    }
}

impl SceneParams {
    /// Render distance clamped to the supported range (1..=32 chunks).
    pub fn clamped_render_distance(&self) -> u32 {
        self.render_distance.clamp(1, 32)
    }

    /// DH distance clamped to `render_distance..=256` chunks (0 stays 0).
    pub fn clamped_dh_distance(&self) -> u32 {
        if self.dh_render_distance == 0 {
            0
        } else {
            self.dh_render_distance.clamp(self.clamped_render_distance() + 1, 256)
        }
    }

    /// The camera position for this scene (above the terrain at `camera_xz`).
    pub fn camera_position(&self) -> Vec3 {
        let w = world::World::new(self.seed);
        let [x, z] = sanitize_xz(self.camera_xz);
        let ground = w.height(x.floor() as i32, z.floor() as i32).max(w.sea_level());
        let h = if self.camera_height.is_finite() { self.camera_height.clamp(1.0, 400.0) } else { 22.0 };
        [x, f64::from(ground) + 1.0 + h, z]
    }
}

fn sanitize_xz(xz: [f64; 2]) -> [f64; 2] {
    xz.map(|v| if v.is_finite() { v.clamp(-1.0e6, 1.0e6) } else { 0.0 })
}

/// A sub-draw of an indexed mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SubDraw {
    pub first_index: u32,
    pub index_count: u32,
    pub vertex_offset: i32,
    pub first_instance: u32,
}

/// CPU-side indexed mesh in one of the [`formats`] layouts.
#[derive(Debug, Clone)]
pub(crate) struct CpuMesh {
    pub layout: &'static VertexLayout,
    pub vertices: Vec<u8>,
    pub indices: Vec<u32>,
    pub draws: Vec<SubDraw>,
}

impl CpuMesh {
    pub fn new(layout: &'static VertexLayout) -> Self {
        Self { layout, vertices: Vec::new(), indices: Vec::new(), draws: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty() || self.draws.is_empty()
    }

    /// Append quads (4 vertices each, already in `vertices`) as one sub-draw.
    /// `vertex_bytes` holds the quads' vertices; indices 0,1,2,2,3,0 per quad.
    pub fn push_draw(&mut self, vertex_bytes: &[u8], first_instance: u32) {
        let stride = self.layout.stride(0).unwrap_or(1) as usize;
        let vcount = vertex_bytes.len() / stride;
        if vcount < 4 {
            return;
        }
        let base_vertex = (self.vertices.len() / stride) as i32;
        let first_index = self.indices.len() as u32;
        for q in 0..(vcount / 4) as u32 {
            let b = q * 4;
            self.indices.extend_from_slice(&[b, b + 1, b + 2, b + 2, b + 3, b]);
        }
        self.vertices.extend_from_slice(&vertex_bytes[..(vcount / 4) * 4 * stride]);
        self.draws.push(SubDraw {
            first_index,
            index_count: self.indices.len() as u32 - first_index,
            vertex_offset: base_vertex,
            first_instance,
        });
    }

    /// Append raw triangles (indices relative to the appended vertices) as one sub-draw.
    pub fn push_triangles(&mut self, vertex_bytes: &[u8], indices: &[u32]) {
        let stride = self.layout.stride(0).unwrap_or(1) as usize;
        let vcount = (vertex_bytes.len() / stride) as u32;
        if indices.is_empty() || indices.iter().any(|&i| i >= vcount) {
            return;
        }
        let base_vertex = (self.vertices.len() / stride) as i32;
        let first_index = self.indices.len() as u32;
        self.indices.extend_from_slice(indices);
        self.vertices.extend_from_slice(&vertex_bytes[..vcount as usize * stride]);
        self.draws.push(SubDraw { first_index, index_count: indices.len() as u32, vertex_offset: base_vertex, first_instance: 0 });
    }
}

/// Blocks of the synthetic world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)] // `GrassBlock` is the block's registry name
pub(crate) enum Block {
    GrassBlock,
    Dirt,
    Stone,
    Sand,
    Water,
    Leaves,
    Log,
    Planks,
}

impl Block {
    pub const ALL: [Block; 8] =
        [Block::GrassBlock, Block::Dirt, Block::Stone, Block::Sand, Block::Water, Block::Leaves, Block::Log, Block::Planks];

    /// Registry names the block is known by in `block.properties`.
    fn names(self) -> &'static [&'static str] {
        match self {
            Block::GrassBlock => &["grass_block"],
            Block::Dirt => &["dirt"],
            Block::Stone => &["stone"],
            Block::Sand => &["sand"],
            Block::Water => &["water", "flowing_water"],
            Block::Leaves => &["oak_leaves"],
            Block::Log => &["oak_log"],
            Block::Planks => &["oak_planks"],
        }
    }

    /// Light emission (for `at_midBlock.w`).
    pub fn emission(self) -> u8 {
        0
    }
}

/// `block.properties` ids of the scene's blocks (`-1` when the pack does not map them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockIds {
    ids: [i32; 8],
}

impl BlockIds {
    pub fn get(&self, b: Block) -> i32 {
        self.ids[Block::ALL.iter().position(|x| *x == b).unwrap_or(0)]
    }

    /// Resolve from the pack's id map: the first id whose entry list names the block
    /// (with or without the `minecraft:` namespace, with or without block-state
    /// properties). Tags (`%`/`#` entries) are not resolved.
    pub fn from_id_maps(maps: &IdMaps) -> Self {
        let mut ids = [-1; 8];
        for (slot, block) in ids.iter_mut().zip(Block::ALL) {
            *slot = lookup_id(&maps.blocks, block.names()).unwrap_or(-1);
        }
        Self { ids }
    }
}

/// The id of the first map entry naming one of `names`.
pub(crate) fn lookup_id(map: &indexmap::IndexMap<i32, Vec<String>>, names: &[&str]) -> Option<i32> {
    map.iter().find_map(|(id, entries)| entries.iter().any(|e| entry_matches(e, names)).then_some(*id))
}

/// Whether an id-map entry (`[namespace:]name[:prop=value...]`) names one of `names`
/// in the `minecraft` namespace.
fn entry_matches(entry: &str, names: &[&str]) -> bool {
    let entry = entry.trim();
    if entry.is_empty() || entry.starts_with('%') || entry.starts_with('#') {
        return false;
    }
    let parts: Vec<&str> = entry.split(':').collect();
    let (namespace, name) = match parts.as_slice() {
        [name] => ("minecraft", *name),
        [first, second, ..] if !second.contains('=') => (*first, *second),
        [name, ..] => ("minecraft", *name),
        [] => return false,
    };
    namespace == "minecraft" && names.contains(&name)
}

/// All CPU geometry of the scene.
#[derive(Debug, Clone)]
pub(crate) struct CpuScene {
    /// Camera world position.
    pub camera: Vec3,
    pub terrain: terrain::TerrainMeshes,
    pub entities: Vec<entity::EntityInstance>,
    pub entity_mesh: CpuMesh,
    pub sky: sky::SkyMeshes,
    pub dh: dh::DhMeshes,
}

impl CpuScene {
    /// Generate the whole scene (`with_sodium`: the terrain in Sodium's format too).
    pub fn generate(params: &SceneParams, ids: &IdMaps, with_dh: bool, with_sodium: bool) -> Self {
        let world = world::World::new(params.seed);
        let camera = params.camera_position();
        let block_ids = BlockIds::from_id_maps(ids);
        let rd = params.clamped_render_distance() as i32;
        let cam_chunk = [(camera[0].floor() as i32).div_euclid(16), (camera[2].floor() as i32).div_euclid(16)];
        let terrain = terrain::build(&world, &block_ids, cam_chunk, rd, with_sodium);
        let (entities, entity_mesh) = if params.entities {
            entity::build(&world, ids, camera, math::mc_look_vector(params.yaw, 0.0))
        } else {
            (Vec::new(), CpuMesh::new(&formats::VANILLA_ENTITY))
        };
        let sky = sky::build(rd as u32);
        let dh_distance = params.clamped_dh_distance() as i32;
        let dh = if with_dh && dh_distance > rd {
            dh::build(&world, cam_chunk, rd, dh_distance)
        } else {
            dh::DhMeshes::default()
        };
        Self { camera, terrain, entities, entity_mesh, sky, dh }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    #[test]
    fn id_map_matching() {
        let mut maps = IdMaps::default();
        maps.blocks.insert(10, vec!["minecraft:stone".into(), "%minecraft:logs".into()]);
        maps.blocks.insert(11, vec!["oak_leaves:persistent=false".into()]);
        maps.blocks.insert(12, vec!["minecraft:water:level=0".into(), "flowing_water".into()]);
        maps.blocks.insert(13, vec!["othermod:grass_block".into()]);
        maps.blocks.insert(14, vec!["grass_block".into()]);
        let ids = BlockIds::from_id_maps(&maps);
        assert_eq!(ids.get(Block::Stone), 10);
        assert_eq!(ids.get(Block::Leaves), 11);
        assert_eq!(ids.get(Block::Water), 12);
        assert_eq!(ids.get(Block::GrassBlock), 14);
        assert_eq!(ids.get(Block::Log), -1);
        let empty: IndexMap<i32, Vec<String>> = IndexMap::new();
        assert_eq!(lookup_id(&empty, &["pig"]), None);
    }

    #[test]
    fn params_are_sanitized() {
        let p = SceneParams { render_distance: 0, dh_render_distance: 1, camera_xz: [f64::NAN, 1e30], ..Default::default() };
        assert_eq!(p.clamped_render_distance(), 1);
        assert_eq!(p.clamped_dh_distance(), 2);
        let c = p.camera_position();
        assert!(c.iter().all(|v| v.is_finite()));
        let p = SceneParams { dh_render_distance: 0, ..Default::default() };
        assert_eq!(p.clamped_dh_distance(), 0);
    }

    #[test]
    fn mesh_quads_are_indexed_as_dh_does() {
        let mut m = CpuMesh::new(&formats::DH_TERRAIN);
        m.push_draw(&[0u8; 16 * 8], 0);
        assert_eq!(m.indices, vec![0, 1, 2, 2, 3, 0, 4, 5, 6, 6, 7, 4]);
        m.push_draw(&[0u8; 16 * 3], 0); // incomplete quad: ignored
        assert_eq!(m.draws.len(), 1);
        m.push_triangles(&[0u8; 16 * 3], &[0, 1, 2]);
        assert_eq!(m.draws[1].vertex_offset, 8);
        m.push_triangles(&[0u8; 16 * 3], &[0, 1, 5]); // out of range: ignored
        assert_eq!(m.draws.len(), 2);
    }

    #[test]
    fn scene_is_deterministic() {
        let p = SceneParams { render_distance: 1, dh_render_distance: 3, ..Default::default() };
        let a = CpuScene::generate(&p, &IdMaps::default(), true, true);
        let b = CpuScene::generate(&p, &IdMaps::default(), true, true);
        assert_eq!(a.terrain.solid.vertices, b.terrain.solid.vertices);
        assert_eq!(a.dh.regions.len(), b.dh.regions.len());
        assert!(!a.terrain.solid.is_empty());
        assert!(!a.dh.regions.is_empty());
        let c = CpuScene::generate(&SceneParams { seed: 2, ..p }, &IdMaps::default(), true, false);
        assert!(c.terrain.sodium.is_none());
        let sa = a.terrain.sodium.as_ref().expect("sodium meshes");
        assert_eq!(sa.layers[0].vertices, b.terrain.sodium.as_ref().expect("sodium meshes").layers[0].vertices);
        // Same quads in both formats.
        assert_eq!(sa.layers[0].indices.len(), a.terrain.solid.indices.len());
        assert_ne!(a.terrain.solid.vertices, c.terrain.solid.vertices);
    }
}
