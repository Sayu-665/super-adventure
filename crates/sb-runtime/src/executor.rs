//! Per-render state and its setup: render targets, textures, custom resources, scene
//! buffers, prepared programs and the uniform/descriptor allocators.

use crate::RenderRequest;
use crate::device::Gpu;
use crate::error::RuntimeError;
use crate::flips::Flips;
use crate::pipelines::{self, PreparedProgram};
use crate::resources::{Arena, BufferId, ImageDesc, ImageId, SamplerKey};
use crate::scene::entity::EntityInstance;
use crate::scene::formats::VertexLayout;
use crate::scene::{CpuMesh, CpuScene, SubDraw};
use crate::stats::FrameStats;
use crate::texel;
use crate::textures::{self, TexDim, TextureData};
use ash::vk;
use gpu_allocator::MemoryLocation;
use sb_core::model::{ColorTarget, DepthMode, DhStrategy, DimensionPipeline, ImageSize, ResourceRef, TargetSize};
use sb_expr::CustomUniforms;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Depth convention of the render.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DepthConfig {
    pub mode: DepthMode,
    pub compare: vk::CompareOp,
    /// Clear value of depth attachments (the far value).
    pub clear: f32,
    pub neg_one_to_one: bool,
}

impl DepthConfig {
    pub fn new(mode: DepthMode, gpu: &Gpu) -> Result<Self, RuntimeError> {
        Ok(match mode {
            DepthMode::ForwardZeroToOne => Self { mode, compare: vk::CompareOp::LESS_OR_EQUAL, clear: 1.0, neg_one_to_one: false },
            DepthMode::ReversedZeroToOne => Self { mode, compare: vk::CompareOp::GREATER_OR_EQUAL, clear: 0.0, neg_one_to_one: false },
            DepthMode::GlNegOneToOne => {
                if !gpu.depth_clip_control {
                    return Err(RuntimeError::Unsupported("DepthMode::GlNegOneToOne needs VK_EXT_depth_clip_control".into()));
                }
                Self { mode, compare: vk::CompareOp::LESS_OR_EQUAL, clear: 1.0, neg_one_to_one: true }
            }
        })
    }

    /// Convert a stored depth value to GL window depth (what packs see).
    pub fn gl_depth(self, d: f32) -> f32 {
        match self.mode {
            DepthMode::ReversedZeroToOne => 1.0 - d,
            _ => d,
        }
    }
}

/// A main/alt pair of a colour target.
#[derive(Debug, Clone)]
pub(crate) struct ColorPair {
    pub images: [ImageId; 2],
    pub format: vk::Format,
    pub clear: bool,
    /// `None` = the Iris default for the index (fog colour for colortex0).
    pub clear_color: Option<[f32; 4]>,
    pub mipped: bool,
}

/// Render targets.
pub(crate) struct Targets {
    pub color: BTreeMap<u32, ColorPair>,
    pub shadow_color: BTreeMap<u32, ColorPair>,
    /// depthtex0 (the depth attachment), depthtex1, depthtex2.
    pub depth: [ImageId; 3],
    /// DH depth attachment (= dhDepthTex0) and dhDepthTex1.
    pub dh_depth: [ImageId; 2],
    /// shadowtex0 (attachment) and shadowtex1.
    pub shadow: [ImageId; 2],
    pub output: ImageId,
    /// Write-only attachments for outputs without a valid target, per format and slot.
    pub sinks: HashMap<(vk::Format, usize), ImageId>,
    /// Copies of attachments that are also sampled in the same pass.
    pub snapshots: HashMap<ImageId, ImageId>,
    pub extent: vk::Extent2D,
}

/// A mesh in GPU buffers.
pub(crate) struct GpuMesh {
    pub vertex: BufferId,
    pub index: BufferId,
    pub draws: Vec<SubDraw>,
    pub layout: &'static VertexLayout,
}

/// The terrain in Sodium's format: per layer (solid, cutout, translucent) one sub-draw
/// per region.
pub(crate) struct GpuSodium {
    pub layers: [Option<GpuMesh>; 3],
    /// Region of each sub-draw of each layer (index into `regions`).
    pub draw_regions: [Vec<usize>; 3],
    pub regions: Vec<crate::scene::terrain::SodiumRegion>,
}

