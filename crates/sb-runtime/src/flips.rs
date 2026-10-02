//! Main/alt ("flip") state of the colortex and shadowcolor buffers over a frame, as in
//! Iris' `BufferFlipper`:
//!
//! * every frame starts with all buffers in their main image;
//! * gbuffers/shadow geometry reads and writes the current "main" image (`read`);
//! * composite-style programs read the current image and write the other one, then the
//!   pass's `flips_after` swap the roles;
//! * the model's per-pass `flip_state` is authoritative (it is adopted when present);
//! * at the end of the frame, buffers flipped an odd number of times are copied
//!   alt → main (`end_of_frame_copies`), so history-dependent effects see the latest
//!   data in main next frame.
//!
//! Shadowcolor buffers ping-pong the same way in `shadowcomp` passes (Iris'
//! `ShadowCompositeRenderer`); the model has no shadowcolor schedule, so the runtime
//! derives it from the buffers each shadowcomp program writes.

/// The flip state of one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Flips {
    color: Vec<bool>,
    shadow: Vec<bool>,
}

impl Default for Flips {
    fn default() -> Self {
        Self { color: vec![false; sb_uniforms::MAX_COLOR_TEX as usize], shadow: vec![false; sb_uniforms::MAX_SHADOW_COLOR as usize] }
    }
}

impl Flips {
    /// Start of a frame: everything in main.
    pub fn reset(&mut self) {
        self.color.iter_mut().for_each(|f| *f = false);
        self.shadow.iter_mut().for_each(|f| *f = false);
    }

    /// Image (0 = main, 1 = alt) holding colortex `i`'s current contents: what programs
    /// read and what geometry writes.
    pub fn read(&self, i: u32) -> usize {
        usize::from(self.color.get(i as usize).copied().unwrap_or(false))
    }

    /// Image a composite-style program writes for colortex `i`.
    pub fn write(&self, i: u32) -> usize {
        1 - self.read(i)
    }

    /// Current image of shadowcolor `i`.
    pub fn shadow_read(&self, i: u32) -> usize {
        usize::from(self.shadow.get(i as usize).copied().unwrap_or(false))
    }

    /// Image a shadowcomp program writes for shadowcolor `i`.
    pub fn shadow_write(&self, i: u32) -> usize {
        1 - self.shadow_read(i)
    }

    /// Adopt the model's flip state at the start of a pass. Returns the indices among
    /// `known` (existing targets) whose tracked state disagreed.
    pub fn adopt(&mut self, pass_state: &[bool], known: impl Fn(u32) -> bool) -> Vec<u32> {
        let mut mismatches = Vec::new();
        for (i, &s) in pass_state.iter().enumerate() {
            if let Some(f) = self.color.get_mut(i) {
                if *f != s && known(i as u32) {
                    mismatches.push(i as u32);
                }
                *f = s;
            }
        }
        mismatches
    }

    /// Swap main/alt of the given colortex buffers (a pass's `flips_after`).
    pub fn flip(&mut self, indices: &[u32]) {
        for &i in indices {
            if let Some(f) = self.color.get_mut(i as usize) {
                *f = !*f;
            }
        }
    }

    /// Swap main/alt of the shadowcolor buffers a shadowcomp program wrote.
    pub fn flip_shadow(&mut self, indices: &[u32]) {
        for &i in indices {
            if let Some(f) = self.shadow.get_mut(i as usize) {
                *f = !*f;
            }
        }
    }

    /// Which of the model's `end_of_frame_copies` apply now (buffer currently in alt).
    pub fn end_of_frame_copies(&self, copies: &[u32]) -> Vec<u32> {
        copies.iter().copied().filter(|&i| self.read(i) == 1).collect()
    }

    /// Shadowcolor buffers (among the non-cleared `candidates`) that end the frame in
    /// their alt image and must be copied back.
    pub fn shadow_copies(&self, candidates: &[u32]) -> Vec<u32> {
        candidates.iter().copied().filter(|&i| self.shadow_read(i) == 1).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_ping_pong_like_iris() {
        // composite (writes 0, flips 0) → composite1 (writes 0 and 1, flips both) →
        // composite2 (writes 2 with flip.composite2.colortex2=false: no flip) → final.
        let mut f = Flips::default();
        assert_eq!((f.read(0), f.write(0)), (0, 1));
        f.flip(&[0]);
        assert_eq!((f.read(0), f.write(0)), (1, 0));
        f.flip(&[0, 1]);
        assert_eq!(f.read(0), 0);
        assert_eq!(f.read(1), 1);
        // composite2 writes the alt of 2 but does not flip: later passes keep reading main.
        assert_eq!(f.write(2), 1);
        assert_eq!(f.read(2), 0);
        // colortex1 was flipped an odd number of times; colortex0 twice.
        assert_eq!(f.end_of_frame_copies(&[0, 1, 2]), vec![1]);
        f.reset();
        assert_eq!(f.read(1), 0);
    }

    #[test]
    fn model_flip_state_is_authoritative() {
        let mut f = Flips::default();
        // The model says colortex3 is in alt at this pass (e.g. a `prepare_pre` flip the
        // runtime did not see); colortex4 does not exist and is not reported.
        let m = f.adopt(&[false, false, false, true, true], |i| i != 4);
        assert_eq!(m, vec![3]);
        assert_eq!(f.read(3), 1);
        assert_eq!(f.read(4), 1);
        // Consistent state: no mismatch.
        assert!(f.adopt(&[false, false, false, true], |_| true).is_empty());
        // Out-of-range indices are ignored.
        f.flip(&[999]);
        assert_eq!(f.read(999), 0);
        assert!(f.adopt(&[true; 100], |_| true).len() <= 32);
    }

    #[test]
    fn shadowcolor_ping_pong() {
        let mut f = Flips::default();
        assert_eq!((f.shadow_read(0), f.shadow_write(0)), (0, 1));
        f.flip_shadow(&[0, 1]);
        f.flip_shadow(&[1]);
        assert_eq!(f.shadow_read(0), 1);
        assert_eq!(f.shadow_read(1), 0);
        assert_eq!(f.shadow_copies(&[0, 1]), vec![0]);
    }
}
