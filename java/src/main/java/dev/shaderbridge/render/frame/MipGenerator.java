package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.api.textures.FilterMode;
import com.mojang.renderpearl.api.textures.GpuSampler;
import dev.shaderbridge.render.draw.BlitPipelines;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import dev.shaderbridge.render.pipeline.TextureFormats;
import dev.shaderbridge.render.targets.ColorPair;
import java.util.Optional;
import java.util.function.Consumer;

/**
 * Fills the lower mip levels of mipmapped render targets ({@code shadowcolorNMipmap} after the
 * shadow pass, a composite program's {@code mipmap} targets before it runs), as the headless
 * executor does. Renderpearl has no blit command, so each level is drawn from the one above with
 * Minecraft's screen blit cloned for the target's format ({@link BlitPipelines}), filtering
 * linearly where every device can. Integer targets get no mipmaps (they are sampled at their base
 * level only). Render thread only, outside any render pass.
 */
final class MipGenerator {
    /** The blit's sampler name. */
    private static final String SAMPLER = "InSampler";

    private final Consumer<String> diagnostics;

    /** @param diagnostics receives targets whose mipmaps cannot be generated (deduplicated by the receiver) */
    MipGenerator(Consumer<String> diagnostics) {
        this.diagnostics = diagnostics;
    }

    /**
     * @param format a render target format
     * @return whether its mipmaps are generated (formats shaders write as floats)
     */
    static boolean generated(GpuFormat format) {
        return TextureFormats.numericClass(format) == ScalarClass.FLOAT;
    }

    /**
     * Generates every level below the base level of one texture of a target.
     *
     * @param pair the target
     * @param alt  its alternate texture
     */
    void generate(ColorPair pair, boolean alt) {
        int levels = pair.spec().mipLevels();
        GpuFormat format = pair.spec().format();
        if (levels <= 1 || !generated(format)) {
            return;
        }
        CompiledRenderPipeline blit = RenderSystem.getCompiledPipelineNullable(BlitPipelines.to(format));
        if (blit == null) {
            diagnostics.accept(pair.spec().name() + ": the mipmap pipeline for " + format + " does not compile; its lower levels keep old contents");
            return;
        }
        GpuSampler sampler = RenderSystem.getSamplerCache().getClampToEdge(TextureFormats.filterable(format) ? FilterMode.LINEAR : FilterMode.NEAREST);
        CommandEncoder encoder = RenderSystem.getDevice().createCommandEncoder();
        for (int level = 1; level < levels; level++) {
            try (RenderPass pass = encoder.createRenderPass(() -> "ShaderBridge mipmaps of " + pair.spec().name(), pair.levelView(alt, level),
                Optional.empty())) {
                RenderSystem.bindDefaultUniforms(pass);
                pass.setPipeline(blit);
                pass.setUniform(SAMPLER, pair.levelView(alt, level - 1), sampler);
                pass.draw(3, 1, 0, 0);
            }
        }
    }
}
