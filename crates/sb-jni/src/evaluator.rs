//! Custom-uniform evaluators (`createUniformEvaluator` / `evaluateUniforms`).

use crate::error::{Error, Result};
use crate::registry::Recover;
use sb_core::model::{BlockLayout, CustomUniform};
use sb_expr::CustomUniforms;

/// The compiled `uniform.*` / `variable.*` definitions of one dimension folder plus its
/// `sb_Frame` layout. It owns copies of both, so it stays valid after its session is
/// closed or recompiled.
pub(crate) struct Evaluator {
    uniforms: CustomUniforms,
    frame: BlockLayout,
}

impl Evaluator {
    /// Compile `defs` for the block `frame` (same inputs, constants and rules as the
    /// pipeline and `sb-runtime`). Fails when there is nothing to evaluate.
    pub(crate) fn new(folder: &str, defs: &[CustomUniform], frame: BlockLayout) -> Result<Self> {
        if defs.is_empty() {
            return Err(Error::Unavailable(format!("dimension folder `{folder}` has no custom uniforms")));
        }
        let (uniforms, _diagnostics) = CustomUniforms::compile(
            defs,
            &sb_uniforms::custom_uniform_input_type,
            &sb_pipeline::macros::expression_constants(),
        );
        // The diagnostics are already part of the compiled pack's diagnostics.
        if uniforms.is_empty() {
            return Err(Error::Unavailable(format!(
                "none of the custom uniforms of dimension folder `{folder}` compiled (see the pack diagnostics)"
            )));
        }
        Ok(Self { uniforms, frame })
    }

    /// Evaluate in place on `block`, the whole `sb_Frame` block: builtin members are read,
    /// custom members written. `frame_delta_seconds` drives `smooth()`; non-finite or
    /// negative values count as 0.
    pub(crate) fn evaluate(&mut self, block: &mut [u8], frame_delta_seconds: f32) -> Result<()> {
        let size = self.frame.size as usize;
        if block.len() < size {
            return Err(Error::invalid(format!(
                "the frame block has {} bytes but `{}` needs {size}",
                block.len(),
                self.frame.name
            )));
        }
        self.uniforms.evaluate_into_block(&self.frame, block, frame_delta_seconds);
        Ok(())
    }
}

impl Recover for Evaluator {
    fn recover(&mut self) {
        self.uniforms.reset();
    }
}
