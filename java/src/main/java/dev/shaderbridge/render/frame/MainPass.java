package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.platform.Lighting;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.textures.AddressMode;
import com.mojang.renderpearl.api.textures.FilterMode;
import com.mojang.renderpearl.api.textures.GpuSampler;
import dev.shaderbridge.mixin.LevelRendererAccess;
import java.util.OptionalDouble;
import net.minecraft.client.Minecraft;
import net.minecraft.client.TextureFilteringMethod;
import net.minecraft.client.renderer.GameRenderer;
import net.minecraft.client.renderer.chunk.ChunkSectionsToRender;
import net.minecraft.client.renderer.feature.FeatureRenderDispatcher;
import net.minecraft.client.renderer.state.OptionsRenderState;
import net.minecraft.client.renderer.state.level.LevelRenderState;

/**
 * ShaderBridge's replacement of the body of Minecraft's main level pass
 * ({@code LevelRenderer.addMainPass}'s {@code executes} lambda) while a pack frame is active. It
 * does what the vanilla body does before drawing (fog, chunk sampler, translucent preparation,
 * level lighting), then lets Minecraft draw the opaque geometry and the classic (non-OIT)
 * transparency into two gbuffers passes, running the pack's deferred passes between them and its
 * composite and final passes after them. The outline, see-through and always-on-top features that
 * follow in the vanilla body are left to the caller, which runs them whether or not this
 * succeeded.
 */
final class MainPass {
    private MainPass() {
    }

    /**
     * @param renderer   the pack frame (begun)
     * @param level      the level renderer
     * @param terrainFog the terrain fog block
     * @param chunks     the frame's chunk draws
     * @param features   the frame's prepared feature draws
     */
    static void run(PackRenderer renderer, LevelRendererAccess level, GpuBufferSlice terrainFog, ChunkSectionsToRender chunks,
                    FeatureRenderDispatcher.PreparedFrame features) {
        GameRenderer gameRenderer = Minecraft.getInstance().gameRenderer;
        RenderSystem.setShaderFog(terrainFog);
        updateChunkSampler(level, gameRenderer.gameRenderState().levelRenderState, gameRenderer.gameRenderState().optionsRenderState);
        level.shaderbridge$prepareTranslucents();
        gameRenderer.lighting().setupFor(Lighting.Entry.LEVEL);
        RenderPass opaque = renderer.openGbuffers("ShaderBridge gbuffers (opaque)");
        try (opaque) {
            level.shaderbridge$executeSolid(chunks, features, opaque);
        } finally {
            renderer.closed(opaque);
        }
        renderer.afterOpaque();
        RenderPass translucent = renderer.openGbuffers("ShaderBridge gbuffers (translucent)");
        try (translucent) {
            level.shaderbridge$executeClassicTransparency(chunks, features, translucent);
        } finally {
            renderer.closed(translucent);
        }
        renderer.finishFrame();
    }

    /** The vanilla body's chunk sampler upkeep (recreated when the texture filtering options change). */
    private static void updateChunkSampler(LevelRendererAccess level, LevelRenderState state, OptionsRenderState options) {
        GpuSampler current = level.shaderbridge$chunkLayerSampler();
        if (!state.shouldResetChunkLayerSampler && current != null) {
            return;
        }
        if (current != null) {
            current.close();
        }
        int maxAnisotropy = options.textureFiltering == TextureFilteringMethod.ANISOTROPIC ? options.maxAnisotropyValue : 1;
        level.shaderbridge$setChunkLayerSampler(RenderSystem.getDevice()
            .createSampler(AddressMode.CLAMP_TO_EDGE, AddressMode.CLAMP_TO_EDGE, FilterMode.LINEAR, FilterMode.LINEAR, maxAnisotropy, OptionalDouble.empty()));
    }
}
