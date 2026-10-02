//! Sky geometry: Minecraft's sky disc (`vanilla_position`) and the sun and moon quads
//! (`vanilla_position_tex`).

use super::CpuMesh;
use super::formats::{VANILLA_POSITION, VANILLA_POSITION_TEX, VertexWriter};

/// Sky meshes.
#[derive(Debug, Clone)]
pub(crate) struct SkyMeshes {
    /// The upper sky disc (16 blocks above the camera, radius 512), drawn with the sky
    /// colour as `ColorModulator`.
    pub disc: CpuMesh,
    /// The sun quad (half size 30 at y = 100 in the celestial frame).
    pub sun: CpuMesh,
    /// The moon quads, one sub-draw per moon phase (half size 20 at y = -100).
    pub moon: CpuMesh,
}

fn disc() -> CpuMesh {
    let mut m = CpuMesh::new(&VANILLA_POSITION);
    let mut w = VertexWriter::default();
    let segments = 16u32;
    w.f32s(&[0.0, 16.0, 0.0]);
    for i in 0..=segments {
        // Clockwise seen from above = counter-clockwise seen from below (the camera).
        let a = -(i as f32) * std::f32::consts::TAU / segments as f32;
        w.f32s(&[512.0 * a.cos(), 16.0, 512.0 * a.sin()]);
    }
    let mut idx = Vec::new();
    for i in 1..=segments {
        idx.extend_from_slice(&[0, i, i + 1]);
    }
    m.push_triangles(&w.bytes, &idx);
    m
}

fn celestial_quad(m: &mut CpuMesh, y: f32, half: f32, uv: [f32; 4]) {
    let mut w = VertexWriter::default();
    // Vanilla order (counter-clockwise seen from the camera at the origin).
    let corners = if y > 0.0 {
        [[-half, -half], [half, -half], [half, half], [-half, half]]
    } else {
        [[-half, half], [half, half], [half, -half], [-half, -half]]
    };
    let uvs = [[uv[2], uv[3]], [uv[0], uv[3]], [uv[0], uv[1]], [uv[2], uv[1]]];
    for (c, t) in corners.iter().zip(uvs) {
        w.f32s(&[c[0], y, c[1]]).f32s(&t);
    }
    m.push_draw(&w.bytes, 0);
}

/// Build the sky meshes.
pub(crate) fn build() -> SkyMeshes {
    let mut sun = CpuMesh::new(&VANILLA_POSITION_TEX);
    celestial_quad(&mut sun, 100.0, 30.0, [0.0, 0.0, 1.0, 1.0]);
    let mut moon = CpuMesh::new(&VANILLA_POSITION_TEX);
    for phase in 0..8u32 {
        let (col, row) = ((phase % 4) as f32, (phase / 4) as f32);
        celestial_quad(&mut moon, -100.0, 20.0, [col / 4.0, row / 2.0, (col + 1.0) / 4.0, (row + 1.0) / 2.0]);
    }
    SkyMeshes { disc: disc(), sun, moon }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sky_meshes() {
        let s = build();
        assert_eq!(s.disc.indices.len(), 48);
        assert_eq!(s.disc.vertices.len(), 18 * 12);
        assert_eq!(s.sun.draws.len(), 1);
        assert_eq!(s.moon.draws.len(), 8);
        assert_eq!(s.sun.vertices.len(), 4 * 20);
    }
}