/// The scene in GPU buffers.
pub(crate) struct GpuScene {
    pub camera: [f64; 3],
    pub solid: Option<GpuMesh>,
    pub cutout: Option<GpuMesh>,
    pub water: Option<GpuMesh>,
    pub instances: Option<BufferId>,
    pub entities: Option<GpuMesh>,
    pub entity_list: Vec<EntityInstance>,
    pub sky_disc: Option<GpuMesh>,
    pub sun: Option<GpuMesh>,
    pub moon: Option<GpuMesh>,
    pub dh: Vec<([i32; 3], Option<GpuMesh>, Option<GpuMesh>)>,
    /// Present when a program was translated for the `sodium_terrain` profile.
    pub sodium: Option<GpuSodium>,
}

/// Textures and other bound resources.
pub(crate) struct Textures {
    pub atlas: ImageId,
    pub normals: ImageId,
    pub specular: ImageId,
    pub lightmap: ImageId,
    pub overlay: ImageId,
    pub noise: ImageId,
    pub dh_atlas: ImageId,
    pub white: ImageId,
    pub entity: ImageId,
    pub sun: ImageId,
    pub moon: ImageId,
    /// `CustomTexture` id → image and sampler.
    pub custom: HashMap<String, (ImageId, SamplerKey)>,
    /// Custom images by name.
    pub images: HashMap<String, ImageId>,
    /// SSBOs by index.
    pub ssbos: HashMap<u32, BufferId>,
    /// 1x1 fallbacks by (view type, numeric class, depth).
    pub fallbacks: HashMap<(vk::ImageViewType, texel::NumericClass, bool), ImageId>,
    /// Storage-image fallbacks by (format, view type).
    pub storage_fallbacks: HashMap<(vk::Format, vk::ImageViewType), ImageId>,
    /// Zero buffer (fallback SSBO, unused instance data).
    pub zero: BufferId,
    /// GL default vertex attribute values (missing vertex inputs).
    pub attribute_defaults: BufferId,
}

/// Uniform ring buffer with dynamic offsets.
pub(crate) struct Ring {
    pub buffer: BufferId,
    pub capacity: u64,
    pub head: u64,
    pub align: u64,
    pub overflow: bool,
}

impl Ring {
    /// Reserve `size` bytes and copy `data` into them; returns the dynamic offset.
    pub fn push(&mut self, arena: &mut Arena, data: &[u8], size: u32) -> u32 {
        let size = u64::from(size).max(data.len() as u64).max(16);
        let start = self.head.div_ceil(self.align) * self.align;
        if start + size > self.capacity || start > u64::from(u32::MAX) {
            self.overflow = true;
            return 0;
        }
        if let Some(m) = arena.buffer_mut(self.buffer).mapped() {
            let dst = &mut m[start as usize..(start + size) as usize];
            dst.fill(0);
            dst[..data.len()].copy_from_slice(data);
        }
        self.head = start + size;
        start as u32
    }
}

/// Program preparation result.
pub(crate) enum ProgramState {
    Ready(Box<PreparedProgram>),
    /// The reason is in [`FrameStats::programs_skipped`].
    Skipped,
}

/// Everything one render needs.
pub(crate) struct Executor<'r> {
    pub gpu: &'r mut Gpu,
    pub arena: Arena,
    pub req: &'r RenderRequest<'r>,
    pub dim: &'r DimensionPipeline,
    pub stats: FrameStats,
    pub depth: DepthConfig,
    pub targets: Targets,
    pub tex: Textures,
    pub programs: Vec<ProgramState>,
    pub scene: GpuScene,
    pub ring: Ring,
    pub pools: Vec<vk::DescriptorPool>,
    pub pool_cursor: usize,
    pub custom: CustomUniforms,
    pub flips: Flips,
    pub center_depth: f32,
    pub center_buffer: BufferId,
    pub dh_enabled: bool,
    pub unified: bool,
    pub shadow_enabled: bool,
    warned: HashSet<String>,
}

/// Iris' default clear colour of a colour target.
pub(crate) fn default_clear(index: u32, shadow: bool, fog: [f32; 3]) -> [f32; 4] {
    if shadow {
        [1.0; 4]
    } else {
        match index {
            0 => [fog[0], fog[1], fog[2], 1.0],
            1 => [1.0; 4],
            _ => [0.0; 4],
        }
    }
}

