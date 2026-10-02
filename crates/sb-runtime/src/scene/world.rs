//! The procedural world: a seeded value-noise heightmap with sea level, beaches and trees.

/// Deterministic 64-bit hash (SplitMix64 finalizer).
pub(crate) fn hash64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// Hash of a 2D lattice point and a salt, as a float in `[0, 1)`.
pub(crate) fn hash2(seed: u64, x: i32, z: i32, salt: u64) -> f64 {
    let k = hash64(seed ^ hash64((x as u32 as u64) | ((z as u32 as u64) << 32)) ^ salt.wrapping_mul(0x5851_f42d_4c95_7f2d));
    (k >> 11) as f64 / (1u64 << 53) as f64
}

/// The world generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct World {
    seed: u64,
}

impl World {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }

    /// Water fills columns up to this block level.
    pub fn sea_level(&self) -> i32 {
        62
    }

    /// Smooth value noise in `[-1, 1]` with lattice spacing `scale`.
    fn noise(&self, x: f64, z: f64, scale: f64, salt: u64) -> f64 {
        let fx = x / scale;
        let fz = z / scale;
        let ix = fx.floor();
        let iz = fz.floor();
        let tx = fx - ix;
        let tz = fz - iz;
        let s = |t: f64| t * t * (3.0 - 2.0 * t);
        let (ix, iz) = (ix as i32, iz as i32);
        let v = |dx: i32, dz: i32| hash2(self.seed, ix.wrapping_add(dx), iz.wrapping_add(dz), salt) * 2.0 - 1.0;
        let a = v(0, 0) + (v(1, 0) - v(0, 0)) * s(tx);
        let b = v(0, 1) + (v(1, 1) - v(0, 1)) * s(tx);
        a + (b - a) * s(tz)
    }

    /// Terrain height (the y of the top solid block) of column `(x, z)`.
    pub fn height(&self, x: i32, z: i32) -> i32 {
        let (x, z) = (f64::from(x) + 0.5, f64::from(z) + 0.5);
        let h = 66.0
            + 22.0 * self.noise(x, z, 192.0, 1)
            + 9.0 * self.noise(x, z, 56.0, 2)
            + 3.0 * self.noise(x, z, 19.0, 3)
            + 1.0 * self.noise(x, z, 7.0, 4);
        h.round().clamp(1.0, 250.0) as i32
    }

    /// Whether the column top is sand (at or just above sea level).
    pub fn is_beach(&self, height: i32) -> bool {
        height <= self.sea_level() + 1
    }

    /// Whether a tree grows on column `(x, z)`: grass columns well above the water,
    /// with a deterministic ~1.2% chance and no other tree within 2 blocks.
    pub fn tree_at(&self, x: i32, z: i32) -> bool {
        let candidate = |x: i32, z: i32| {
            let h = self.height(x, z);
            h > self.sea_level() + 2 && hash2(self.seed, x, z, 77) < 0.012
        };
        if !candidate(x, z) {
            return false;
        }
        // Keep the first candidate (in scan order) of each neighbourhood.
        for dz in -2..=2 {
            for dx in -2..=2 {
                if (dz < 0 || (dz == 0 && dx < 0)) && candidate(x + dx, z + dz) {
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heightmap_is_deterministic_and_varied() {
        let w = World::new(7);
        let mut min = i32::MAX;
        let mut max = i32::MIN;
        for x in -100..100 {
            for z in (-100..100).step_by(7) {
                let h = w.height(x, z);
                assert_eq!(h, World::new(7).height(x, z));
                min = min.min(h);
                max = max.max(h);
            }
        }
        assert!(max - min > 10, "{min}..{max}");
        assert_ne!(World::new(8).height(3, 4) * 1000 + World::new(8).height(40, -9), w.height(3, 4) * 1000 + w.height(40, -9));
        // Extreme coordinates do not overflow.
        let _ = w.height(i32::MAX, i32::MIN);
    }

    #[test]
    fn trees_are_sparse_and_spaced() {
        let w = World::new(1);
        let trees: Vec<(i32, i32)> = (-64..64).flat_map(|x| (-64..64).map(move |z| (x, z))).filter(|&(x, z)| w.tree_at(x, z)).collect();
        assert!(!trees.is_empty() && trees.len() < 400, "{}", trees.len());
        for (i, a) in trees.iter().enumerate() {
            for b in &trees[i + 1..] {
                assert!((a.0 - b.0).abs() > 2 || (a.1 - b.1).abs() > 2, "{a:?} {b:?}");
            }
        }
    }
}
