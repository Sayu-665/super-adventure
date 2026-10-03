//! Reading images back to the host as RGBA8 (rows flipped so that GL's bottom-up image
//! appears upright).

use crate::error::RuntimeError;
use crate::executor::Executor;
use crate::resources::{ImageId, full_barrier, host_read_barrier};
use crate::texel;
use ash::vk;
use gpu_allocator::MemoryLocation;

/// Convert decoded texels of `format` to RGBA8: float/normalized values are clamped to
/// `[0, 1]`, integer values to `[0, 255]`, depth is grey; GL row order (bottom-up) is
/// flipped to top-down.
pub(crate) fn to_rgba8(format: vk::Format, width: u32, height: u32, bytes: &[u8], depth_to_gl: &dyn Fn(f32) -> f32) -> image::RgbaImage {
    let size = texel::texel_size(format).unwrap_or(4);
    let comps = texel::component_count(format).unwrap_or(4);
    let class = texel::numeric_class(format);
    let mut out = image::RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let i = (y as usize * width as usize + x as usize) * size;
            let v = bytes.get(i..i + size).and_then(|b| texel::decode(format, b)).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let px = if format == vk::Format::D32_SFLOAT {
                let g = (f64::from(depth_to_gl(v[0] as f32)).clamp(0.0, 1.0) * 255.0).round() as u8;
                [g, g, g, 255]
            } else {
                let conv = |c: f64| -> u8 {
                    let c = if c.is_nan() { 0.0 } else { c };
                    match class {
                        texel::NumericClass::Float => (c.clamp(0.0, 1.0) * 255.0).round() as u8,
                        _ => c.clamp(0.0, 255.0).round() as u8,
                    }
                };
                let mut p = [conv(v[0]), conv(v[1]), conv(v[2]), conv(v[3])];
                if comps < 4 {
                    p[3] = 255;
                }
                p
            };
            out.put_pixel(x, height - 1 - y, image::Rgba(px));
        }
    }
    out
}

/// Set the alpha of every pixel to 255.
pub(crate) fn force_opaque(img: &mut image::RgbaImage) {
    for p in img.pixels_mut() {
        p[3] = 255;
    }
}

impl Executor<'_> {
    /// Read level 0 of an image.
    pub(crate) fn read_image(&mut self, id: ImageId) -> Result<image::RgbaImage, RuntimeError> {
        let img = self.arena.image(id);
        let (format, ext, aspect, image) = (img.desc.format, img.extent_2d(), img.aspect, img.image);
        let size = texel::texel_size(format).unwrap_or(4) as u64 * u64::from(ext.width) * u64::from(ext.height);
        let buf = self.arena.create_buffer(self.gpu, "readback", size, vk::BufferUsageFlags::TRANSFER_DST, MemoryLocation::GpuToCpu)?;
        let vkbuf = self.arena.buffer(buf).buffer;
        let region = vk::BufferImageCopy {
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image_subresource: vk::ImageSubresourceLayers { aspect_mask: aspect, mip_level: 0, base_array_layer: 0, layer_count: 1 },
            image_offset: vk::Offset3D::default(),
            image_extent: vk::Extent3D { width: ext.width, height: ext.height, depth: 1 },
        };
        self.gpu.one_shot(|d, cmd| {
            full_barrier(d, cmd);
            unsafe { d.cmd_copy_image_to_buffer(cmd, image, vk::ImageLayout::GENERAL, vkbuf, &[region]) };
            full_barrier(d, cmd);
            host_read_barrier(d, cmd);
        })?;
        let depth = self.depth;
        let bytes = self.arena.buffer(buf).mapped_ref().map(<[u8]>::to_vec).unwrap_or_default();
        Ok(to_rgba8(format, ext.width, ext.height, &bytes, &|d| depth.gl_depth(d)))
    }

    /// The final image, opaque: it stands for Minecraft's window, which has no alpha
    /// (Iris' final pass writes the main framebuffer, and the window is composited
    /// opaque). A `final` program may leave alpha undefined (photon and Bliss declare
    /// `out vec3`), so alpha is forced to 255 instead of keeping whatever the driver wrote.
    pub(crate) fn read_output(&mut self) -> Result<image::RgbaImage, RuntimeError> {
        let mut img = self.read_image(self.targets.output)?;
        force_opaque(&mut img);
        Ok(img)
    }

    /// Every colortex / shadowcolor / depth target of the last frame.
    pub(crate) fn read_targets(&mut self) -> Result<Vec<(String, image::RgbaImage)>, RuntimeError> {
        let mut list: Vec<(String, ImageId)> = Vec::new();
        for (i, p) in &self.targets.color {
            list.push((format!("colortex{i}"), p.images[self.flips.read(*i)]));
        }
        for (i, p) in &self.targets.shadow_color {
            list.push((format!("shadowcolor{i}"), p.images[self.flips.shadow_read(*i)]));
        }
        for (i, img) in self.targets.depth.iter().enumerate() {
            list.push((format!("depthtex{i}"), *img));
        }
        if self.shadow_enabled {
            for (i, img) in self.targets.shadow.iter().enumerate() {
                list.push((format!("shadowtex{i}"), *img));
            }
        }
        if self.dh_enabled && !self.unified {
            for (i, img) in self.targets.dh_depth.iter().enumerate() {
                list.push((format!("dhDepthTex{i}"), *img));
            }
        }
        let mut out = Vec::with_capacity(list.len());
        for (name, img) in list {
            out.push((name, self.read_image(img)?));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_flipped_and_values_clamped() {
        // 1x2 RGBA16F: bottom row (GL row 0) red, top row (GL row 1) green, out of range.
        let mut bytes = Vec::new();
        for v in [[1.0f32, 0.0, 0.0, 1.0], [0.0, 2.0, -1.0, 0.5]] {
            for c in v {
                bytes.extend_from_slice(&texel::f32_to_f16(c).to_le_bytes());
            }
        }
        let img = to_rgba8(vk::Format::R16G16B16A16_SFLOAT, 1, 2, &bytes, &|d| d);
        assert_eq!(img.get_pixel(0, 1).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(0, 0).0, [0, 255, 0, 128]);
        // Depth is grey in GL convention; R8 gets opaque alpha.
        let img = to_rgba8(vk::Format::D32_SFLOAT, 1, 1, &0.25f32.to_le_bytes(), &|d| 1.0 - d);
        assert_eq!(img.get_pixel(0, 0).0, [191, 191, 191, 255]);
        let img = to_rgba8(vk::Format::R8_UNORM, 1, 1, &[255], &|d| d);
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
        // Short data does not panic.
        let img = to_rgba8(vk::Format::R8G8B8A8_UNORM, 2, 2, &[1, 2], &|d| d);
        assert_eq!(img.dimensions(), (2, 2));
    }

    /// The final image is opaque even when the `final` program leaves alpha at 0
    /// (`out vec3`), as Minecraft's window is.
    #[test]
    fn final_output_is_forced_opaque() {
        let mut img = to_rgba8(vk::Format::R8G8B8A8_UNORM, 2, 1, &[10, 20, 30, 0, 40, 50, 60, 7], &|d| d);
        force_opaque(&mut img);
        assert_eq!(img.get_pixel(0, 0).0, [10, 20, 30, 255]);
        assert_eq!(img.get_pixel(1, 0).0, [40, 50, 60, 255]);
    }
}
