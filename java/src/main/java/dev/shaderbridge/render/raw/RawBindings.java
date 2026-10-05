package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.backend.vulkan.VulkanConst;
import com.mojang.renderpearl.backend.vulkan.VulkanGpuBuffer;
import com.mojang.renderpearl.backend.vulkan.VulkanGpuSampler;
import com.mojang.renderpearl.backend.vulkan.VulkanGpuTextureView;
import dev.shaderbridge.model.ResourceKind;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.model.TextureFormat;
import dev.shaderbridge.render.pipeline.DepthStates;
import dev.shaderbridge.render.pipeline.SpirvReflection.ImageDim;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import dev.shaderbridge.render.pipeline.TextureFormats;
import dev.shaderbridge.render.targets.ColorPair;
import dev.shaderbridge.render.targets.SamplerSpec;
import dev.shaderbridge.render.targets.TextureBinding;
import java.util.Locale;
import java.util.Optional;
import org.lwjgl.vulkan.VK10;

/**
 * Resolves the descriptors of a raw program to Vulkan handles for one use: {@code sb_Frame} and
 * {@code sb_Draw} to the frame's buffers; sampled pack resources through the
 * {@code TextureResolver} renderpearl draws use (render targets per {@link ImageChoice}, Minecraft's
 * samplers), with the raw path's own comparison samplers for {@code sampler2DShadow}, except
 * custom images and the raw textures the raw path provides ({@link RawTextureData}); storage
 * images to the render targets (when they have storage usage) or custom images; storage buffers to
 * the pack's buffers. A descriptor whose resource is missing or does not fit (dimensionality, texel
 * class, no storage usage) gets a stand-in, as in the headless executor, and a message. Render thread only.
 */
final class RawBindings {
    private final VulkanContext ctx;
    private final RawContext raw;
    private final RawResources resources;
    private final RawSamplers samplers;

    /** What one descriptor is bound to. */
    sealed interface Bound {
        /** @return the raw path's own resource behind it (initialized before use), or null for Minecraft's */
        Object own();

        /**
         * A buffer range.
         *
         * @param buffer the {@code VkBuffer}
         * @param offset offset in bytes
         * @param range  size in bytes
         * @param own    the raw path's buffer, or null
         */
        record Buffer(long buffer, long offset, long range, Object own) implements Bound {
        }

        /**
         * An image view, with a sampler for combined image samplers.
         *
         * @param view    the {@code VkImageView}
         * @param sampler the {@code VkSampler}, {@code VK_NULL_HANDLE} for storage images
         * @param own     the raw path's image, or null
         */
        record Image(long view, long sampler, Object own) implements Bound {
        }
    }

    RawBindings(VulkanContext ctx, RawContext raw, RawResources resources, RawSamplers samplers) {
        this.ctx = ctx;
        this.raw = raw;
        this.resources = resources;
        this.samplers = samplers;
    }

    /**
     * @param binding a binding of the program's plan
     * @param use     the use being recorded
     * @return what to bind
     */
    Bound resolve(DescriptorPlan.Binding binding, RawUse use) {
        return switch (binding.source()) {
            case DescriptorPlan.Source.Frame f -> buffer(use.frameUniforms());
            case DescriptorPlan.Source.Draw d -> buffer(use.drawUniforms());
            case DescriptorPlan.Source.Pack p -> switch (p.entry().kind()) {
                case ResourceKind.Sampler s -> sampled(binding, p, s, use);
                case ResourceKind.StorageImage s -> storage(binding, p, s, use);
                case ResourceKind.StorageBuffer s -> storageBuffer(binding, p.entry().resource(), use);
                case ResourceKind.UniformBuffer u -> throw new IllegalStateException(binding.name() + ": host blocks are rejected by the plan");
            };
        };
    }

    private static Bound buffer(GpuBufferSlice slice) {
        return new Bound.Buffer(((VulkanGpuBuffer) slice.buffer()).vkBuffer(), slice.offset(), slice.length(), null);
    }

