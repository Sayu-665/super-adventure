//! GPU resources of one render: images (with lazily created views), buffers, samplers
//! and every other Vulkan object, owned by an [`Arena`] that destroys them all at the
//! end of the render (also on error paths).
//!
//! Every image lives in `VK_IMAGE_LAYOUT_GENERAL` for its whole life: it is valid for
//! attachments, sampling, storage and transfers, which keeps the executor free of layout
//! tracking. Synchronization uses full memory barriers between passes.

use crate::device::Gpu;
use crate::error::{RuntimeError, VkResultExt};
use crate::texel;
use crate::textures::{TexDim, TextureData};
use ash::vk;
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme};
use std::collections::HashMap;

/// Index of an image in the [`Arena`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct ImageId(pub usize);

/// Index of a buffer in the [`Arena`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct BufferId(pub usize);

/// Image creation parameters.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ImageDesc {
    pub name: String,
    pub format: vk::Format,
    pub extent: [u32; 3],
    pub dim: TexDim,
    pub mip_levels: u32,
    /// 6 for cube-compatible images.
    pub layers: u32,
    pub cube: bool,
    pub usage: vk::ImageUsageFlags,
    /// Initial clear value (colour as floats/ints by format class, depth in `[0]`).
    pub clear: [f32; 4],
}

impl ImageDesc {
    pub fn new_2d(name: impl Into<String>, format: vk::Format, width: u32, height: u32, usage: vk::ImageUsageFlags) -> Self {
        Self { name: name.into(), format, extent: [width.max(1), height.max(1), 1], dim: TexDim::D2, mip_levels: 1, layers: 1, cube: false, usage, clear: [0.0; 4] }
    }
}

/// Which view of an image a descriptor needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ViewKey {
    pub ty: vk::ImageViewType,
    /// All mip levels (`true`) or the base level only.
    pub all_levels: bool,
}

/// An image and its views.
pub(crate) struct GpuImage {
    pub image: vk::Image,
    allocation: Option<Allocation>,
    pub desc: ImageDesc,
    pub aspect: vk::ImageAspectFlags,
    views: HashMap<ViewKey, vk::ImageView>,
    /// Single-level 2D views (`level` → view), for rendering mip levels.
    level_views: HashMap<u32, vk::ImageView>,
}

impl GpuImage {
    /// The view type matching the image's own dimensionality.
    pub fn natural_view_type(&self) -> vk::ImageViewType {
        match (self.desc.dim, self.desc.cube) {
            (_, true) => vk::ImageViewType::CUBE,
            (TexDim::D1, _) => vk::ImageViewType::TYPE_1D,
            (TexDim::D2, _) => vk::ImageViewType::TYPE_2D,
            (TexDim::D3, _) => vk::ImageViewType::TYPE_3D,
        }
    }

    /// Whether a view of type `ty` can be created for this image.
    pub fn supports_view(&self, ty: vk::ImageViewType) -> bool {
        match ty {
            vk::ImageViewType::TYPE_1D | vk::ImageViewType::TYPE_1D_ARRAY => self.desc.dim == TexDim::D1,
            vk::ImageViewType::TYPE_2D | vk::ImageViewType::TYPE_2D_ARRAY => self.desc.dim == TexDim::D2 && (!self.desc.cube || ty == vk::ImageViewType::TYPE_2D_ARRAY),
            vk::ImageViewType::TYPE_3D => self.desc.dim == TexDim::D3,
            vk::ImageViewType::CUBE | vk::ImageViewType::CUBE_ARRAY => self.desc.cube,
            _ => false,
        }
    }

    pub fn is_depth(&self) -> bool {
        self.aspect.contains(vk::ImageAspectFlags::DEPTH)
    }

    pub fn extent_2d(&self) -> vk::Extent2D {
        vk::Extent2D { width: self.desc.extent[0], height: self.desc.extent[1] }
    }

    pub fn full_range(&self) -> vk::ImageSubresourceRange {
        vk::ImageSubresourceRange { aspect_mask: self.aspect, base_mip_level: 0, level_count: self.desc.mip_levels, base_array_layer: 0, layer_count: self.desc.layers }
    }
}

