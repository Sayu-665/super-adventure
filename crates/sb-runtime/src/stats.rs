//! Render statistics.

/// A program that could not be used, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedProgram {
    /// Program name (`world0/gbuffers_terrain`, `composite3`, ...).
    pub name: String,
    /// Why it was skipped.
    pub reason: String,
}

/// What the last frame did, plus everything the runtime had to work around.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Frames rendered.
    pub frames: u32,
    /// Passes executed in the last frame (model passes plus geometry passes).
    pub passes_run: u32,
    /// Draw calls of the last frame.
    pub draws: u32,
    /// Compute dispatches of the last frame.
    pub dispatches: u32,
    /// Graphics/compute pipelines created.
    pub pipelines_created: u32,
    /// Programs that were skipped, with reasons (each program once).
    pub programs_skipped: Vec<SkippedProgram>,
    /// Scene geometry drawn without a pack program (no `geometry` entry), e.g.
    /// `gbuffers_water (water)`.
    pub geometry_skipped: Vec<String>,
    /// Model inconsistencies and fallbacks (missing textures, unknown resources, ...).
    pub warnings: Vec<String>,
    /// Validation-layer errors (also in `RenderOutput::validation_messages`).
    pub validation_errors: u32,
    /// Validation-layer warnings.
    pub validation_warnings: u32,
}

impl FrameStats {
    pub(crate) fn skip_program(&mut self, name: &str, reason: &str) {
        if !self.programs_skipped.iter().any(|s| s.name == name) {
            log::warn!("skipping program `{name}`: {reason}");
            self.programs_skipped.push(SkippedProgram { name: name.to_string(), reason: reason.to_string() });
        }
    }

    pub(crate) fn skip_geometry(&mut self, what: String) {
        if !self.geometry_skipped.contains(&what) {
            self.geometry_skipped.push(what);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_are_recorded_once() {
        let mut s = FrameStats::default();
        s.skip_program("a", "x");
        s.skip_program("a", "y");
        s.skip_geometry("water".into());
        s.skip_geometry("water".into());
        assert_eq!(s.programs_skipped.len(), 1);
        assert_eq!(s.programs_skipped[0].reason, "x");
        assert_eq!(s.geometry_skipped.len(), 1);
    }
}