    private Bound sampled(DescriptorPlan.Binding b, DescriptorPlan.Source.Pack p, ResourceKind.Sampler kind, RawUse use) {
        ResourceRef ref = p.entry().resource();
        int dims = dimensions(b.descriptor().dim());
        ScalarClass texel = b.descriptor().sampled();
        if (ref instanceof ResourceRef.Image image) {
            Optional<VmaImage> own = resources.image(image.name()).filter(i -> i.dimensions() == dims);
            if (own.isPresent()) {
                boolean linear = ctx.supports(own.get().vkFormat(), VK10.VK_FORMAT_FEATURE_SAMPLED_IMAGE_FILTER_LINEAR_BIT);
                return new Bound.Image(own.get().view(), samplers.get(new RawSamplers.Key(linear, false, compareOp(kind))), own.get());
            }
            return standIn(b, use, dims, texel, kind, "custom image " + image.name() + " does not exist as a " + dims + "D image");
        }
        if (ref instanceof ResourceRef.CustomTexture custom) {
            Optional<RawResources.Texture> own = resources.texture(custom.id()).filter(t -> t.image().dimensions() == dims);
            if (own.isPresent()) {
                return rawTexture(b, use, own.get(), texel, kind);
            }
        }
        if (dims != 2) {
            return standIn(b, use, dims, texel, kind, "only the raw path's custom images and raw textures are " + dims + "D");
        }
        boolean alt = ImageChoice.alt(use.program().kind(), ref, p.useAlt(), use.colorAlt(), use.shadowColorAlt(), m -> report(use, m));
        TextureBinding texture = raw.resolver().resolve(ref, alt, use.program(), raw.host().get());
        GpuFormat format = texture.view().texture().getFormat();
        if (TextureFormats.numericClass(format) != texel && !format.hasDepthAspect()) {
            return standIn(b, use, dims, texel, kind, "its texture has format " + format + ", which reads as " + TextureFormats.numericClass(format));
        }
        long view = ((VulkanGpuTextureView) texture.view()).vkImageView();
        if (kind.shadow()) {
            SamplerSpec spec = raw.resolver().samplerSpec(ref, use.program());
            boolean linear = spec.linear() && ctx.supports(VulkanConst.toVk(format), VK10.VK_FORMAT_FEATURE_SAMPLED_IMAGE_FILTER_LINEAR_BIT);
            return new Bound.Image(view, samplers.get(new RawSamplers.Key(linear, spec.repeat(), compareOp(kind))), null);
        }
        return new Bound.Image(view, ((VulkanGpuSampler) texture.sampler()).vkSampler(), null);
    }

    private Bound rawTexture(DescriptorPlan.Binding b, RawUse use, RawResources.Texture texture, ScalarClass texel, ResourceKind.Sampler kind) {
        if (TextureFormats.numericClass(texture.format()) != texel) {
            return standIn(b, use, texture.image().dimensions(), texel, kind, "its raw texture has format " + texture.format() + ", which reads as "
                + TextureFormats.numericClass(texture.format()));
        }
        boolean linear = texture.linear() && ctx.supports(texture.image().vkFormat(), VK10.VK_FORMAT_FEATURE_SAMPLED_IMAGE_FILTER_LINEAR_BIT);
        return new Bound.Image(texture.image().view(), samplers.get(new RawSamplers.Key(linear, texture.repeat(), compareOp(kind))), texture.image());
    }