/// A buffer.
pub(crate) struct GpuBuffer {
    pub buffer: vk::Buffer,
    allocation: Option<Allocation>,
    pub size: u64,
}

impl GpuBuffer {
    /// The mapped contents (host-visible buffers only).
    pub fn mapped(&mut self) -> Option<&mut [u8]> {
        self.allocation.as_mut()?.mapped_slice_mut()
    }

    pub fn mapped_ref(&self) -> Option<&[u8]> {
        self.allocation.as_ref()?.mapped_slice()
    }
}

/// Sampler parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SamplerKey {
    /// Linear min/mag filter (nearest otherwise).
    pub linear: bool,
    /// The bound view has mip levels to use (otherwise `maxLod = 0`, mipmap mode nearest,
    /// matching GL's non-mipmap filters).
    pub mipmapped: bool,
    /// Linear filtering between mip levels.
    pub mip_linear: bool,
    pub repeat: bool,
    /// Depth comparison op (shadow samplers).
    pub compare: Option<vk::CompareOp>,
}

impl SamplerKey {
    pub const NEAREST_CLAMP: SamplerKey = SamplerKey { linear: false, mipmapped: false, mip_linear: false, repeat: false, compare: None };
    pub const LINEAR_CLAMP: SamplerKey = SamplerKey { linear: true, mipmapped: false, mip_linear: false, repeat: false, compare: None };
}

/// Owner of every Vulkan object of one render.
#[derive(Default)]
pub(crate) struct Arena {
    images: Vec<GpuImage>,
    buffers: Vec<GpuBuffer>,
    samplers: HashMap<SamplerKey, vk::Sampler>,
    pub modules: Vec<vk::ShaderModule>,
    pub set_layouts: Vec<vk::DescriptorSetLayout>,
    pub pipeline_layouts: Vec<vk::PipelineLayout>,
    pub pipelines: Vec<vk::Pipeline>,
    pub descriptor_pools: Vec<vk::DescriptorPool>,
    pub command_buffers: Vec<vk::CommandBuffer>,
}

fn is_depth_format(f: vk::Format) -> bool {
    matches!(f, vk::Format::D32_SFLOAT | vk::Format::D16_UNORM | vk::Format::D24_UNORM_S8_UINT | vk::Format::D32_SFLOAT_S8_UINT)
}

/// A clear value for `format`.
pub(crate) fn clear_color_value(format: vk::Format, c: [f32; 4]) -> vk::ClearColorValue {
    match texel::numeric_class(format) {
        texel::NumericClass::Float => vk::ClearColorValue { float32: c },
        texel::NumericClass::Int => vk::ClearColorValue { int32: c.map(|v| v as i32) },
        texel::NumericClass::Uint => vk::ClearColorValue { uint32: c.map(|v| v.max(0.0) as u32) },
    }
}

/// A full memory barrier (all commands, all memory accesses).
pub(crate) fn full_barrier(device: &ash::Device, cmd: vk::CommandBuffer) {
    let mb = [vk::MemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)
        .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)];
    unsafe {
        device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::ALL_COMMANDS, vk::PipelineStageFlags::ALL_COMMANDS, vk::DependencyFlags::empty(), &mb, &[], &[]);
    }
}

/// Make every device write visible to host reads after the submission's fence wait
/// (the device → host domain operation readbacks need; `ALL_COMMANDS` does not cover
/// the host stage).
pub(crate) fn host_read_barrier(device: &ash::Device, cmd: vk::CommandBuffer) {
    let mb = [vk::MemoryBarrier::default().src_access_mask(vk::AccessFlags::MEMORY_WRITE).dst_access_mask(vk::AccessFlags::HOST_READ)];
    unsafe {
        device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::ALL_COMMANDS, vk::PipelineStageFlags::HOST, vk::DependencyFlags::empty(), &mb, &[], &[]);
    }
}

impl Arena {
    pub fn image(&self, id: ImageId) -> &GpuImage {
        &self.images[id.0]
    }

