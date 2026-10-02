//! Helpers for unit tests.

use sb_core::model::{BindingTable, DhPipeline, DimensionPipeline, PackSettings, RenderTargets, ShadowSettings, UniformLayout};

/// A dimension without programs.
pub(crate) fn empty_dimension() -> DimensionPipeline {
    DimensionPipeline {
        folder: String::new(),
        dimension_ids: Vec::new(),
        targets: RenderTargets {
            colortex: Vec::new(),
            shadowcolor: Vec::new(),
            shadow: ShadowSettings::default(),
            uses_depthtex1: false,
            uses_depthtex2: false,
            noise_texture_resolution: 256,
            noise_texture: None,
            custom_textures: Vec::new(),
            images: Vec::new(),
            buffers: Vec::new(),
        },
        settings: PackSettings::default(),
        uniforms: UniformLayout::default(),
        custom_uniforms: Vec::new(),
        bindings: BindingTable::default(),
        programs: Vec::new(),
        geometry: Default::default(),
        passes: Vec::new(),
        gbuffer_attachments: Vec::new(),
        shadow_attachments: Vec::new(),
        end_of_frame_copies: Vec::new(),
        distant_horizons: DhPipeline::default(),
    }
}
