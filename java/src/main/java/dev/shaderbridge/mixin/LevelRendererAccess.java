package dev.shaderbridge.mixin;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.textures.GpuSampler;
import net.minecraft.client.renderer.LevelRenderer;
import net.minecraft.client.renderer.chunk.ChunkSectionsToRender;
import net.minecraft.client.renderer.feature.FeatureRenderDispatcher;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.gen.Accessor;
import org.spongepowered.asm.mixin.gen.Invoker;

/**
 * The private parts of {@link LevelRenderer} ShaderBridge's main-pass replacement calls, so that
 * Minecraft's own drawing code runs inside ShaderBridge's render passes.
 */
@Mixin(LevelRenderer.class)
public interface LevelRendererAccess {
    /** Prepares clouds, the world border and weather for the translucent draws. */
    @Invoker("prepareTranslucents")
    void shaderbridge$prepareTranslucents();

    /**
     * Draws opaque terrain and opaque features.
     *
     * @param chunks   the frame's chunk draws
     * @param features the frame's feature draws
     * @param pass     the pass to draw into
     */
    @Invoker("executeSolid")
    void shaderbridge$executeSolid(ChunkSectionsToRender chunks, FeatureRenderDispatcher.PreparedFrame features, RenderPass pass);

    /**
     * Draws translucent features and terrain, clouds, weather and the world border (classic, non-OIT).
     *
     * @param chunks   the frame's chunk draws
     * @param features the frame's feature draws
     * @param pass     the pass to draw into
     */
    @Invoker("executeClassicTransparency")
    void shaderbridge$executeClassicTransparency(ChunkSectionsToRender chunks, FeatureRenderDispatcher.PreparedFrame features, RenderPass pass);

    /**
     * Draws the entity outline target (glowing entities).
     *
     * @param features the frame's feature draws
     */
    @Invoker("executeOutline")
    void shaderbridge$executeOutline(FeatureRenderDispatcher.PreparedFrame features);

    /**
     * Draws the see-through features (name tags behind walls).
     *
     * @param features the frame's feature draws
     * @param main     Minecraft's main target
     */
    @Invoker("executeSeeThrough")
    void shaderbridge$executeSeeThrough(FeatureRenderDispatcher.PreparedFrame features, RenderTarget main);

    /**
     * Draws the always-on-top features (debug gizmos).
     *
     * @param features                the frame's feature draws
     * @param main                    Minecraft's main target
     * @param consistentDepthRequired post effects need the depth of the world alone
     */
    @Invoker("executeAlwaysOnTop")
    void shaderbridge$executeAlwaysOnTop(FeatureRenderDispatcher.PreparedFrame features, RenderTarget main, boolean consistentDepthRequired);

    /** @return the terrain sampler, null until the first main pass */
    @Accessor("chunkLayerSampler")
    GpuSampler shaderbridge$chunkLayerSampler();

    /** @param sampler the new terrain sampler */
    @Accessor("chunkLayerSampler")
    void shaderbridge$setChunkLayerSampler(GpuSampler sampler);
}