    pub fn buffer(&self, id: BufferId) -> &GpuBuffer {
        &self.buffers[id.0]
    }

    pub fn buffer_mut(&mut self, id: BufferId) -> &mut GpuBuffer {
        &mut self.buffers[id.0]
    }

    /// Create an image, transition it to `GENERAL` and clear it to `desc.clear`.
    pub fn create_image(&mut self, gpu: &mut Gpu, desc: ImageDesc) -> Result<ImageId, RuntimeError> {
        let id = self.create_image_uninit(gpu, desc)?;
        let img = &self.images[id.0];
        let (image, range, format, clear, depth, can_clear) = (img.image, img.full_range(), img.desc.format, img.desc.clear, img.is_depth(), img.desc.usage.contains(vk::ImageUsageFlags::TRANSFER_DST));
        gpu.one_shot(|d, cmd| {
            init_layout(d, cmd, image, range);
            if can_clear {
                if depth {
                    let v = vk::ClearDepthStencilValue { depth: clear[0], stencil: 0 };
                    unsafe { d.cmd_clear_depth_stencil_image(cmd, image, vk::ImageLayout::GENERAL, &v, &[range]) };
                } else {
                    unsafe { d.cmd_clear_color_image(cmd, image, vk::ImageLayout::GENERAL, &clear_color_value(format, clear), &[range]) };
                }
            }
        })?;
        Ok(id)
    }

