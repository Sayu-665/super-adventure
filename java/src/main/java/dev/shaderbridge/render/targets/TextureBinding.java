package dev.shaderbridge.render.targets;

import com.mojang.renderpearl.api.textures.GpuSampler;
import com.mojang.renderpearl.api.textures.GpuTextureView;

/**
 * What a sampler descriptor is bound to ({@code RenderPass.setUniform(name, view, sampler)}).
 *
 * @param view    the texture view
 * @param sampler the sampler
 */
public record TextureBinding(GpuTextureView view, GpuSampler sampler) {
}
