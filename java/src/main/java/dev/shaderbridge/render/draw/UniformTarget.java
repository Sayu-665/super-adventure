package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import dev.shaderbridge.render.targets.TextureBinding;
import java.util.Optional;

/** Where a pack pipeline's descriptors are bound: a render pass, by descriptor name. */
public interface UniformTarget {
    /**
     * @param name a descriptor name
     * @return whether the pass already has a value for it (bound by the host draw path)
     */
    boolean isBound(String name);

    /**
     * @param name a sampler name
     * @return the texture the pass has for it, if any (e.g. the albedo the host bound as {@code Sampler0})
     */
    Optional<TextureBinding> boundTexture(String name);

    /**
     * @param name  a uniform block name
     * @param slice its buffer slice
     */
    void bind(String name, GpuBufferSlice slice);

    /**
     * @param name    a sampler name
     * @param texture its texture view and sampler
     */
    void bind(String name, TextureBinding texture);
}