fn target_format(gpu: &Gpu, t: &ColorTarget) -> (vk::Format, Option<String>) {
    let f = vk::Format::from_raw(t.format.vk_format_renderable() as i32);
    let need = vk::FormatFeatureFlags::COLOR_ATTACHMENT | vk::FormatFeatureFlags::SAMPLED_IMAGE | vk::FormatFeatureFlags::TRANSFER_DST | vk::FormatFeatureFlags::TRANSFER_SRC;
    if gpu.format_features(f).contains(need) && texel::layout(f).is_some() {
        return (f, None);
    }
    let fallback = match texel::numeric_class(f) {
        texel::NumericClass::Float => vk::Format::R16G16B16A16_SFLOAT,
        texel::NumericClass::Int => vk::Format::R32G32B32A32_SINT,
        texel::NumericClass::Uint => vk::Format::R32G32B32A32_UINT,
    };
    (fallback, Some(format!("format {} ({f:?}) is not renderable on this device; using {fallback:?}", t.format)))
}

impl<'r> Executor<'r> {
    pub fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if self.warned.insert(msg.clone()) {
            log::warn!("{msg}");
            self.stats.warnings.push(msg);
        }
    }

    /// Pipelines created by all programs.
    pub fn pipelines_created(&self) -> u32 {
        self.programs.iter().map(|p| if let ProgramState::Ready(p) = p { p.pipelines_created } else { 0 }).sum()
    }

    pub fn program(&mut self, index: u32) -> Option<&mut PreparedProgram> {
        match self.programs.get_mut(index as usize) {
            Some(ProgramState::Ready(p)) => Some(p),
            _ => None,
        }
    }

    pub fn new(gpu: &'r mut Gpu, req: &'r RenderRequest<'r>, dim: &'r DimensionPipeline) -> Result<Self, RuntimeError> {
        let depth = DepthConfig::new(req.depth_mode, gpu)?;
        let mut arena = Arena::default();
        let mut stats = FrameStats::default();
        let built = Self::build(gpu, &mut arena, req, dim, depth, &mut stats);
        match built {
            Ok(parts) => Ok(Self {
                gpu,
                arena,
                req,
                dim,
                stats,
                depth,
                targets: parts.targets,
                tex: parts.tex,
                programs: parts.programs,
                scene: parts.scene,
                ring: parts.ring,
                pools: Vec::new(),
                pool_cursor: 0,
                custom: parts.custom,
                flips: Flips::default(),
                center_depth: 1.0,
                center_buffer: parts.center_buffer,
                dh_enabled: parts.dh_enabled,
                unified: parts.unified,
                shadow_enabled: parts.shadow_enabled,
                warned: parts.warned,
            }),
            Err(e) => {
                arena.destroy_all(gpu);
                Err(e)
            }
        }
    }

    fn build(gpu: &mut Gpu, arena: &mut Arena, req: &RenderRequest<'_>, dim: &DimensionPipeline, depth: DepthConfig, stats: &mut FrameStats) -> Result<Parts, RuntimeError> {
        let mut warned = HashSet::new();
        let mut warn = |stats: &mut FrameStats, m: String| {
            if warned.insert(m.clone()) {
                stats.warnings.push(m);
            }
        };
        let (w, h) = (req.width, req.height);
        let extent = vk::Extent2D { width: w, height: h };
        let color_usage = vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST;
        let depth_usage = vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST;

        // Which colortex indices are referenced anywhere.
        let mut used: BTreeSet<u32> = dim.targets.colortex.iter().filter(|t| t.used).map(|t| t.index).collect();
        used.extend(dim.gbuffer_attachments.iter().copied());
        used.insert(0);
        for p in &dim.programs {
            let shadowish = matches!(p.kind, sb_core::model::ProgramKind::Geometry { program } if program.group() == sb_core::program::GeometryGroup::Shadow)
                || matches!(p.kind, sb_core::model::ProgramKind::Composite { group: sb_core::PassGroup::ShadowComp, .. });
            if !shadowish {
                used.extend(p.draw_buffers.iter().copied());
            }
            for b in &p.bindings_used {
                if let Some(e) = dim.bindings.get(&b.name)
                    && let ResourceRef::ColorTex(i) | ResourceRef::ColorImage(i) = e.resource
                {
                    used.insert(i);
                }
            }
        }
        for e in &dim.bindings.entries {
            if let ResourceRef::ColorTex(i) | ResourceRef::ColorImage(i) = e.resource {
                used.insert(i);
            }
        }
        used.retain(|&i| i < sb_uniforms::MAX_COLOR_TEX);
        let mipped_targets: BTreeSet<u32> = dim.programs.iter().flat_map(|p| p.mipmap_targets.iter().copied()).collect();
        let mut color = BTreeMap::new();
        for i in used {
            let default = ColorTarget { index: i, format: Default::default(), clear: true, clear_color: None, mipmap_programs: Vec::new(), size: TargetSize::default(), used: true };
            let t = dim.targets.colortex.iter().find(|t| t.index == i).unwrap_or(&default);
            let (format, note) = target_format(gpu, t);
            if let Some(n) = note {
                warn(stats, format!("colortex{i}: {n}"));
            }
            let (tw, th) = t.size.resolve(w, h);
            let (tw, th) = (tw.min(16384), th.min(16384));
            let mipped = !t.mipmap_programs.is_empty() || mipped_targets.contains(&i);
            let storage = gpu.format_features(format).contains(vk::FormatFeatureFlags::STORAGE_IMAGE);
            let usage = color_usage | if storage { vk::ImageUsageFlags::STORAGE } else { vk::ImageUsageFlags::empty() };
            let mut images = [ImageId(0); 2];
            for (k, img) in images.iter_mut().enumerate() {
                let mut d = ImageDesc::new_2d(format!("colortex{i}{}", if k == 0 { "" } else { " (alt)" }), format, tw, th, usage);
                d.mip_levels = if mipped { 32 } else { 1 };
                *img = arena.create_image(gpu, d)?;
            }
            color.insert(i, ColorPair { images, format, clear: t.clear, clear_color: t.clear_color, mipped });
        }

        // Shadow targets.
        let sh = &dim.targets.shadow;
        let shadow_enabled = sh.enabled;
        let shadow_res = if shadow_enabled { sh.resolution.clamp(16, 8192) } else { 1 };
        let mut shadow_used: BTreeSet<u32> = dim.targets.shadowcolor.iter().filter(|t| t.used).map(|t| t.index).collect();
        shadow_used.extend(dim.shadow_attachments.iter().copied());
        for p in &dim.programs {
            let shadowish = matches!(p.kind, sb_core::model::ProgramKind::Geometry { program } if program.group() == sb_core::program::GeometryGroup::Shadow)
                || matches!(p.kind, sb_core::model::ProgramKind::Composite { group: sb_core::PassGroup::ShadowComp, .. });
            if shadowish {
                shadow_used.extend(p.draw_buffers.iter().copied());
            }
        }
        for e in &dim.bindings.entries {
            if let ResourceRef::ShadowColor(i) | ResourceRef::ShadowColorImage(i) = e.resource {
                shadow_used.insert(i);
            }
        }
        shadow_used.retain(|&i| i < sb_uniforms::MAX_SHADOW_COLOR);
        let mut shadow_color = BTreeMap::new();
        for i in shadow_used {
            let default = ColorTarget { index: i, format: Default::default(), clear: true, clear_color: None, mipmap_programs: Vec::new(), size: TargetSize::default(), used: true };
            let t = dim.targets.shadowcolor.iter().find(|t| t.index == i).unwrap_or(&default);
            let (format, note) = target_format(gpu, t);
            if let Some(n) = note {
                warn(stats, format!("shadowcolor{i}: {n}"));
            }
            let mipped = sh.color_mipmap.get(i as usize).copied().unwrap_or(false);
            let storage = gpu.format_features(format).contains(vk::FormatFeatureFlags::STORAGE_IMAGE);
            let usage = color_usage | if storage { vk::ImageUsageFlags::STORAGE } else { vk::ImageUsageFlags::empty() };
            let mut images = [ImageId(0); 2];
            for (k, img) in images.iter_mut().enumerate() {
                let mut d = ImageDesc::new_2d(format!("shadowcolor{i}{}", if k == 0 { "" } else { " (alt)" }), format, shadow_res, shadow_res, usage);
                d.mip_levels = if mipped { 32 } else { 1 };
                d.clear = t.clear_color.unwrap_or([1.0; 4]);
                *img = arena.create_image(gpu, d)?;
            }
            shadow_color.insert(i, ColorPair { images, format, clear: t.clear, clear_color: t.clear_color, mipped });
        }
        let depth_img = |arena: &mut Arena, gpu: &mut Gpu, name: &str, w: u32, h: u32| {
            let mut d = ImageDesc::new_2d(name, vk::Format::D32_SFLOAT, w, h, depth_usage);
            d.clear = [depth.clear; 4];
            arena.create_image(gpu, d)
        };
        let depth_t = [depth_img(arena, gpu, "depthtex0", w, h)?, depth_img(arena, gpu, "depthtex1", w, h)?, depth_img(arena, gpu, "depthtex2", w, h)?];
        let dh_depth = [depth_img(arena, gpu, "dhDepthTex0", w, h)?, depth_img(arena, gpu, "dhDepthTex1", w, h)?];
        let shadow_t = [depth_img(arena, gpu, "shadowtex0", shadow_res, shadow_res)?, depth_img(arena, gpu, "shadowtex1", shadow_res, shadow_res)?];
        let output = arena.create_image(gpu, ImageDesc::new_2d("output", vk::Format::R8G8B8A8_UNORM, w, h, color_usage))?;
        let targets = Targets { color, shadow_color, depth: depth_t, dh_depth, shadow: shadow_t, output, sinks: HashMap::new(), snapshots: HashMap::new(), extent };

        // Textures.
        let scene_params = &req.scene;
        let daylight = {
            let sky = crate::math::celestial::sky_angle(scene_params.world_time.rem_euclid(24000));
            ((sky * std::f64::consts::TAU).cos() as f32 * 2.0 + 0.5).clamp(0.0, 1.0)
        };
        let lightmap_data = textures::lightmap(daylight);
        let upload = |arena: &mut Arena, gpu: &mut Gpu, name: &str, d: &TextureData| arena.upload_texture(gpu, name, d, vk::ImageUsageFlags::empty());
        let noise_data = match &dim.targets.noise_texture {
            Some(src) => match textures::load(src, req.textures, &|_| None) {
                Ok(d) => d,
                Err(e) => {
                    warn(stats, format!("texture.noise: {e}; using generated noise"));
                    textures::noise(dim.targets.noise_texture_resolution)
                }
            },
            None => textures::noise(dim.targets.noise_texture_resolution),
        };
        let mut tex = Textures {
            atlas: upload(arena, gpu, "block atlas", &textures::block_atlas())?,
            normals: upload(arena, gpu, "normals atlas", &textures::flat_atlas([128, 128, 255, 255]))?,
            specular: upload(arena, gpu, "specular atlas", &textures::flat_atlas([0, 0, 0, 0]))?,
            lightmap: upload(arena, gpu, "lightmap", &lightmap_data)?,
            overlay: upload(arena, gpu, "overlay", &textures::overlay())?,
            noise: upload(arena, gpu, "noisetex", &noise_data)?,
            dh_atlas: upload(arena, gpu, "DH block atlas", &textures::dh_atlas())?,
            white: upload(arena, gpu, "white", &textures::solid([255; 4]))?,
            entity: upload(arena, gpu, "entity texture", &textures::entity_texture())?,
            sun: upload(arena, gpu, "sun", &textures::sun_texture())?,
            moon: upload(arena, gpu, "moon phases", &textures::moon_texture())?,
            custom: HashMap::new(),
            images: HashMap::new(),
            ssbos: HashMap::new(),
            fallbacks: HashMap::new(),
            storage_fallbacks: HashMap::new(),
            zero: arena.create_buffer_with_data(gpu, "zero buffer", &[], 4096, vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::STORAGE_BUFFER)?,
            attribute_defaults: arena.create_buffer_with_data(gpu, "vertex attribute defaults", &pipelines::attribute_defaults(), 32, vk::BufferUsageFlags::VERTEX_BUFFER)?,
        };
        let dynamic = |name: &str| (name.contains("lightmap") || name.contains("light_map")).then(|| lightmap_data.clone());
        for ct in &dim.targets.custom_textures {
            let id = match &ct.source {
                sb_core::model::TextureSource::Raw { target, .. } => sb_uniforms::raw_texture_id(&ct.stage, &ct.sampler, target),
                _ => sb_uniforms::custom_texture_id(&ct.stage, &ct.sampler),
            };
            let key = SamplerKey { linear: ct.blur, mipmapped: false, mip_linear: false, repeat: !ct.clamp, compare: None };
            match textures::load(&ct.source, req.textures, &dynamic) {
                Ok(data) => {
                    let feats = gpu.format_features(data.format);
                    let key = SamplerKey { linear: key.linear && feats.contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR), ..key };
                    let img = arena.upload_texture(gpu, &format!("custom texture {id}"), &data, vk::ImageUsageFlags::empty())?;
                    tex.custom.insert(id, (img, key));
                }
                Err(e) => warn(stats, format!("custom texture `{id}`: {e}; a white texture is bound instead")),
            }
        }
        for ci in &dim.targets.images {
            let format = vk::Format::from_raw(ci.format.vk_format_renderable() as i32);
            let (dimension, [iw, ih, idp]) = match ci.size {
                ImageSize::Relative { x, y } => (TexDim::D2, [TargetSize::Relative { x, y }.resolve(w, h).0, TargetSize::Relative { x, y }.resolve(w, h).1, 1]),
                ImageSize::Absolute1D { width } => (TexDim::D1, [width, 1, 1]),
                ImageSize::Absolute2D { width, height } => (TexDim::D2, [width, height, 1]),
                ImageSize::Absolute3D { width, height, depth } => (TexDim::D3, [width, height, depth]),
            };
            if iw == 0 || ih == 0 || idp == 0 || iw > 16384 || ih > 16384 || idp > 2048 {
                warn(stats, format!("custom image `{}` has an unsupported size", ci.name));
                continue;
            }
            let feats = gpu.format_features(format);
            if !feats.contains(vk::FormatFeatureFlags::STORAGE_IMAGE) {
                warn(stats, format!("custom image `{}`: format {} cannot be a storage image here", ci.name, ci.format));
                continue;
            }
            let desc = ImageDesc {
                name: format!("image {}", ci.name),
                format,
                extent: [iw, ih, idp],
                dim: dimension,
                mip_levels: 1,
                layers: 1,
                cube: false,
                usage: vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::TRANSFER_SRC,
                clear: [0.0; 4],
            };
            let img = arena.create_image(gpu, desc)?;
            tex.images.insert(ci.name.clone(), img);
        }
        for b in &dim.targets.buffers {
            let size = match b.relative {
                Some([sx, sy]) => {
                    let (rw, rh) = TargetSize::Relative { x: sx, y: sy }.resolve(w, h);
                    b.size.saturating_mul(u64::from(rw)).saturating_mul(u64::from(rh))
                }
                None => b.size,
            };
            let max = u64::from(gpu.limits().max_storage_buffer_range).min(1 << 30);
            if size > max {
                warn(stats, format!("SSBO {} of {size} bytes exceeds the device limit; it is clamped to {max}", b.index));
            }
            let size = size.clamp(16, max);
            let data = match &b.file {
                Some(f) => match req.textures.read(f) {
                    Some(d) => d.into_iter().take(size as usize).collect(),
                    None => {
                        warn(stats, format!("SSBO {}: initial data `{f}` not found", b.index));
                        Vec::new()
                    }
                },
                None => Vec::new(),
            };
            let buf = arena.create_buffer_with_data(gpu, &format!("ssbo {}", b.index), &data, size, vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::INDIRECT_BUFFER | vk::BufferUsageFlags::TRANSFER_SRC)?;
            tex.ssbos.insert(b.index, buf);
        }

        // Scene.
        let dh_enabled = dim.distant_horizons.strategy != DhStrategy::Disabled && req.scene.clamped_dh_distance() > 0;
        let unified = dh_enabled && dim.distant_horizons.unified_projection;
        let with_sodium = dim.programs.iter().any(|p| p.draw_profile.as_deref() == Some(crate::scene::formats::SODIUM_TERRAIN.profile));
        let cpu = CpuScene::generate(&req.scene, &req.pack.id_maps, dh_enabled, with_sodium);
        let scene = upload_scene(gpu, arena, &cpu)?;

        // Programs.
        let mut programs = Vec::with_capacity(dim.programs.len());
        for p in &dim.programs {
            match pipelines::prepare(gpu, arena, dim, req.blobs, p) {
                Ok(pp) => programs.push(ProgramState::Ready(Box::new(pp))),
                Err(reason) => {
                    stats.skip_program(&p.name, &reason);
                    programs.push(ProgramState::Skipped);
                }
            }
        }

        // Uniform ring.
        let align = gpu.limits().min_uniform_buffer_offset_alignment.max(16);
        let capacity = 8 << 20;
        let ring_buf = arena.create_buffer(gpu, "uniform ring", capacity, vk::BufferUsageFlags::UNIFORM_BUFFER, MemoryLocation::CpuToGpu)?;
        let ring = Ring { buffer: ring_buf, capacity, head: 0, align, overflow: false };

        // Custom uniforms.
        let constants = crate::uniforms::expression_constants();
        let (mut custom, diags) = CustomUniforms::compile(&dim.custom_uniforms, &sb_uniforms::custom_uniform_input_type, &constants);
        custom.set_seed(req.scene.seed);
        for d in diags.iter().filter(|d| d.severity >= sb_core::Severity::Warning) {
            warn(stats, format!("custom uniforms: {}", d.message));
        }
        let center_buffer = arena.create_buffer(gpu, "center depth", 16, vk::BufferUsageFlags::TRANSFER_DST, MemoryLocation::GpuToCpu)?;
        Ok(Parts { targets, tex, programs, scene, ring, custom, center_buffer, dh_enabled, unified, shadow_enabled, warned })
    }
}

