//! Descriptor binding: what each reflected descriptor binds in the current pass (main
//! vs alt images, the draw's albedo texture, type-compatible fallbacks), feedback-loop
//! snapshots, descriptor pools and set writes.

use crate::error::{RuntimeError, VkResultExt};
use crate::executor::Executor;
use crate::pipelines::{Role, Slot, sample_type_matches};
use crate::resources::{ImageDesc, ImageId, SamplerKey, ViewKey};
use crate::texel::{self, NumericClass};
use crate::textures::TexDim;
use ash::vk;
use sb_compile::{DescriptorKind, ImageDim};
use sb_core::ScalarKind;
use sb_core::model::ResourceRef;

/// Per-draw-batch binding context.
#[derive(Debug, Clone)]
pub(crate) struct BindContext {
    /// World-geometry program (unknown samplers read the albedo texture, not colortex0).
    pub geometry: bool,
    /// Albedo texture (`gtexture`/`Sampler0`) of the batch.
    pub albedo: ImageId,
    /// Targets whose mip chain this program may use.
    pub mip_targets: Vec<u32>,
    /// Attachments of the current rendering scope: sampling them binds a snapshot.
    pub attachments: Vec<ImageId>,
}

/// A resolved image binding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ImageBinding {
    pub image: ImageId,
    pub view: ViewKey,
    pub sampler: Option<SamplerKey>,
}

/// What one descriptor element binds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Bound {
    Image(ImageBinding),
    Buffer { buffer: vk::Buffer, range: u64 },
    Dynamic { range: u32 },
}

fn required_view(dim: ImageDim, arrayed: bool) -> Option<vk::ImageViewType> {
    Some(match (dim, arrayed) {
        (ImageDim::D1, false) => vk::ImageViewType::TYPE_1D,
        (ImageDim::D1, true) => vk::ImageViewType::TYPE_1D_ARRAY,
        (ImageDim::D2 | ImageDim::Rect, false) => vk::ImageViewType::TYPE_2D,
        (ImageDim::D2 | ImageDim::Rect, true) => vk::ImageViewType::TYPE_2D_ARRAY,
        (ImageDim::D3, _) => vk::ImageViewType::TYPE_3D,
        (ImageDim::Cube, false) => vk::ImageViewType::CUBE,
        (ImageDim::Cube, true) => vk::ImageViewType::CUBE_ARRAY,
        _ => return None,
    })
}

fn class_of(t: ScalarKind) -> NumericClass {
    match t {
        ScalarKind::Int => NumericClass::Int,
        ScalarKind::Uint => NumericClass::Uint,
        _ => NumericClass::Float,
    }
}

const POOL_SETS: u32 = 512;

