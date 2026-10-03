package dev.shaderbridge.render.targets;

import com.mojang.blaze3d.systems.SamplerCache;
import com.mojang.renderpearl.api.textures.AddressMode;
import com.mojang.renderpearl.api.textures.FilterMode;
import com.mojang.renderpearl.api.textures.GpuSampler;

/**
 * Sampler parameters of a binding, in the terms of Mojang's {@link SamplerCache}.
 *
 * @param linear  linear minification and magnification (nearest otherwise)
 * @param mipmaps sample every mip level (otherwise the base level only)
 * @param repeat  repeat addressing (clamp to edge otherwise)
 */
public record SamplerSpec(boolean linear, boolean mipmaps, boolean repeat) {
    /** Nearest, base level, clamped: depth textures, integer targets, constant textures. */
    public static final SamplerSpec NEAREST_CLAMP = new SamplerSpec(false, false, false);
    /** Linear, base level, clamped. */
    public static final SamplerSpec LINEAR_CLAMP = new SamplerSpec(true, false, false);

    /**
     * @param cache Mojang's sampler cache ({@code RenderSystem.getSamplerCache()})
     * @return the cached sampler
     */
    public GpuSampler sampler(SamplerCache cache) {
        AddressMode address = repeat ? AddressMode.REPEAT : AddressMode.CLAMP_TO_EDGE;
        FilterMode filter = linear ? FilterMode.LINEAR : FilterMode.NEAREST;
        return cache.getSampler(address, address, filter, filter, mipmaps);
    }
}
