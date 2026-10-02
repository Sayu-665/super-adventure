//! A few box-shaped entities standing on the terrain in front of the camera, in the
//! `vanilla_entity` vertex layout (model-space positions; the host places them with
//! `DynamicTransforms.ModelOffset`, as Minecraft does for camera-relative entities).

use super::formats::{ENTITY_STRIDE, VANILLA_ENTITY, VertexWriter};
use super::terrain::Face;
use super::world::World;
use super::{CpuMesh, lookup_id};
use crate::math::Vec3;
use sb_core::model::IdMaps;

/// One entity: which sub-draw of the entity mesh it uses and where it stands.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EntityInstance {
    /// Entity type (`minecraft:<name>`).
    pub name: &'static str,
    /// `entity.properties` id (`-1` when unmapped).
    pub entity_id: i32,
    /// World position of the feet.
    pub position: Vec3,
    /// Index into the entity mesh's sub-draws.
    pub draw: usize,
}

/// Lightmap coordinates of entities: full sky light.
const UV2: [i16; 2] = [0, 240];
/// Overlay coordinates of an unhurt entity (`OverlayTexture.NO_OVERLAY`).
const UV1: [i16; 2] = [0, 10];

fn box_quads(w: &mut VertexWriter, min: [f32; 3], max: [f32; 3], uv: [f32; 4]) {
    for face in Face::ALL {
        let n = face.normal();
        for c in face.corners() {
            let p = [0, 1, 2].map(|i| if c[i] > 0.5 { max[i] } else { min[i] });
            let [s, t] = face.sprite_uv(c);
            w.f32s(&p)
                .unorm4([1.0, 1.0, 1.0, 1.0])
                .f32s(&[uv[0] + (uv[2] - uv[0]) * s, uv[1] + (uv[3] - uv[1]) * t])
                .i16(UV1[0])
                .i16(UV1[1])
                .i16(UV2[0])
                .i16(UV2[1])
                .snorm4([n[0], n[1], n[2], 0.0]);
        }
    }
}

/// Build the entities: a pig, a cow and a zombie spread out in front of the camera.
pub(crate) fn build(world: &World, ids: &IdMaps, camera: Vec3, forward: Vec3) -> (Vec<EntityInstance>, CpuMesh) {
    let mut mesh = CpuMesh::new(&VANILLA_ENTITY);
    let mut out = Vec::new();
    let flen = (forward[0] * forward[0] + forward[2] * forward[2]).sqrt();
    let (fx, fz) = if flen > 1e-6 { (forward[0] / flen, forward[2] / flen) } else { (0.0, 1.0) };
    let (sx, sz) = (-fz, fx);
    // (name, distance ahead, sideways offset, body size, head size)
    let specs: [(&'static str, f64, f64, [f32; 3], f32); 3] = [
        ("pig", 7.0, -2.5, [0.9, 0.9, 1.3], 0.5),
        ("cow", 10.0, 2.0, [0.9, 1.3, 1.5], 0.6),
        ("zombie", 13.0, -0.5, [0.6, 1.4, 0.4], 0.5),
    ];
    for (name, ahead, side, body, head) in specs {
        let x = camera[0] + fx * ahead + sx * side;
        let z = camera[2] + fz * ahead + sz * side;
        let ground = world.height(x.floor() as i32, z.floor() as i32).max(world.sea_level());
        let position = [x, f64::from(ground) + 1.0, z];
        let mut w = VertexWriter::default();
        let hb = [body[0] * 0.5, body[2] * 0.5];
        box_quads(&mut w, [-hb[0], 0.0, -hb[1]], [hb[0], body[1], hb[1]], [0.0, 0.0, 0.5, 1.0]);
        let hh = head * 0.5;
        box_quads(&mut w, [-hh, body[1], hb[1] - hh], [hh, body[1] + head, hb[1] + hh], [0.5, 0.0, 1.0, 0.5]);
        debug_assert_eq!(w.bytes.len() % ENTITY_STRIDE as usize, 0);
        let draw = mesh.draws.len();
        mesh.push_draw(&w.bytes, 0);
        let full = format!("minecraft:{name}");
        let entity_id = lookup_id(&ids.entities, &[name]).or_else(|| lookup_id(&ids.entities, &[full.as_str()])).unwrap_or(-1);
        out.push(EntityInstance { name, entity_id, position, draw });
    }
    (out, mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_vertices_follow_the_entity_format() {
        let mut ids = IdMaps::default();
        ids.entities.insert(42, vec!["minecraft:zombie".into()]);
        let (ents, mesh) = build(&World::new(1), &ids, [0.0, 80.0, 0.0], [0.0, 0.0, 1.0]);
        assert_eq!(ents.len(), 3);
        assert_eq!(ents[2].entity_id, 42);
        assert_eq!(ents[0].entity_id, -1);
        assert_eq!(mesh.draws.len(), 3);
        assert_eq!(mesh.vertices.len() % ENTITY_STRIDE as usize, 0);
        // 2 boxes x 6 faces x 4 vertices per entity.
        assert_eq!(mesh.vertices.len() / ENTITY_STRIDE as usize, 3 * 48);
        let v = &mesh.vertices[..ENTITY_STRIDE as usize];
        assert_eq!(i16::from_le_bytes([v[24], v[25]]), 0);
        assert_eq!(i16::from_le_bytes([v[26], v[27]]), 10);
        assert_eq!(i16::from_le_bytes([v[30], v[31]]), 240);
        assert!(ents.iter().all(|e| e.position[1] > 60.0));
    }
}
