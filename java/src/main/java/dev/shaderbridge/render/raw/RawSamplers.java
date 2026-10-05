package dev.shaderbridge.render.raw;

import java.nio.LongBuffer;
import java.util.HashMap;
import java.util.Map;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VkSamplerCreateInfo;

/**
 * The samplers the raw path creates itself, for what Mojang's sampler cache cannot express:
 * depth-comparison samplers ({@code sampler2DShadow}) and the raw path's own images. Created on
 * first use, destroyed with the raw path. Render thread only.
 */
final class RawSamplers implements AutoCloseable {
    /**
     * A sampler configuration.
     *
     * @param linear    linear filtering (else nearest)
     * @param repeat    repeat addressing (else clamp to edge)
     * @param compareOp {@code VkCompareOp} of a comparison sampler, -1 for none
     */
    record Key(boolean linear, boolean repeat, int compareOp) {
        /** Nearest, clamped, no comparison. */
        static final Key NEAREST_CLAMP = new Key(false, false, -1);
    }

    private final VulkanContext ctx;
    private final Map<Key, Long> samplers = new HashMap<>();

    RawSamplers(VulkanContext ctx) {
        this.ctx = ctx;
    }

    /**
     * @param key the configuration
     * @return the sampler
     * @throws RawVulkanException if Vulkan fails
     */
    long get(Key key) {
        Long known = samplers.get(key);
        if (known != null) {
            return known;
        }
        int filter = key.linear() ? VK10.VK_FILTER_LINEAR : VK10.VK_FILTER_NEAREST;
        int address = key.repeat() ? VK10.VK_SAMPLER_ADDRESS_MODE_REPEAT : VK10.VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE;
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkSamplerCreateInfo info = VkSamplerCreateInfo.calloc(stack).sType$Default()
                .magFilter(filter)
                .minFilter(filter)
                .mipmapMode(VK10.VK_SAMPLER_MIPMAP_MODE_NEAREST)
                .addressModeU(address)
                .addressModeV(address)
                .addressModeW(address)
                .minLod(0)
                .maxLod(0)
                .borderColor(VK10.VK_BORDER_COLOR_FLOAT_OPAQUE_WHITE)
                .compareEnable(key.compareOp() >= 0)
                .compareOp(Math.max(0, key.compareOp()));
            LongBuffer pSampler = stack.mallocLong(1);
            RawVulkanException.check(VK10.vkCreateSampler(ctx.vk(), info, null, pSampler), "vkCreateSampler");
            samplers.put(key, pSampler.get(0));
            return pSampler.get(0);
        }
    }

    @Override
    public void close() {
        samplers.values().forEach(s -> ctx.destroyLater(() -> VK10.vkDestroySampler(ctx.vk(), s, null)));
        samplers.clear();
    }
}