    fn create_image_uninit(&mut self, gpu: &mut Gpu, desc: ImageDesc) -> Result<ImageId, RuntimeError> {
        let (image_type, extent) = match desc.dim {
            TexDim::D1 => (vk::ImageType::TYPE_1D, vk::Extent3D { width: desc.extent[0].max(1), height: 1, depth: 1 }),
            TexDim::D2 => (vk::ImageType::TYPE_2D, vk::Extent3D { width: desc.extent[0].max(1), height: desc.extent[1].max(1), depth: 1 }),
            TexDim::D3 => (vk::ImageType::TYPE_3D, vk::Extent3D { width: desc.extent[0].max(1), height: desc.extent[1].max(1), depth: desc.extent[2].max(1) }),
        };
        let max_dim = extent.width.max(extent.height).max(extent.depth);
        let max_levels = 32 - max_dim.leading_zeros();
        let mut desc = desc;
        desc.mip_levels = desc.mip_levels.clamp(1, max_levels);
        desc.extent = [extent.width, extent.height, extent.depth];
        let flags = if desc.cube { vk::ImageCreateFlags::CUBE_COMPATIBLE } else { vk::ImageCreateFlags::empty() };
        let info = vk::ImageCreateInfo::default()
            .flags(flags)
            .image_type(image_type)
            .format(desc.format)
            .extent(extent)
            .mip_levels(desc.mip_levels)
            .array_layers(desc.layers.max(1))
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(desc.usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let image = unsafe { gpu.device.create_image(&info, None) }.vk("vkCreateImage")?;
        let req = unsafe { gpu.device.get_image_memory_requirements(image) };
        let allocation = match gpu.allocate(&AllocationCreateDesc { name: &desc.name, requirements: req, location: MemoryLocation::GpuOnly, linear: false, allocation_scheme: AllocationScheme::GpuAllocatorManaged }) {
            Ok(a) => a,
            Err(e) => {
                unsafe { gpu.device.destroy_image(image, None) };
                return Err(e);
            }
        };
        if let Err(e) = unsafe { gpu.device.bind_image_memory(image, allocation.memory(), allocation.offset()) } {
            unsafe { gpu.device.destroy_image(image, None) };
            gpu.free(allocation);
            return Err(RuntimeError::vk("vkBindImageMemory", e));
        }
        gpu.set_name(image, &desc.name);
        let aspect = if is_depth_format(desc.format) { vk::ImageAspectFlags::DEPTH } else { vk::ImageAspectFlags::COLOR };
        self.images.push(GpuImage { image, allocation: Some(allocation), desc, aspect, views: HashMap::new(), level_views: HashMap::new() });
        Ok(ImageId(self.images.len() - 1))
    }

    /// A view of `id` (created on first use).
    pub fn view(&mut self, gpu: &Gpu, id: ImageId, key: ViewKey) -> Result<vk::ImageView, RuntimeError> {
        let img = &mut self.images[id.0];
        if let Some(v) = img.views.get(&key) {
            return Ok(*v);
        }
        let layers = match key.ty {
            vk::ImageViewType::CUBE => 6,
            vk::ImageViewType::TYPE_2D | vk::ImageViewType::TYPE_1D | vk::ImageViewType::TYPE_3D => 1,
            _ => img.desc.layers.max(1),
        };
        let range = vk::ImageSubresourceRange {
            aspect_mask: img.aspect,
            base_mip_level: 0,
            level_count: if key.all_levels { img.desc.mip_levels } else { 1 },
            base_array_layer: 0,
            layer_count: layers,
        };
        let info = vk::ImageViewCreateInfo::default().image(img.image).view_type(key.ty).format(img.desc.format).subresource_range(range);
        let view = unsafe { gpu.device.create_image_view(&info, None) }.vk("vkCreateImageView")?;
        img.views.insert(key, view);
        Ok(view)
    }

    /// A 2D view of mip level `level` alone (created on first use), to render into that
    /// level or to sample exactly it.
    pub fn level_view(&mut self, gpu: &Gpu, id: ImageId, level: u32) -> Result<vk::ImageView, RuntimeError> {
        let img = &mut self.images[id.0];
        if let Some(v) = img.level_views.get(&level) {
            return Ok(*v);
        }
        if level >= img.desc.mip_levels || img.desc.dim != TexDim::D2 {
            return Err(RuntimeError::InvalidRequest(format!("{}: no 2D mip level {level}", img.desc.name)));
        }
        let range = vk::ImageSubresourceRange { aspect_mask: img.aspect, base_mip_level: level, level_count: 1, base_array_layer: 0, layer_count: 1 };
        let info = vk::ImageViewCreateInfo::default().image(img.image).view_type(vk::ImageViewType::TYPE_2D).format(img.desc.format).subresource_range(range);
        let view = unsafe { gpu.device.create_image_view(&info, None) }.vk("vkCreateImageView")?;
        img.level_views.insert(level, view);
        Ok(view)
    }

    /// The natural view with all or only the base level.
    pub fn default_view(&mut self, gpu: &Gpu, id: ImageId, all_levels: bool) -> Result<vk::ImageView, RuntimeError> {
        let ty = self.images[id.0].natural_view_type();
        self.view(gpu, id, ViewKey { ty, all_levels })
    }

    /// Create a buffer. Host-visible locations are persistently mapped.
    pub fn create_buffer(&mut self, gpu: &mut Gpu, name: &str, size: u64, usage: vk::BufferUsageFlags, location: MemoryLocation) -> Result<BufferId, RuntimeError> {
        let size = size.max(16);
        let info = vk::BufferCreateInfo::default().size(size).usage(usage).sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe { gpu.device.create_buffer(&info, None) }.vk("vkCreateBuffer")?;
        let req = unsafe { gpu.device.get_buffer_memory_requirements(buffer) };
        let allocation = match gpu.allocate(&AllocationCreateDesc { name, requirements: req, location, linear: true, allocation_scheme: AllocationScheme::GpuAllocatorManaged }) {
            Ok(a) => a,
            Err(e) => {
                unsafe { gpu.device.destroy_buffer(buffer, None) };
                return Err(e);
            }
        };
        if let Err(e) = unsafe { gpu.device.bind_buffer_memory(buffer, allocation.memory(), allocation.offset()) } {
            unsafe { gpu.device.destroy_buffer(buffer, None) };
            gpu.free(allocation);
            return Err(RuntimeError::vk("vkBindBufferMemory", e));
        }
        gpu.set_name(buffer, name);
        self.buffers.push(GpuBuffer { buffer, allocation: Some(allocation), size });
        Ok(BufferId(self.buffers.len() - 1))
    }

    /// Create a device-local buffer filled with `data` (zero-filled up to `min_size`).
    pub fn create_buffer_with_data(&mut self, gpu: &mut Gpu, name: &str, data: &[u8], min_size: u64, usage: vk::BufferUsageFlags) -> Result<BufferId, RuntimeError> {
        let size = (data.len() as u64).max(min_size).max(16);
        let id = self.create_buffer(gpu, name, size, usage | vk::BufferUsageFlags::TRANSFER_DST, MemoryLocation::GpuOnly)?;
        let staging = self.create_buffer(gpu, &format!("{name} (staging)"), size, vk::BufferUsageFlags::TRANSFER_SRC, MemoryLocation::CpuToGpu)?;
        if let Some(m) = self.buffers[staging.0].mapped() {
            m.fill(0);
            m[..data.len()].copy_from_slice(data);
        }
        let (dst, src) = (self.buffers[id.0].buffer, self.buffers[staging.0].buffer);
        let result = gpu.one_shot(|d, cmd| unsafe {
            d.cmd_copy_buffer(cmd, src, dst, &[vk::BufferCopy { src_offset: 0, dst_offset: 0, size }]);
        });
        self.destroy_buffer(gpu, staging);
        result?;
        Ok(id)
    }

    fn destroy_buffer(&mut self, gpu: &mut Gpu, id: BufferId) {
        let b = &mut self.buffers[id.0];
        if b.buffer != vk::Buffer::null() {
            unsafe { gpu.device.destroy_buffer(b.buffer, None) };
            b.buffer = vk::Buffer::null();
        }
        if let Some(a) = b.allocation.take() {
            gpu.free(a);
        }
    }

    /// Upload a texture (all levels), leaving it in `GENERAL` layout.
    pub fn upload_texture(&mut self, gpu: &mut Gpu, name: &str, data: &TextureData, extra_usage: vk::ImageUsageFlags) -> Result<ImageId, RuntimeError> {
        let desc = ImageDesc {
            name: name.to_string(),
            format: data.format,
            extent: [data.width, data.height, data.depth],
            dim: data.dim,
            mip_levels: data.mip_levels(),
            layers: 1,
            cube: false,
            usage: vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::TRANSFER_SRC | extra_usage,
            clear: [0.0; 4],
        };
        let id = self.create_image_uninit(gpu, desc)?;
        let total: usize = data.levels.iter().map(Vec::len).sum();
        let staging = self.create_buffer(gpu, &format!("{name} (staging)"), total as u64, vk::BufferUsageFlags::TRANSFER_SRC, MemoryLocation::CpuToGpu)?;
        let mut regions = Vec::new();
        let texel_size = texel::texel_size(data.format).unwrap_or(4) as u64;
        if let Some(m) = self.buffers[staging.0].mapped() {
            let mut off = 0usize;
            let img = &self.images[id.0];
            for (level, bytes) in data.levels.iter().enumerate().take(img.desc.mip_levels as usize) {
                let level = level as u32;
                let w = (img.desc.extent[0] >> level).max(1);
                let h = (img.desc.extent[1] >> level).max(1);
                let dpt = (img.desc.extent[2] >> level).max(1);
                let expected = u64::from(w) * u64::from(h) * u64::from(dpt) * texel_size;
                if bytes.len() as u64 != expected || off + bytes.len() > m.len() {
                    break;
                }
                m[off..off + bytes.len()].copy_from_slice(bytes);
                regions.push(vk::BufferImageCopy {
                    buffer_offset: off as u64,
                    buffer_row_length: 0,
                    buffer_image_height: 0,
                    image_subresource: vk::ImageSubresourceLayers { aspect_mask: img.aspect, mip_level: level, base_array_layer: 0, layer_count: 1 },
                    image_offset: vk::Offset3D::default(),
                    image_extent: vk::Extent3D { width: w, height: h, depth: dpt },
                });
                off += bytes.len();
            }
        }
        let img = &self.images[id.0];
        let (image, range, format) = (img.image, img.full_range(), img.desc.format);
        let src = self.buffers[staging.0].buffer;
        let result = gpu.one_shot(|d, cmd| {
            init_layout(d, cmd, image, range);
            // Levels without data (malformed input) read as zero.
            unsafe { d.cmd_clear_color_image(cmd, image, vk::ImageLayout::GENERAL, &clear_color_value(format, [0.0; 4]), &[range]) };
            full_barrier(d, cmd);
            if !regions.is_empty() {
                unsafe { d.cmd_copy_buffer_to_image(cmd, src, image, vk::ImageLayout::GENERAL, &regions) };
            }
        });
        self.destroy_buffer(gpu, staging);
        result?;
        Ok(id)
    }

    /// A sampler (cached).
    pub fn sampler(&mut self, gpu: &Gpu, key: SamplerKey) -> Result<vk::Sampler, RuntimeError> {
        if let Some(s) = self.samplers.get(&key) {
            return Ok(*s);
        }
        let filter = if key.linear { vk::Filter::LINEAR } else { vk::Filter::NEAREST };
        let address = if key.repeat { vk::SamplerAddressMode::REPEAT } else { vk::SamplerAddressMode::CLAMP_TO_EDGE };
        let mut info = vk::SamplerCreateInfo::default()
            .mag_filter(filter)
            .min_filter(filter)
            .mipmap_mode(if key.mipmapped && key.mip_linear { vk::SamplerMipmapMode::LINEAR } else { vk::SamplerMipmapMode::NEAREST })
            .address_mode_u(address)
            .address_mode_v(address)
            .address_mode_w(address)
            .min_lod(0.0)
            .max_lod(if key.mipmapped { vk::LOD_CLAMP_NONE } else { 0.0 })
            .border_color(vk::BorderColor::FLOAT_OPAQUE_WHITE);
        if let Some(op) = key.compare {
            info = info.compare_enable(true).compare_op(op);
        }
        let s = unsafe { gpu.device.create_sampler(&info, None) }.vk("vkCreateSampler")?;
        self.samplers.insert(key, s);
        Ok(s)
    }

    /// Destroy everything (waits for the device first).
    pub fn destroy_all(&mut self, gpu: &mut Gpu) {
        let _ = gpu.wait_idle();
        let d = &gpu.device;
        unsafe {
            for p in self.pipelines.drain(..) {
                d.destroy_pipeline(p, None);
            }
            for l in self.pipeline_layouts.drain(..) {
                d.destroy_pipeline_layout(l, None);
            }
            for l in self.set_layouts.drain(..) {
                d.destroy_descriptor_set_layout(l, None);
            }
            for m in self.modules.drain(..) {
                d.destroy_shader_module(m, None);
            }
            for p in self.descriptor_pools.drain(..) {
                d.destroy_descriptor_pool(p, None);
            }
            if !self.command_buffers.is_empty() {
                d.free_command_buffers(gpu.command_pool, &self.command_buffers);
                self.command_buffers.clear();
            }
            for (_, s) in self.samplers.drain() {
                d.destroy_sampler(s, None);
            }
        }
        let images = std::mem::take(&mut self.images);
        for mut img in images {
            unsafe {
                for v in img.views.drain().map(|(_, v)| v).chain(img.level_views.drain().map(|(_, v)| v)) {
                    gpu.device.destroy_image_view(v, None);
                }
                gpu.device.destroy_image(img.image, None);
            }
            if let Some(a) = img.allocation.take() {
                gpu.free(a);
            }
        }
        let buffers = std::mem::take(&mut self.buffers);
        for mut b in buffers {
            if b.buffer != vk::Buffer::null() {
                unsafe { gpu.device.destroy_buffer(b.buffer, None) };
            }
            if let Some(a) = b.allocation.take() {
                gpu.free(a);
            }
        }
    }
}

/// UNDEFINED → GENERAL for a fresh image.
pub(crate) fn init_layout(device: &ash::Device, cmd: vk::CommandBuffer, image: vk::Image, range: vk::ImageSubresourceRange) {
    let barrier = [vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::GENERAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(range)];
    unsafe {
        device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::ALL_COMMANDS, vk::DependencyFlags::empty(), &[], &[], &barrier);
    }
}
