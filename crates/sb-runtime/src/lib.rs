//! # sb-runtime: headless Vulkan executor for ShaderBridge
//!
//! `sb-runtime` executes a [`CompiledPack`] (the `sb-core` model plus its [`BlobTable`])
//! on a Vulkan device without a window. It renders a synthetic Minecraft-like scene —
//! voxel terrain with trees and water, entities, the sky, and Distant Horizons LOD
//! terrain beyond the vanilla render distance — through the pack's passes and returns the
//! final image (and optionally every render target).
//!
//! It is both the end-to-end proof that translated packs run on Vulkan (pipelines link,
//! descriptors match, nothing faults under the Khronos validation layer) and the
//! reference implementation of the host contract that the Java mod mirrors:
//!
//! * frame order follows Iris: clears → `setup` (first frame) → `begin` → shadow pass
//!   (opaque casters, shadowtex1 copy, translucent casters) → `shadowcomp` → `prepare` →
//!   opaque gbuffers (sky, DH LODs, terrain, entities) → centre depth sample and depthtex2
//!   copy (`beginHand`), depthtex1 copy (`beginTranslucents`) → `deferred` → translucent
//!   gbuffers (water, DH water) → `composite` → `final` → end-of-frame alt → main copies;
//!   composite-style programs read the current image of each colortex and write the
//!   other, as in Iris' `BufferFlipper`, with the model's `flip_state` as the authority;
//! * every attachment is written only where the fragment shader declares an output of
//!   the attachment's numeric class (otherwise it is masked or bound to a sink), shadow
//!   samplers always get comparison samplers, and stage interfaces are checked before a
//!   program is used;
//! * every builtin uniform of the sb-uniforms registry has a provider, `sb_Frame` is
//!   filled once per frame and custom uniforms are evaluated with `sb-expr`;
//! * the draw-profile host blocks (`Globals`, `TerrainUniform`, `DynamicTransforms`, DH's
//!   `vertUniqueUniformBlock`/`vertSharedUniformBlock`, ...) are filled per draw;
//! * the scene's vertex buffers are byte-exact Minecraft 26.3 / DH 3.3 layouts, plus
//!   Sodium 0.9's compact terrain format for programs translated with the
//!   `sodium_terrain` profile ([`scene::formats`]);
//! * matrices are GL-style ([`math`]), including Iris' shadow and celestial math.
//!
//! The runtime never translates anything: it only consumes the model. Model
//! inconsistencies never panic — the offending program is skipped and recorded in
//! [`FrameStats`].
//!
//! ```no_run
//! use sb_runtime::{NoTextures, RenderRequest, Runtime, RuntimeOptions, SceneParams};
//! # fn demo(pack: &sb_core::model::CompiledPack, blobs: &sb_core::model::BlobTable) -> Result<(), sb_runtime::RuntimeError> {
//! let mut rt = Runtime::new(&RuntimeOptions { validation: true, prefer_cpu_device: true, device_name_filter: None })?;
//! let out = rt.render(&RenderRequest {
//!     pack,
//!     blobs,
//!     dimension: "world0",
//!     width: 640,
//!     height: 360,
//!     frames: 3,
//!     scene: SceneParams::default(),
//!     depth_mode: pack.info.environment.depth_mode,
//!     textures: &NoTextures,
//!     capture_targets: false,
//! })?;
//! out.image.save("out.png").ok();
//! assert_eq!(out.stats.validation_errors, 0);
//! # Ok(()) }
//! ```

#![warn(missing_docs)]

mod descriptors;
mod device;
mod error;
mod executor;
mod flips;
mod frame;
pub mod math;
mod pipelines;
mod readback;
mod resources;
pub mod scene;
mod stats;
mod texel;
mod textures;
mod uniforms;

#[cfg(test)]
mod testutil;

pub use device::{DeviceInfo, RuntimeOptions};
pub use error::RuntimeError;
pub use scene::SceneParams;
pub use stats::{FrameStats, SkippedProgram};
pub use textures::{DirTextureSource, NoTextures, TextureSource};

use device::{Gpu, MessageSeverity};
use executor::Executor;
use sb_core::model::{BlobTable, CompiledPack, DepthMode};
use std::path::{Path, PathBuf};

/// Largest accepted output size per axis.
pub const MAX_RENDER_SIZE: u32 = 8192;
/// Most frames one request may render.
pub const MAX_FRAMES: u32 = 1000;

/// One render of a pack.
pub struct RenderRequest<'a> {
    /// The compiled pack.
    pub pack: &'a CompiledPack,
    /// Its blob table (SPIR-V).
    pub blobs: &'a BlobTable,
    /// World folder of the [`DimensionPipeline`](sb_core::model::DimensionPipeline) to
    /// render (`""` = the pack root, `world0`, ...).
    pub dimension: &'a str,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Frames to render; the output is the last one (temporal effects settle).
    pub frames: u32,
    /// Scene and camera.
    pub scene: SceneParams,
    /// Depth convention; must match the mode the pack was compiled with
    /// (`pack.info.environment.depth_mode`), otherwise depth reads are wrong.
    pub depth_mode: DepthMode,
    /// Pack files for custom textures, `texture.noise` and SSBO contents.
    pub textures: &'a dyn TextureSource,
    /// Also return every colortex / shadow / depth target of the last frame.
    pub capture_targets: bool,
}

