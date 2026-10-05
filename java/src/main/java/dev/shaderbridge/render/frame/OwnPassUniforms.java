package dev.shaderbridge.render.frame;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.RenderPass;
import dev.shaderbridge.render.draw.UniformTarget;
import dev.shaderbridge.render.targets.TextureBinding;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Map;
import java.util.Optional;
import java.util.Set;

/**
 * A render pass ShaderBridge created for its own draws (composite-style passes, Distant Horizons
 * LODs) as a {@link UniformTarget}: it starts with nothing bound and remembers what was bound
 * through it.
 */
final class OwnPassUniforms implements UniformTarget {
    private final RenderPass pass;
    private final Set<String> bound = new HashSet<>();
    private final Map<String, TextureBinding> textures = new HashMap<>();

    /** @param pass a pass nothing was bound to yet */
    OwnPassUniforms(RenderPass pass) {
        this.pass = pass;
    }

    @Override
    public boolean isBound(String name) {
        return bound.contains(name);
    }

    @Override
    public Optional<TextureBinding> boundTexture(String name) {
        return Optional.ofNullable(textures.get(name));
    }

    @Override
    public void bind(String name, GpuBufferSlice slice) {
        pass.setUniform(name, slice);
        bound.add(name);
    }

    @Override
    public void bind(String name, TextureBinding texture) {
        pass.setUniform(name, texture.view(), texture.sampler());
        bound.add(name);
        textures.put(name, texture);
    }
}
