package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.util.TextureViewAndSampler;
import dev.shaderbridge.render.targets.TextureBinding;
import java.util.Map;
import java.util.Optional;

/**
 * A Minecraft render pass as a {@link UniformTarget}: binds through the pass and reads what the
 * pass already holds from its uniform map (Mojang's render pass keeps every value bound in the pass
 * and re-applies them to each pipeline).
 *
 * @param pass   the pass
 * @param values the pass's uniform values by name (not copied; read only)
 */
public record RenderPassUniforms(RenderPass pass, Map<String, Object> values) implements UniformTarget {
    @Override
    public boolean isBound(String name) {
        return values.containsKey(name);
    }

    @Override
    public Optional<TextureBinding> boundTexture(String name) {
        return values.get(name) instanceof TextureViewAndSampler t ? Optional.of(new TextureBinding(t.view(), t.sampler())) : Optional.empty();
    }

    @Override
    public void bind(String name, GpuBufferSlice slice) {
        pass.setUniform(name, slice);
    }

    @Override
    public void bind(String name, TextureBinding texture) {
        pass.setUniform(name, texture.view(), texture.sampler());
    }
}