/// The result of a render.
#[derive(Debug, Clone)]
pub struct RenderOutput {
    /// The final image (top row first).
    pub image: image::RgbaImage,
    /// Render targets of the last frame when [`RenderRequest::capture_targets`] is set:
    /// `colortexN`, `shadowcolorN`, `depthtexN`, `shadowtexN`, `dhDepthTexN` (depth in
    /// GL convention as grey).
    pub targets: Vec<(String, image::RgbaImage)>,
    /// Validation-layer errors and warnings, prefixed with `[error]` / `[warning]`.
    pub validation_messages: Vec<String>,
    /// What was rendered and what was skipped.
    pub stats: FrameStats,
}

impl RenderOutput {
    /// The validation messages of error severity.
    pub fn validation_errors(&self) -> impl Iterator<Item = &String> {
        self.validation_messages.iter().filter(|m| m.starts_with("[error]"))
    }
}

/// A Vulkan instance, device, queue and allocator, reusable across packs.
pub struct Runtime {
    gpu: Gpu,
}

impl Runtime {
    /// Create the instance and pick a device (see [`RuntimeOptions`]).
    pub fn new(opts: &RuntimeOptions) -> Result<Runtime, RuntimeError> {
        Ok(Runtime { gpu: Gpu::new(opts)? })
    }

    /// The selected device and its capabilities.
    pub fn device_info(&self) -> DeviceInfo {
        self.gpu.info.clone()
    }

    /// Render `req.frames` frames of a pack and read the result back.
    pub fn render(&mut self, req: &RenderRequest<'_>) -> Result<RenderOutput, RuntimeError> {
        if req.width == 0 || req.height == 0 || req.width > MAX_RENDER_SIZE || req.height > MAX_RENDER_SIZE {
            return Err(RuntimeError::InvalidRequest(format!("size {}x{} is outside 1..={MAX_RENDER_SIZE}", req.width, req.height)));
        }
        if req.frames > MAX_FRAMES {
            return Err(RuntimeError::InvalidRequest(format!("{} frames requested (at most {MAX_FRAMES})", req.frames)));
        }
        let Some(dim) = req.pack.dimensions.iter().find(|d| d.folder == req.dimension) else {
            let folders: Vec<String> = req.pack.dimensions.iter().map(|d| format!("`{}`", d.folder)).collect();
            return Err(RuntimeError::InvalidRequest(format!("the pack has no dimension folder `{}` (available: {})", req.dimension, folders.join(", "))));
        };
        // Drop messages left over from earlier renders.
        let _ = self.gpu.take_messages();
        let mut exec = Executor::new(&mut self.gpu, req, dim)?;
        if req.depth_mode != req.pack.info.environment.depth_mode {
            exec.warn(format!(
                "depth mode {:?} differs from the mode the pack was compiled for ({:?}); depth reads will be wrong",
                req.depth_mode, req.pack.info.environment.depth_mode
            ));
        }
        exec.run(req.frames)?;
        let image = exec.read_output()?;
        let targets = if req.capture_targets { exec.read_targets()? } else { Vec::new() };
        let mut stats = std::mem::take(&mut exec.stats);
        stats.pipelines_created = exec.pipelines_created();
        drop(exec);
        let mut messages = self.gpu.setup_notes().to_vec();
        messages.extend(self.gpu.take_messages());
        stats.validation_errors = messages.iter().filter(|m| m.severity == MessageSeverity::Error).count() as u32;
        stats.validation_warnings = messages.iter().filter(|m| m.severity == MessageSeverity::Warning).count() as u32;
        Ok(RenderOutput { image, targets, validation_messages: messages.iter().map(|m| m.render()).collect(), stats })
    }
}

impl Drop for Executor<'_> {
    fn drop(&mut self) {
        self.arena.destroy_all(self.gpu);
    }
}

/// Settings of [`render_to_png`] (what the `shaderbridge render` command exposes).
#[derive(Debug, Clone)]
pub struct PngRenderSettings {
    /// Output PNG path (parent directories are created).
    pub output: PathBuf,
    /// Image width.
    pub width: u32,
    /// Image height.
    pub height: u32,
    /// Frames to render.
    pub frames: u32,
    /// World folder (`world0`, `""`, ...).
    pub dimension: String,
    /// Scene and camera (`--time` sets [`SceneParams::world_time`]).
    pub scene: SceneParams,
    /// Depth mode; `None` = the mode the pack was compiled for.
    pub depth_mode: Option<DepthMode>,
    /// Device selection and validation.
    pub runtime: RuntimeOptions,
    /// Also write every render target as `<dir>/<name>.png`.
    pub capture_dir: Option<PathBuf>,
}

