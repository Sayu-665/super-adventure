//! Mipmap generation by rendering, for colour formats the device cannot blit into.
//!
//! `generate_mips` normally blits every level from the previous one, which needs
//! `BLIT_SRC | BLIT_DST` on the image's format. Lavapipe (and some mobile drivers) lack
//! `BLIT_DST` for `B10G11R11_UFLOAT_PACK32` — the format of many packs' HDR scene buffer,
//! whose smallest mip level drives auto exposure (`textureLod(colortex0, vec2(0.5), 10)`).
//! Without mipmaps those levels stay at their clear value and exposure runs away (white
//! or black images). For such formats every level is rendered instead: a fullscreen
//! triangle samples the previous level bilinearly at the destination texel centre, which
//! is exactly what a `VK_FILTER_LINEAR` blit between consecutive levels computes.

use crate::error::{RuntimeError, VkResultExt};
use crate::executor::Executor;
use crate::resources::{ImageId, SamplerKey, full_barrier};
use crate::texel::{self, NumericClass};
use crate::textures::TexDim;
use ash::vk;
use sb_core::ShaderStage;
use std::collections::HashMap;
use std::ffi::CString;

/// Fullscreen triangle.
const VERTEX: &str = "#version 450
void main() {
    vec2 p = vec2((gl_VertexIndex << 1) & 2, gl_VertexIndex & 2);
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
";

/// One destination texel = the bilinear sample of the previous level at its centre. The
/// bound view holds only the previous level; level sizes halve and round down (min 1).
const FRAGMENT: &str = "#version 450
layout(set = 0, binding = 0) uniform sampler2D sb_src;
layout(location = 0) out vec4 sb_out;
void main() {
    vec2 dst = max(floor(vec2(textureSize(sb_src, 0)) * 0.5), vec2(1.0));
    sb_out = textureLod(sb_src, gl_FragCoord.xy / dst, 0.0);
}
";

/// The downsampling pipeline objects of one render (created on first use; the arena owns
/// and destroys them).
pub(crate) struct MipRenderer {
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    vertex: vk::ShaderModule,
    fragment: vk::ShaderModule,
    pipelines: HashMap<vk::Format, vk::Pipeline>,
}

/// Whether `generate_mips` can blit `features`' format into its own mip levels.
pub(crate) fn can_blit(features: vk::FormatFeatureFlags) -> bool {
    features.contains(vk::FormatFeatureFlags::BLIT_SRC | vk::FormatFeatureFlags::BLIT_DST)
}

/// Whether mip levels of a `format` image can be rendered instead: a float colour
/// attachment that can be sampled with linear filtering.
pub(crate) fn can_render(format: vk::Format, features: vk::FormatFeatureFlags) -> bool {
    texel::numeric_class(format) == NumericClass::Float
        && features.contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT | vk::FormatFeatureFlags::SAMPLED_IMAGE | vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR)
}

fn compile(src: &str, stage: ShaderStage) -> Result<Vec<u32>, RuntimeError> {
    sb_compile::compile_glsl(src, stage, "sb-runtime mip downsample", &sb_compile::CompileOptions::default(), None)
        .map_err(|e| RuntimeError::Unsupported(format!("the mip downsample shader does not compile: {e}")))
}