    private Bound storage(DescriptorPlan.Binding b, DescriptorPlan.Source.Pack p, ResourceKind.StorageImage kind, RawUse use) {
        ResourceRef ref = p.entry().resource();
        int dims = dimensions(b.descriptor().dim());
        ScalarClass texel = b.descriptor().sampled();
        Optional<ColorPair> pair = switch (ref) {
            case ResourceRef.ColorImage c -> raw.targets().color(c.index());
            case ResourceRef.ColorTex c -> raw.targets().color(c.index());
            case ResourceRef.ShadowColorImage s -> raw.targets().shadowColor(s.index());
            case ResourceRef.ShadowColor s -> raw.targets().shadowColor(s.index());
            default -> Optional.empty();
        };
        if (pair.isPresent() && dims == 2) {
            boolean alt = ImageChoice.alt(use.program().kind(), ref, p.useAlt(), use.colorAlt(), use.shadowColorAlt(), m -> report(use, m));
            GpuTexture texture = pair.get().texture(alt);
            if (!StorageUsage.hasStorage(texture)) {
                return storageStandIn(b, use, kind, pair.get().spec().name() + " has no storage usage (its format " + texture.getFormat()
                    + " cannot be stored to on this device, or the storage hook is inactive)");
            }
            if (TextureFormats.numericClass(texture.getFormat()) != texel) {
                return storageStandIn(b, use, kind, pair.get().spec().name() + " has format " + texture.getFormat());
            }
            return new Bound.Image(((VulkanGpuTextureView) pair.get().attachmentView(alt)).vkImageView(), VK10.VK_NULL_HANDLE, null);
        }
        if (ref instanceof ResourceRef.Image image) {
            Optional<VmaImage> own = resources.image(image.name()).filter(i -> i.dimensions() == dims);
            if (own.isPresent()) {
                return new Bound.Image(own.get().view(), VK10.VK_NULL_HANDLE, own.get());
            }
        }
        return storageStandIn(b, use, kind, ref + " has no " + dims + "D storage image");
    }

    private Bound storageBuffer(DescriptorPlan.Binding b, ResourceRef ref, RawUse use) {
        Optional<VmaBuffer> buffer = ref instanceof ResourceRef.Ssbo ssbo ? resources.buffer(ssbo.index()) : Optional.empty();
        VmaBuffer bound = buffer.orElseGet(() -> {
            report(use, b.name() + " (" + ref + ") has no storage buffer; a zero-filled buffer is bound");
            return resources.zero(ResourceSizes.MIN_BUFFER);
        });
        return new Bound.Buffer(bound.buffer(), 0, Math.min(bound.size(), ctx.maxBufferRange()), bound);
    }

    private Bound standIn(DescriptorPlan.Binding b, RawUse use, int dims, ScalarClass texel, ResourceKind.Sampler kind, String why) {
        report(use, b.name() + ": " + why + "; a stand-in texture is bound");
        VmaImage image = resources.standIn(dims, RawResources.standInFormat(texel));
        return new Bound.Image(image.view(), samplers.get(new RawSamplers.Key(false, false, compareOp(kind))), image);
    }

    private Bound storageStandIn(DescriptorPlan.Binding b, RawUse use, ResourceKind.StorageImage kind, String why) {
        report(use, b.name() + ": " + why + "; a stand-in image is bound");
        int format = declaredFormat(kind).filter(f -> ctx.supports(f, VK10.VK_FORMAT_FEATURE_STORAGE_IMAGE_BIT))
            .orElse(RawResources.standInFormat(b.descriptor().sampled()));
        VmaImage image = resources.standIn(dimensions(b.descriptor().dim()), format);
        return new Bound.Image(image.view(), VK10.VK_NULL_HANDLE, image);
    }

    /** The {@code VkFormat} of a storage image's GLSL format qualifier ({@code rgba16f}), if declared and known. */
    private static Optional<Integer> declaredFormat(ResourceKind.StorageImage kind) {
        if (kind.format() == null) {
            return Optional.empty();
        }
        try {
            return Optional.of(VulkanConst.toVk(TextureFormats.renderable(TextureFormat.valueOf(kind.format().toUpperCase(Locale.ROOT)))));
        } catch (IllegalArgumentException unknown) {
            return Optional.empty();
        }
    }

    /** {@code VkCompareOp} of a comparison sampler in the pack's depth convention, -1 for a plain sampler. */
    private int compareOp(ResourceKind.Sampler kind) {
        if (!kind.shadow()) {
            return -1;
        }
        return DepthStates.reversed(raw.depthMode()) ? VK10.VK_COMPARE_OP_GREATER_OR_EQUAL : VK10.VK_COMPARE_OP_LESS_OR_EQUAL;
    }

    private static int dimensions(ImageDim dim) {
        return switch (dim) {
            case D1 -> 1;
            case D3 -> 3;
            default -> 2;
        };
    }

    private void report(RawUse use, String message) {
        raw.diagnostics().report(use.program().name() + ": " + message);
    }
}