impl Executor<'_> {
    fn read_index(&self, i: u32) -> usize {
        self.flips.read(i)
    }

    /// Main (0) or alt (1) image of colortex `i` to read: the binding's `use_alt` for
    /// composite-style programs (warning when it disagrees with the pass flip state),
    /// the current flip state otherwise.
    fn color_read(&mut self, i: u32, ctx: &BindContext, use_alt: Option<bool>) -> usize {
        let state = self.read_index(i);
        match use_alt {
            Some(alt) if !ctx.geometry => {
                if usize::from(alt) != state {
                    self.warn(format!("colortex{i}: binding use_alt={alt} disagrees with the pass flip state; following use_alt"));
                }
                usize::from(alt)
            }
            _ => state,
        }
    }

    fn shadow_read_index(&self, i: u32) -> usize {
        self.flips.shadow_read(i)
    }

    fn linear_ok(&self, image: ImageId) -> bool {
        let f = self.arena.image(image).desc.format;
        texel::numeric_class(f) == NumericClass::Float && self.gpu.format_features(f).contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR)
    }

    /// The image (and sampler) a resource refers to for sampling, before compatibility
    /// checks. `None` = nothing sensible (a fallback is bound).
    fn sampled_candidate(&mut self, r: &ResourceRef, ctx: &BindContext, shadow_compare: bool, use_alt: Option<bool>) -> Option<(ImageId, bool, SamplerKey)> {
        let nearest = SamplerKey::NEAREST_CLAMP;
        let atlas_key = SamplerKey { linear: false, mipmapped: true, mip_linear: true, repeat: true, compare: None };
        let sh = &self.dim.targets.shadow;
        Some(match r {
            ResourceRef::ColorTex(i) | ResourceRef::ColorImage(i) => {
                if !self.targets.color.contains_key(i) {
                    self.warn(format!("colortex{i} is referenced but has no render target"));
                    return None;
                }
                let k = self.color_read(*i, ctx, use_alt);
                let pair = self.targets.color.get(i)?;
                let img = pair.images[k];
                let mip = pair.mipped && ctx.mip_targets.contains(i);
                (img, mip, SamplerKey { linear: self.linear_ok(img), mipmapped: mip, mip_linear: true, repeat: false, compare: None })
            }
            ResourceRef::DepthTex(i) => (self.targets.depth[(*i as usize).min(2)], false, nearest),
            ResourceRef::DhDepthTex(i) => (self.targets.dh_depth[(*i as usize).min(1)], false, nearest),
            ResourceRef::ShadowTex(i) | ResourceRef::ShadowTexHw(i) => {
                let k = (*i as usize).min(1);
                let img = self.targets.shadow[k];
                let linear = !sh.nearest[k] && self.gpu.format_features(vk::Format::D32_SFLOAT).contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR);
                let compare = shadow_compare.then_some(self.depth.compare);
                (img, false, SamplerKey { linear, mipmapped: false, mip_linear: false, repeat: false, compare })
            }
            ResourceRef::ShadowColor(i) | ResourceRef::ShadowColorImage(i) => {
                let Some(pair) = self.targets.shadow_color.get(i) else {
                    self.warn(format!("shadowcolor{i} is referenced but has no render target"));
                    return None;
                };
                let img = pair.images[self.shadow_read_index(*i)];
                let near = sh.color_nearest.get(*i as usize).copied().unwrap_or(false);
                (img, pair.mipped, SamplerKey { linear: !near && self.linear_ok(img), mipmapped: pair.mipped, mip_linear: true, repeat: false, compare: None })
            }
            ResourceRef::Noise => (self.tex.noise, false, SamplerKey { linear: true, mipmapped: false, mip_linear: false, repeat: true, compare: None }),
            ResourceRef::Atlas => self.albedo_binding(ctx.albedo),
            ResourceRef::Normals => (self.tex.normals, true, atlas_key),
            ResourceRef::Specular => (self.tex.specular, true, atlas_key),
            ResourceRef::Lightmap => (self.tex.lightmap, false, SamplerKey::LINEAR_CLAMP),
            ResourceRef::Overlay => (self.tex.overlay, false, nearest),
            ResourceRef::DhBlockAtlas => (self.tex.dh_atlas, false, nearest),
            ResourceRef::White => (self.tex.white, false, nearest),
            ResourceRef::CustomTexture(id) => match self.tex.custom.get(id) {
                Some((img, key)) => (*img, false, *key),
                None => {
                    self.warn(format!("custom texture `{id}` is not loaded; binding white"));
                    (self.tex.white, false, nearest)
                }
            },
            ResourceRef::Image(name) => match self.tex.images.get(name) {
                Some(img) => (*img, false, SamplerKey { linear: self.linear_ok(*img), ..SamplerKey::LINEAR_CLAMP }),
                None => {
                    self.warn(format!("custom image `{name}` does not exist"));
                    return None;
                }
            },
            ResourceRef::Unknown(name) => {
                if ctx.geometry {
                    self.albedo_binding(ctx.albedo)
                } else {
                    let _ = name;
                    let pair = self.targets.color.get(&0)?;
                    let img = pair.images[self.read_index(0)];
                    (img, false, SamplerKey { linear: self.linear_ok(img), ..SamplerKey::LINEAR_CLAMP })
                }
            }
            ResourceRef::Ssbo(_) | ResourceRef::UniformBlock(_) => {
                self.warn(format!("{r:?} is bound to a sampler"));
                return None;
            }
        })
    }

    fn albedo_binding(&self, albedo: ImageId) -> (ImageId, bool, SamplerKey) {
        if albedo == self.tex.atlas {
            (albedo, true, SamplerKey { linear: false, mipmapped: true, mip_linear: true, repeat: true, compare: None })
        } else {
            (albedo, false, SamplerKey { linear: false, mipmapped: false, mip_linear: false, repeat: true, compare: None })
        }
    }

    /// A 1x1 fallback texture for a view type / class: black `(0, 0, 0, 1)` like an
    /// incomplete GL texture (zero for integer samplers), or a depth image at the far
    /// value for comparison samplers (so lookups pass, i.e. "lit").
    fn fallback(&mut self, ty: vk::ImageViewType, class: NumericClass, depth: bool) -> Result<ImageId, RuntimeError> {
        if let Some(id) = self.tex.fallbacks.get(&(ty, class, depth)) {
            return Ok(*id);
        }
        let (dim, cube) = match ty {
            vk::ImageViewType::TYPE_1D | vk::ImageViewType::TYPE_1D_ARRAY => (TexDim::D1, false),
            vk::ImageViewType::TYPE_3D => (TexDim::D3, false),
            vk::ImageViewType::CUBE | vk::ImageViewType::CUBE_ARRAY => (TexDim::D2, true),
            _ => (TexDim::D2, false),
        };
        let (format, clear) = if depth {
            (vk::Format::D32_SFLOAT, [self.depth.clear; 4])
        } else {
            match class {
                NumericClass::Float => (vk::Format::R8G8B8A8_UNORM, [0.0, 0.0, 0.0, 1.0]),
                c => (texel::rgba32_of(c), [0.0; 4]),
            }
        };
        let usage = vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST;
        let desc = ImageDesc { name: format!("fallback {ty:?} {class:?}{}", if depth { " depth" } else { "" }), format, extent: [1, 1, 1], dim, mip_levels: 1, layers: if cube { 6 } else { 1 }, cube, usage, clear };
        let id = self.arena.create_image(self.gpu, desc)?;
        self.tex.fallbacks.insert((ty, class, depth), id);
        Ok(id)
    }

    fn storage_fallback(&mut self, ty: vk::ImageViewType, format: vk::Format) -> Result<ImageId, RuntimeError> {
        if let Some(id) = self.tex.storage_fallbacks.get(&(format, ty)) {
            return Ok(*id);
        }
        let dim = match ty {
            vk::ImageViewType::TYPE_1D | vk::ImageViewType::TYPE_1D_ARRAY => TexDim::D1,
            vk::ImageViewType::TYPE_3D => TexDim::D3,
            _ => TexDim::D2,
        };
        let usage = vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST;
        let desc = ImageDesc { name: format!("storage fallback {format:?}"), format, extent: [1, 1, 1], dim, mip_levels: 1, layers: 1, cube: false, usage, clear: [0.0; 4] };
        let id = self.arena.create_image(self.gpu, desc)?;
        self.tex.storage_fallbacks.insert((format, ty), id);
        Ok(id)
    }

    /// Resolve a sampled descriptor to a compatible image.
    fn resolve_sampled(&mut self, slot: &Slot, ctx: &BindContext) -> Result<ImageBinding, RuntimeError> {
        let (dim, arrayed, shadow, sample_type, with_sampler) = match slot.kind {
            DescriptorKind::CombinedImageSampler { dim, arrayed, shadow, sample_type, .. } => (dim, arrayed, shadow, sample_type, true),
            DescriptorKind::SampledImage { dim, arrayed, sample_type, .. } => (dim, arrayed, false, sample_type, false),
            _ => (ImageDim::D2, false, false, ScalarKind::Float, true),
        };
        let ty = required_view(dim, arrayed).unwrap_or(vk::ImageViewType::TYPE_2D);
        let class = class_of(sample_type);
        let r = match &slot.role {
            Role::Resource(r) => r.clone(),
            _ => ResourceRef::Unknown(slot.name.clone()),
        };
        let candidate = self.sampled_candidate(&r, ctx, shadow, slot.use_alt);
        let compatible = candidate.filter(|(img, _, _)| {
            let i = self.arena.image(*img);
            i.supports_view(ty) && sample_type_matches(sample_type, texel::numeric_class(i.desc.format)) && (!shadow || i.is_depth()) && i.desc.usage.contains(vk::ImageUsageFlags::SAMPLED)
        });
        let (image, all_levels, key) = match compatible {
            Some(c) => c,
            None => {
                if let Some((img, _, _)) = candidate {
                    let i = self.arena.image(img);
                    let msg = format!(
                        "`{}` ({:?}) expects a {ty:?} {class:?}{} texture but {} is {:?} {:?}; a fallback is bound",
                        slot.name,
                        r,
                        if shadow { " depth" } else { "" },
                        i.desc.name,
                        i.natural_view_type(),
                        i.desc.format
                    );
                    self.warn(msg);
                }
                let img = self.fallback(ty, class, shadow)?;
                let compare = shadow.then_some(self.depth.compare);
                (img, false, SamplerKey { compare, ..SamplerKey::NEAREST_CLAMP })
            }
        };
        let image = self.substitute(image, ctx)?;
        Ok(ImageBinding { image, view: ViewKey { ty, all_levels }, sampler: with_sampler.then_some(key) })
    }

    /// Resolve a storage-image descriptor.
    fn resolve_storage(&mut self, slot: &Slot, ctx: &BindContext) -> Result<ImageBinding, RuntimeError> {
        let DescriptorKind::StorageImage { dim, arrayed, format, sample_type, .. } = &slot.kind else {
            return Err(RuntimeError::InvalidRequest("not a storage image".into()));
        };
        let ty = required_view(*dim, *arrayed).unwrap_or(vk::ImageViewType::TYPE_2D);
        let declared = format.as_deref().and_then(sb_core::TextureFormat::from_glsl_image_format).map(|f| vk::Format::from_raw(f.vk_format_renderable() as i32));
        let candidate = match &slot.role {
            Role::Resource(ResourceRef::ColorImage(i) | ResourceRef::ColorTex(i)) => {
                let k = self.color_read(*i, ctx, slot.use_alt);
                self.targets.color.get(i).map(|p| p.images[k])
            }
            Role::Resource(ResourceRef::ShadowColorImage(i) | ResourceRef::ShadowColor(i)) => self.targets.shadow_color.get(i).map(|p| p.images[self.shadow_read_index(*i)]),
            Role::Resource(ResourceRef::Image(name)) => self.tex.images.get(name).copied(),
            _ => None,
        };
        let ok = candidate.filter(|img| {
            let i = self.arena.image(*img);
            i.desc.usage.contains(vk::ImageUsageFlags::STORAGE) && i.supports_view(ty) && sample_type_matches(*sample_type, texel::numeric_class(i.desc.format))
        });
        let image = match ok {
            Some(img) => {
                if let Some(d) = declared
                    && self.arena.image(img).desc.format != d
                {
                    let msg = format!("storage image `{}` is declared as {} but its target has format {:?}", slot.name, format.as_deref().unwrap_or("?"), self.arena.image(img).desc.format);
                    self.warn(msg);
                }
                img
            }
            None => {
                self.warn(format!("storage image `{}` ({:?}) has no compatible image; a fallback is bound", slot.name, slot.role));
                let f = declared.unwrap_or_else(|| texel::rgba32_of(class_of(*sample_type)));
                self.storage_fallback(ty, f)?
            }
        };
        let image = self.substitute(image, ctx)?;
        Ok(ImageBinding { image, view: ViewKey { ty, all_levels: false }, sampler: None })
    }

    /// Replace an attachment of the current scope by its snapshot.
    fn substitute(&mut self, image: ImageId, ctx: &BindContext) -> Result<ImageId, RuntimeError> {
        if !ctx.attachments.contains(&image) {
            return Ok(image);
        }
        match self.targets.snapshots.get(&image) {
            Some(s) => Ok(*s),
            None => Ok(image),
        }
    }

    /// Images a program samples or stores to with `ctx` (before snapshot substitution).
    pub(crate) fn bound_images(&mut self, program: u32, ctx: &BindContext) -> Result<Vec<ImageId>, RuntimeError> {
        let Some(p) = self.program(program) else { return Ok(Vec::new()) };
        let slots = p.slots.clone();
        let ctx = BindContext { attachments: Vec::new(), ..ctx.clone() };
        let mut out = Vec::new();
        for s in &slots {
            match s.kind {
                DescriptorKind::CombinedImageSampler { .. } | DescriptorKind::SampledImage { .. } => out.push(self.resolve_sampled(s, &ctx)?.image),
                DescriptorKind::StorageImage { .. } => out.push(self.resolve_storage(s, &ctx)?.image),
                _ => {}
            }
        }
        Ok(out)
    }

    /// Make sure every attachment in `images` has a snapshot, and record copies of the
    /// attachments' current contents into them.
    pub(crate) fn snapshot(&mut self, cmd: vk::CommandBuffer, images: &[ImageId]) -> Result<(), RuntimeError> {
        if images.is_empty() {
            return Ok(());
        }
        let mut copies = Vec::new();
        for &img in images {
            let snap = match self.targets.snapshots.get(&img) {
                Some(s) => *s,
                None => {
                    let src = self.arena.image(img);
                    let mut desc = src.desc.clone();
                    desc.name = format!("{} (snapshot)", desc.name);
                    desc.usage &= vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::STORAGE;
                    desc.usage |= vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::TRANSFER_SRC;
                    let s = self.arena.create_image(self.gpu, desc)?;
                    self.targets.snapshots.insert(img, s);
                    s
                }
            };
            copies.push((img, snap));
        }
        let d = &self.gpu.device;
        crate::resources::full_barrier(d, cmd);
        for (src, dst) in copies {
            let (s, t) = (self.arena.image(src), self.arena.image(dst));
            let regions: Vec<vk::ImageCopy> = (0..s.desc.mip_levels)
                .map(|level| {
                    let sub = vk::ImageSubresourceLayers { aspect_mask: s.aspect, mip_level: level, base_array_layer: 0, layer_count: s.desc.layers };
                    vk::ImageCopy {
                        src_subresource: sub,
                        src_offset: vk::Offset3D::default(),
                        dst_subresource: sub,
                        dst_offset: vk::Offset3D::default(),
                        extent: vk::Extent3D { width: (s.desc.extent[0] >> level).max(1), height: (s.desc.extent[1] >> level).max(1), depth: (s.desc.extent[2] >> level).max(1) },
                    }
                })
                .collect();
            unsafe { d.cmd_copy_image(cmd, s.image, vk::ImageLayout::GENERAL, t.image, vk::ImageLayout::GENERAL, &regions) };
        }
        crate::resources::full_barrier(d, cmd);
        Ok(())
    }

    /// Allocate descriptor sets for `layouts` from the frame's pools.
    fn allocate_sets(&mut self, layouts: &[vk::DescriptorSetLayout]) -> Result<Vec<vk::DescriptorSet>, RuntimeError> {
        if layouts.is_empty() {
            return Ok(Vec::new());
        }
        loop {
            let mut created_now = false;
            if self.pool_cursor >= self.pools.len() {
                let sizes = [
                    vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC, descriptor_count: POOL_SETS * 8 },
                    vk::DescriptorPoolSize { ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, descriptor_count: POOL_SETS * 24 },
                    vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: POOL_SETS * 2 },
                    vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: POOL_SETS * 2 },
                    vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_IMAGE, descriptor_count: POOL_SETS * 4 },
                    vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_BUFFER, descriptor_count: POOL_SETS * 4 },
                ];
                let info = vk::DescriptorPoolCreateInfo::default().max_sets(POOL_SETS).pool_sizes(&sizes);
                let pool = unsafe { self.gpu.device.create_descriptor_pool(&info, None) }.vk("vkCreateDescriptorPool")?;
                self.arena.descriptor_pools.push(pool);
                self.pools.push(pool);
                created_now = true;
            }
            let pool = self.pools[self.pool_cursor];
            let info = vk::DescriptorSetAllocateInfo::default().descriptor_pool(pool).set_layouts(layouts);
            match unsafe { self.gpu.device.allocate_descriptor_sets(&info) } {
                Ok(sets) => return Ok(sets),
                Err(vk::Result::ERROR_OUT_OF_POOL_MEMORY | vk::Result::ERROR_FRAGMENTED_POOL) if !created_now => self.pool_cursor += 1,
                Err(vk::Result::ERROR_OUT_OF_POOL_MEMORY | vk::Result::ERROR_FRAGMENTED_POOL) => {
                    return Err(RuntimeError::Unsupported("a program needs more descriptors than a descriptor pool holds".into()));
                }
                Err(e) => return Err(RuntimeError::vk("vkAllocateDescriptorSets", e)),
            }
        }
    }

    /// Reset all descriptor pools (start of a frame).
    pub(crate) fn reset_pools(&mut self) {
        for p in &self.pools {
            let _ = unsafe { self.gpu.device.reset_descriptor_pool(*p, vk::DescriptorPoolResetFlags::empty()) };
        }
        self.pool_cursor = 0;
    }

    /// Allocate and write the descriptor sets of `program` for `ctx`.
    pub(crate) fn write_sets(&mut self, program: u32, ctx: &BindContext) -> Result<Vec<vk::DescriptorSet>, RuntimeError> {
        let Some(p) = self.program(program) else { return Ok(Vec::new()) };
        let layouts = p.set_layouts.clone();
        let slots = p.slots.clone();
        let sets = self.allocate_sets(&layouts)?;
        let mut bound: Vec<(usize, Vec<Bound>)> = Vec::new();
        for (k, s) in slots.iter().enumerate() {
            let one = match s.kind {
                DescriptorKind::UniformBuffer { .. } => Bound::Dynamic { range: s.range },
                DescriptorKind::CombinedImageSampler { .. } | DescriptorKind::SampledImage { .. } => Bound::Image(self.resolve_sampled(s, ctx)?),
                DescriptorKind::Sampler => Bound::Image(ImageBinding { image: self.tex.white, view: ViewKey { ty: vk::ImageViewType::TYPE_2D, all_levels: false }, sampler: Some(SamplerKey::LINEAR_CLAMP) }),
                DescriptorKind::StorageImage { .. } => Bound::Image(self.resolve_storage(s, ctx)?),
                DescriptorKind::StorageBuffer { size, .. } => self.resolve_ssbo(s, u64::from(size))?,
                _ => continue,
            };
            bound.push((k, vec![one; s.count as usize]));
        }
        // Materialize views/samplers, then write.
        let mut image_infos: Vec<Vec<vk::DescriptorImageInfo>> = Vec::new();
        let mut buffer_infos: Vec<Vec<vk::DescriptorBufferInfo>> = Vec::new();
        let ring = self.arena.buffer(self.ring.buffer).buffer;
        let mut plan = Vec::new();
        for (k, elems) in &bound {
            let s = &slots[*k];
            match elems[0] {
                Bound::Image(_) => {
                    let mut infos = Vec::new();
                    for e in elems {
                        let Bound::Image(b) = e else { continue };
                        let view = if matches!(s.kind, DescriptorKind::Sampler) { vk::ImageView::null() } else { self.arena.view(self.gpu, b.image, b.view)? };
                        let sampler = match b.sampler {
                            Some(key) => self.arena.sampler(self.gpu, key)?,
                            None => vk::Sampler::null(),
                        };
                        infos.push(vk::DescriptorImageInfo { sampler, image_view: view, image_layout: vk::ImageLayout::GENERAL });
                    }
                    image_infos.push(infos);
                    plan.push((*k, true, image_infos.len() - 1));
                }
                Bound::Buffer { buffer, range } => {
                    buffer_infos.push(vec![vk::DescriptorBufferInfo { buffer, offset: 0, range }; elems.len()]);
                    plan.push((*k, false, buffer_infos.len() - 1));
                }
                Bound::Dynamic { range } => {
                    buffer_infos.push(vec![vk::DescriptorBufferInfo { buffer: ring, offset: 0, range: u64::from(range) }; elems.len()]);
                    plan.push((*k, false, buffer_infos.len() - 1));
                }
            }
        }
        let writes: Vec<vk::WriteDescriptorSet<'_>> = plan
            .iter()
            .filter_map(|&(k, image, idx)| {
                let s = &slots[k];
                let set = *sets.get(s.set as usize)?;
                let w = vk::WriteDescriptorSet::default().dst_set(set).dst_binding(s.binding).descriptor_type(s.vk_type());
                Some(if image { w.image_info(&image_infos[idx]) } else { w.buffer_info(&buffer_infos[idx]) })
            })
            .collect();
        unsafe { self.gpu.device.update_descriptor_sets(&writes, &[]) };
        Ok(sets)
    }

    fn resolve_ssbo(&mut self, slot: &Slot, declared: u64) -> Result<Bound, RuntimeError> {
        let found = match &slot.role {
            Role::Resource(ResourceRef::Ssbo(i)) => self.tex.ssbos.get(i).copied(),
            _ => None,
        };
        let id = match found {
            Some(b) if self.arena.buffer(b).size >= declared => b,
            other => {
                if other.is_some() {
                    self.warn(format!("SSBO `{}` is smaller than its declared {declared} bytes; a zero buffer is bound", slot.name));
                } else {
                    self.warn(format!("storage buffer `{}` ({:?}) has no buffer; a zero buffer is bound", slot.name, slot.role));
                }
                if self.arena.buffer(self.tex.zero).size >= declared {
                    self.tex.zero
                } else {
                    let size = declared.next_power_of_two().min(1 << 28);
                    let b = self.arena.create_buffer_with_data(self.gpu, "zero storage buffer", &[], size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
                    self.tex.zero = b;
                    b
                }
            }
        };
        let buf = self.arena.buffer(id);
        Ok(Bound::Buffer { buffer: buf.buffer, range: buf.size })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_types() {
        assert_eq!(required_view(ImageDim::D2, false), Some(vk::ImageViewType::TYPE_2D));
        assert_eq!(required_view(ImageDim::Rect, false), Some(vk::ImageViewType::TYPE_2D));
        assert_eq!(required_view(ImageDim::Cube, true), Some(vk::ImageViewType::CUBE_ARRAY));
        assert_eq!(required_view(ImageDim::Buffer, false), None);
        assert_eq!(class_of(ScalarKind::Uint), NumericClass::Uint);
        assert_eq!(class_of(ScalarKind::Bool), NumericClass::Float);
    }
}
