//! Sky geometry: Iris' horizon cone and Minecraft's sky disc (`vanilla_position`), and
//! the sun and moon quads (`vanilla_position_tex`).

use super::CpuMesh;
use super::formats::{VANILLA_POSITION, VANILLA_POSITION_TEX, VertexWriter};

/// Sub-draw of [`SkyMeshes::disc`] holding Iris' horizon cone.
pub(crate) const SKY_HORIZON: usize = 0;
/// Sub-draw of [`SkyMeshes::disc`] holding the vanilla upper sky disc.
pub(crate) const SKY_DISC: usize = 1;

/// Sky meshes.
#[derive(Debug, Clone)]
pub(crate) struct SkyMeshes {
    /// Two sub-draws, both drawn with `gbuffers_skybasic`:
    ///
    /// * [`SKY_HORIZON`]: Iris' `HorizonRenderer` cone, a triangle fan from 16 blocks below
    ///   the camera to an octagon 16 blocks above it with radius
    ///   `min(render distance × 16, 256)`, drawn with the fog colour as `ColorModulator`.
    ///   Vanilla leaves the sky below the disc to the clear colour; Iris draws this cone so
    ///   that packs which shade the sky in `gbuffers_skybasic` cover the horizon too.
    /// * [`SKY_DISC`]: the upper sky disc (16 blocks above the camera, radius 512), drawn
    ///   with the sky colour as `ColorModulator`.
    pub disc: CpuMesh,
    /// The sun quad (half size 30 at y = 100 in the celestial frame).
    pub sun: CpuMesh,
    /// The moon quads, one sub-draw per moon phase (half size 20 at y = -100).
    pub moon: CpuMesh,
}

/// Iris' horizon cone (`HorizonRenderer.buildHorizon`): the fan centre 16 blocks below the
/// camera, then 9 vertices (the octagon, closed) 16 blocks above it.
fn horizon(m: &mut CpuMesh, render_distance_chunks: u32) {
    let radius = (render_distance_chunks.saturating_mul(16)).min(256) as f32;
    let mut w = VertexWriter::default();
    w.f32s(&[0.0, -16.0, 0.0]);
    for i in 0..=8u32 {
        let a = -(i as f32) * std::f32::consts::FRAC_PI_4;
        w.f32s(&[radius * a.cos(), 16.0, radius * a.sin()]);
    }
    let idx: Vec<u32> = (1..=8).flat_map(|i| [0, i, i + 1]).collect();
    m.push_triangles(&w.bytes, &idx);
}

/// The horizon cone ([`SKY_HORIZON`]) and the upper disc ([`SKY_DISC`]).
fn disc(render_distance_chunks: u32) -> CpuMesh {
    let mut m = CpuMesh::new(&VANILLA_POSITION);
    horizon(&mut m, render_distance_chunks);
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

/// Build the sky meshes for a vanilla render distance (in chunks).
pub(crate) fn build(render_distance_chunks: u32) -> SkyMeshes {
    let mut sun = CpuMesh::new(&VANILLA_POSITION_TEX);
    celestial_quad(&mut sun, 100.0, 30.0, [0.0, 0.0, 1.0, 1.0]);
    let mut moon = CpuMesh::new(&VANILLA_POSITION_TEX);
    for phase in 0..8u32 {
        let (col, row) = ((phase % 4) as f32, (phase / 4) as f32);
        celestial_quad(&mut moon, -100.0, 20.0, [col / 4.0, row / 2.0, (col + 1.0) / 4.0, (row + 1.0) / 2.0]);
    }
    SkyMeshes { disc: disc(render_distance_chunks), sun, moon }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sky_meshes() {
        let s = build(4);
        assert_eq!(s.disc.draws.len(), 2);
        // Horizon: 8 fan triangles over 10 vertices; disc: 16 triangles over 18 vertices.
        assert_eq!(s.disc.draws[SKY_HORIZON].index_count, 24);
        assert_eq!(s.disc.draws[SKY_DISC].index_count, 48);
        assert_eq!(s.disc.vertices.len(), (10 + 18) * 12);
        assert_eq!(s.sun.draws.len(), 1);
        assert_eq!(s.moon.draws.len(), 8);
        assert_eq!(s.sun.vertices.len(), 4 * 20);
    }

    /// The horizon cone spans y = -16..16 with Iris' radius (render distance × 16, at
    /// most 256).
    #[test]
    fn horizon_matches_iris() {
        let ys = |m: &CpuMesh, n: usize| -> Vec<[f32; 3]> {
            (0..n)
                .map(|v| {
                    let f = |k: usize| f32::from_le_bytes(m.vertices[v * 12 + k * 4..v * 12 + k * 4 + 4].try_into().unwrap());
                    [f(0), f(1), f(2)]
                })
                .collect()
        };
        for (rd, radius) in [(4, 64.0), (32, 256.0)] {
            let s = build(rd);
            let v = ys(&s.disc, 10);
            assert_eq!(v[0], [0.0, -16.0, 0.0]);
            for p in &v[1..] {
                assert!((p[1] - 16.0).abs() < 1e-6);
                assert!(((p[0] * p[0] + p[2] * p[2]).sqrt() - radius).abs() < 1e-3, "{p:?}");
            }
        }
    }
}