impl Default for PngRenderSettings {
    fn default() -> Self {
        Self {
            output: PathBuf::from("render.png"),
            width: 1280,
            height: 720,
            frames: 3,
            dimension: "world0".into(),
            scene: SceneParams::default(),
            depth_mode: None,
            runtime: RuntimeOptions { validation: true, prefer_cpu_device: false, device_name_filter: None },
            capture_dir: None,
        }
    }
}

fn save_png(img: &image::RgbaImage, path: &Path) -> Result<(), RuntimeError> {
    let io = |e: String| RuntimeError::Io { path: path.display().to_string(), message: e };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| io(e.to_string()))?;
    }
    img.save_with_format(path, image::ImageFormat::Png).map_err(|e| io(e.to_string()))
}

/// Render a pack and write the result as a PNG (plus the targets when
/// `capture_dir` is set). If `dimension` does not exist, the pack root (`""`) or the
/// first dimension is used. Returns the render output.
pub fn render_to_png(pack: &CompiledPack, blobs: &BlobTable, textures: &dyn TextureSource, settings: &PngRenderSettings) -> Result<RenderOutput, RuntimeError> {
    let dimension = if pack.dimensions.iter().any(|d| d.folder == settings.dimension) {
        settings.dimension.clone()
    } else if pack.dimensions.iter().any(|d| d.folder.is_empty()) {
        String::new()
    } else {
        pack.dimensions.first().map(|d| d.folder.clone()).unwrap_or_default()
    };
    let mut rt = Runtime::new(&settings.runtime)?;
    let out = rt.render(&RenderRequest {
        pack,
        blobs,
        dimension: &dimension,
        width: settings.width,
        height: settings.height,
        frames: settings.frames,
        scene: settings.scene.clone(),
        depth_mode: settings.depth_mode.unwrap_or(pack.info.environment.depth_mode),
        textures,
        capture_targets: settings.capture_dir.is_some(),
    })?;
    save_png(&out.image, &settings.output)?;
    if let Some(dir) = &settings.capture_dir {
        for (name, img) in &out.targets {
            save_png(img, &dir.join(format!("{name}.png")))?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod lifetime_tests {
    use super::*;

    fn validating_runtime() -> Option<Runtime> {
        match Runtime::new(&RuntimeOptions { validation: true, prefer_cpu_device: true, device_name_filter: None }) {
            Ok(rt) if rt.device_info().validation => Some(rt),
            Ok(_) => {
                eprintln!("skipping: the validation layer is not installed");
                None
            }
            Err(e) => {
                eprintln!("skipping: {e}");
                None
            }
        }
    }

    /// Every render releases all of its GPU memory, and no Vulkan object outlives the
    /// device: the validation layer reports objects still alive at `vkDestroyDevice`
    /// into the message sink, which is checked after the runtime is dropped.
    #[test]
    fn renders_release_every_object_and_allocation() {
        let (pack, blobs) = crate::testutil::mini_pack();
        for round in 0..3 {
            let Some(mut rt) = validating_runtime() else { return };
            let sink = rt.gpu.message_sink();
            let baseline = rt.gpu.live_allocations();
            for frames in [1, 2] {
                let out = rt
                    .render(&RenderRequest {
                        pack: &pack,
                        blobs: &blobs,
                        dimension: "world0",
                        width: 32,
                        height: 24,
                        frames,
                        scene: SceneParams { render_distance: 1, dh_render_distance: 0, ..Default::default() },
                        depth_mode: DepthMode::ForwardZeroToOne,
                        textures: &NoTextures,
                        capture_targets: true,
                    })
                    .expect("render");
                assert!(out.validation_messages.is_empty(), "{:#?}", out.validation_messages);
                assert!(out.stats.programs_skipped.is_empty(), "{:?}", out.stats.programs_skipped);
                assert!(out.stats.pipelines_created >= 3 && out.stats.dispatches == 1, "{:?}", out.stats);
                assert_eq!(rt.gpu.live_allocations(), baseline, "round {round}: a render leaked GPU memory");
            }
            drop(rt);
            let leaks = sink.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
            assert!(leaks.is_empty(), "round {round}: objects alive at vkDestroyDevice: {leaks:#?}");
        }
    }
}

#[cfg(test)]
mod api_tests {
    use super::*;

    fn assert_send<T: Send>() {}

    #[test]
    fn runtime_is_send() {
        assert_send::<Runtime>();
        assert_send::<RenderOutput>();
    }

    #[test]
    fn png_settings_default_to_validation_and_pack_depth() {
        let s = PngRenderSettings::default();
        assert!(s.runtime.validation);
        assert_eq!(s.depth_mode, None);
        assert_eq!((s.width, s.height, s.frames), (1280, 720, 3));
    }
}