impl Executor<'_> {
    /// Create the shared objects of the downsample pipelines (once per render).
    fn ensure_mip_renderer(&mut self) -> Result<(), RuntimeError> {
        if self.mip_renderer.is_some() {
            return Ok(());
        }
        let d = &self.gpu.device;
        let arena = &mut self.arena;
        let mut module = |src: &str, stage: ShaderStage| -> Result<vk::ShaderModule, RuntimeError> {
            let words = compile(src, stage)?;
            let m = unsafe { d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None) }.vk("vkCreateShaderModule")?;
            arena.modules.push(m);
            Ok(m)
        };
        let vertex = module(VERTEX, ShaderStage::Vertex)?;
        let fragment = module(FRAGMENT, ShaderStage::Fragment)?;
        let bindings = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        let set_layout = unsafe { d.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings), None) }.vk("vkCreateDescriptorSetLayout")?;
        self.arena.set_layouts.push(set_layout);
        let set_layouts = [set_layout];
        let layout = unsafe { d.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts), None) }.vk("vkCreatePipelineLayout")?;
        self.arena.pipeline_layouts.push(layout);
        self.mip_renderer = Some(MipRenderer { set_layout, layout, vertex, fragment, pipelines: HashMap::new() });
        Ok(())
    }

    /// The downsample pipeline rendering into `format`, its layout and set layout.
    fn mip_pipeline(&mut self, format: vk::Format) -> Result<(vk::Pipeline, vk::PipelineLayout, vk::DescriptorSetLayout), RuntimeError> {
        self.ensure_mip_renderer()?;
        let Some(r) = self.mip_renderer.as_mut() else {
            return Err(RuntimeError::Unsupported("the mip downsample pipeline is unavailable".into()));
        };
        if let Some(p) = r.pipelines.get(&format) {
            return Ok((*p, r.layout, r.set_layout));
        }
        let gpu: &crate::device::Gpu = self.gpu;
        let main = CString::new("main").unwrap_or_default();
        let stages = [
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(r.vertex).name(&main),
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(r.fragment).name(&main),
        ];
        let vi = vk::PipelineVertexInputStateCreateInfo::default();
        let ia = vk::PipelineInputAssemblyStateCreateInfo::default().topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let vp = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
        let rs = vk::PipelineRasterizationStateCreateInfo::default().polygon_mode(vk::PolygonMode::FILL).cull_mode(vk::CullModeFlags::NONE).line_width(1.0);
        let ms = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let ds = vk::PipelineDepthStencilStateCreateInfo::default();
        let blend = [vk::PipelineColorBlendAttachmentState::default().color_write_mask(vk::ColorComponentFlags::RGBA)];
        let cb = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend);
        let dynamic = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dy = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic);
        let formats = [format];
        let mut rendering = vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vi)
            .input_assembly_state(&ia)
            .viewport_state(&vp)
            .rasterization_state(&rs)
            .multisample_state(&ms)
            .depth_stencil_state(&ds)
            .color_blend_state(&cb)
            .dynamic_state(&dy)
            .layout(r.layout)
            .push_next(&mut rendering);
        let created = unsafe { gpu.device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None) }.map_err(|(_, e)| e).vk("vkCreateGraphicsPipelines")?;
        let p = created.into_iter().next().ok_or(RuntimeError::Vulkan { call: "vkCreateGraphicsPipelines", result: vk::Result::ERROR_UNKNOWN })?;
        gpu.set_name(p, &format!("sb-runtime mip downsample ({format:?})"));
        r.pipelines.insert(format, p);
        self.arena.pipelines.push(p);
        Ok((p, r.layout, r.set_layout))
    }

    /// Fill mip levels 1.. of the 2D colour image `image` from level 0 by rendering each
    /// level from the previous one (see the module docs). The image must satisfy
    /// [`can_render`].
    pub(crate) fn render_mips(&mut self, cmd: vk::CommandBuffer, image: ImageId) -> Result<(), RuntimeError> {
        let (format, levels, [w, h, _]) = {
            let img = self.arena.image(image);
            if img.desc.dim != TexDim::D2 || img.desc.layers > 1 || img.desc.cube || img.is_depth() {
                return Err(RuntimeError::Unsupported(format!("{}: only 2D colour images get rendered mipmaps", img.desc.name)));
            }
            (img.desc.format, img.desc.mip_levels, img.desc.extent)
        };
        if levels <= 1 {
            return Ok(());
        }
        let (pipeline, layout, set_layout) = self.mip_pipeline(format)?;
        let sampler = self.arena.sampler(self.gpu, SamplerKey::LINEAR_CLAMP)?;
        for level in 1..levels {
            let src = self.arena.level_view(self.gpu, image, level - 1)?;
            let dst = self.arena.level_view(self.gpu, image, level)?;
            let set = self.allocate_sets(&[set_layout])?.into_iter().next().ok_or(RuntimeError::Unsupported("no descriptor set for the mip downsample".into()))?;
            let image_info = [vk::DescriptorImageInfo { sampler, image_view: src, image_layout: vk::ImageLayout::GENERAL }];
            let write = vk::WriteDescriptorSet::default().dst_set(set).dst_binding(0).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).image_info(&image_info);
            let d = &self.gpu.device;
            unsafe { d.update_descriptor_sets(&[write], &[]) };
            let extent = vk::Extent2D { width: (w >> level).max(1), height: (h >> level).max(1) };
            let color = [vk::RenderingAttachmentInfo::default()
                .image_view(dst)
                .image_layout(vk::ImageLayout::GENERAL)
                .load_op(vk::AttachmentLoadOp::DONT_CARE)
                .store_op(vk::AttachmentStoreOp::STORE)];
            let info = vk::RenderingInfo::default().render_area(vk::Rect2D { offset: vk::Offset2D::default(), extent }).layer_count(1).color_attachments(&color);
            full_barrier(d, cmd);
            self.gpu.cmd_begin_rendering(cmd, &info);
            let viewport = [vk::Viewport { x: 0.0, y: 0.0, width: extent.width as f32, height: extent.height as f32, min_depth: 0.0, max_depth: 1.0 }];
            let scissor = [vk::Rect2D { offset: vk::Offset2D::default(), extent }];
            unsafe {
                d.cmd_set_viewport(cmd, 0, &viewport);
                d.cmd_set_scissor(cmd, 0, &scissor);
                d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                d.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, layout, 0, &[set], &[]);
                d.cmd_draw(cmd, 3, 1, 0, 0);
            }
            self.gpu.cmd_end_rendering(cmd);
        }
        full_barrier(&self.gpu.device, cmd);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NoTextures, RenderRequest, Runtime, RuntimeOptions, SceneParams};
    use sb_core::TextureFormat;
    use sb_core::model::DepthMode;

    /// `mini_pack` whose `final` shows mip level 2 of colortex0, with colortex0 in `format`.
    fn average_pack(format: TextureFormat) -> (sb_core::model::CompiledPack, sb_core::model::BlobTable) {
        let (mut pack, mut blobs) = crate::testutil::mini_pack();
        let fsh = "#version 460\nlayout(set = 1, binding = 1) uniform sampler2D colortex0;\nlayout(std430, set = 2, binding = 0) readonly buffer B { vec4 v[]; } b;\n\
                   layout(location = 0) in vec2 uv;\nlayout(location = 0) out vec4 o;\nvoid main() { o = vec4(textureLod(colortex0, uv, 2.0).rgb * b.v[0].rgb, 1.0); }\n";
        let words = compile(fsh, ShaderStage::Fragment).expect("final");
        let dim = &mut pack.dimensions[0];
        let fin = dim.programs.iter().position(|p| p.name == "final").expect("final program");
        let stage = dim.programs[fin].stages.iter_mut().find(|s| s.stage == ShaderStage::Fragment).expect("fragment stage");
        stage.spirv = Some(blobs.push_spirv(&words));
        dim.programs[fin].mipmap_targets = vec![0];
        dim.targets.colortex[0].format = format;
        dim.targets.colortex[0].mipmap_programs = vec![fin as u32];
        (pack, blobs)
    }

    /// Mean absolute difference per channel (0..255) and the variance of the red channel.
    fn compare(a: &image::RgbaImage, b: &image::RgbaImage) -> (f64, f64) {
        let n = f64::from(a.width() * a.height()).max(1.0);
        let diff: f64 = a.pixels().zip(b.pixels()).map(|(p, q)| (0..3).map(|c| (f64::from(p[c]) - f64::from(q[c])).abs()).sum::<f64>() / 3.0).sum::<f64>() / n;
        let mean: f64 = b.pixels().map(|p| f64::from(p[0])).sum::<f64>() / n;
        let var: f64 = b.pixels().map(|p| (f64::from(p[0]) - mean).powi(2)).sum::<f64>() / n;
        (diff, var)
    }

    /// A B10G11R11 target gets a correct mip chain even where the device cannot blit into
    /// that format (lavapipe): its level 2 matches the RGBA16F (blittable) render. Without
    /// mipmaps level 2 keeps its clear value and the image is uniform.
    #[test]
    fn unblittable_formats_get_mipmaps() {
        let Ok(mut rt) = Runtime::new(&RuntimeOptions { validation: true, prefer_cpu_device: true, device_name_filter: None }) else {
            eprintln!("skipping: no Vulkan device");
            return;
        };
        let feats = rt.gpu.format_features(vk::Format::B10G11R11_UFLOAT_PACK32);
        eprintln!("B10G11R11: blit {}, render {}", can_blit(feats), can_render(vk::Format::B10G11R11_UFLOAT_PACK32, feats));
        let mut render = |format| {
            let (pack, blobs) = average_pack(format);
            rt.render(&RenderRequest {
                pack: &pack,
                blobs: &blobs,
                dimension: "world0",
                width: 48,
                height: 30,
                frames: 1,
                scene: SceneParams { render_distance: 1, dh_render_distance: 0, ..Default::default() },
                depth_mode: DepthMode::ForwardZeroToOne,
                textures: &NoTextures,
                capture_targets: false,
            })
            .expect("render")
        };
        let reference = render(TextureFormat::RGBA16F);
        let packed = render(TextureFormat::R11F_G11F_B10F);
        for out in [&reference, &packed] {
            assert!(out.validation_messages.is_empty(), "{:#?}", out.validation_messages);
            assert!(out.stats.programs_skipped.is_empty(), "{:?}", out.stats.programs_skipped);
            assert!(!out.stats.warnings.iter().any(|w| w.contains("mipmaps")), "{:?}", out.stats.warnings);
        }
        let (diff, var) = compare(&reference.image, &packed.image);
        let (_, reference_var) = compare(&packed.image, &reference.image);
        eprintln!("mean abs diff {diff:.3}, variance {var:.1} (reference {reference_var:.1})");
        assert!(reference_var > 20.0 && var > 20.0, "mip level 2 is uniform: {reference_var} / {var}");
        assert!(diff < 3.0, "B10G11R11 mip level 2 differs from RGBA16F: {diff}");
    }

    #[test]
    fn downsample_shaders_compile() {
        compile(VERTEX, ShaderStage::Vertex).expect("vertex");
        compile(FRAGMENT, ShaderStage::Fragment).expect("fragment");
    }

    #[test]
    fn format_capabilities() {
        let all = vk::FormatFeatureFlags::BLIT_SRC
            | vk::FormatFeatureFlags::BLIT_DST
            | vk::FormatFeatureFlags::COLOR_ATTACHMENT
            | vk::FormatFeatureFlags::SAMPLED_IMAGE
            | vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR;
        // lavapipe's B10G11R11_UFLOAT_PACK32: no BLIT_DST, but renderable and filterable.
        let lavapipe = all & !vk::FormatFeatureFlags::BLIT_DST;
        assert!(can_blit(all));
        assert!(!can_blit(lavapipe));
        assert!(can_render(vk::Format::B10G11R11_UFLOAT_PACK32, lavapipe));
        assert!(!can_render(vk::Format::R32_UINT, all), "integer formats cannot be filtered");
        assert!(!can_render(vk::Format::R16G16B16A16_SFLOAT, all & !vk::FormatFeatureFlags::COLOR_ATTACHMENT));
    }
}