struct Parts {
    targets: Targets,
    tex: Textures,
    programs: Vec<ProgramState>,
    scene: GpuScene,
    ring: Ring,
    custom: CustomUniforms,
    center_buffer: BufferId,
    dh_enabled: bool,
    unified: bool,
    shadow_enabled: bool,
    warned: HashSet<String>,
}

fn upload_mesh(gpu: &mut Gpu, arena: &mut Arena, name: &str, m: &CpuMesh) -> Result<Option<GpuMesh>, RuntimeError> {
    if m.is_empty() {
        return Ok(None);
    }
    let vertex = arena.create_buffer_with_data(gpu, &format!("{name} vertices"), &m.vertices, 16, vk::BufferUsageFlags::VERTEX_BUFFER)?;
    let indices: Vec<u8> = m.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
    let index = arena.create_buffer_with_data(gpu, &format!("{name} indices"), &indices, 16, vk::BufferUsageFlags::INDEX_BUFFER)?;
    Ok(Some(GpuMesh { vertex, index, draws: m.draws.clone(), layout: m.layout }))
}

fn upload_scene(gpu: &mut Gpu, arena: &mut Arena, s: &CpuScene) -> Result<GpuScene, RuntimeError> {
    let instances = if s.terrain.instances.is_empty() {
        None
    } else {
        Some(arena.create_buffer_with_data(gpu, "chunk sections", &s.terrain.instances, 16, vk::BufferUsageFlags::VERTEX_BUFFER)?)
    };
    let mut dh = Vec::new();
    for (i, r) in s.dh.regions.iter().enumerate() {
        dh.push((r.origin, upload_mesh(gpu, arena, &format!("DH region {i}"), &r.opaque)?, upload_mesh(gpu, arena, &format!("DH water {i}"), &r.water)?));
    }
    let sodium = match &s.terrain.sodium {
        Some(m) => {
            let [a, b, c] = &m.layers;
            Some(GpuSodium {
                layers: [upload_mesh(gpu, arena, "Sodium terrain solid", a)?, upload_mesh(gpu, arena, "Sodium terrain cutout", b)?, upload_mesh(gpu, arena, "Sodium terrain translucent", c)?],
                draw_regions: m.draw_regions.clone(),
                regions: m.regions.clone(),
            })
        }
        None => None,
    };
    Ok(GpuScene {
        camera: s.camera,
        solid: upload_mesh(gpu, arena, "terrain solid", &s.terrain.solid)?,
        cutout: upload_mesh(gpu, arena, "terrain cutout", &s.terrain.cutout)?,
        water: upload_mesh(gpu, arena, "terrain translucent", &s.terrain.water)?,
        instances,
        entities: upload_mesh(gpu, arena, "entities", &s.entity_mesh)?,
        entity_list: s.entities.clone(),
        sky_disc: upload_mesh(gpu, arena, "sky disc", &s.sky.disc)?,
        sun: upload_mesh(gpu, arena, "sun", &s.sky.sun)?,
        moon: upload_mesh(gpu, arena, "moon", &s.sky.moon)?,
        dh,
        sodium,
    })
}
