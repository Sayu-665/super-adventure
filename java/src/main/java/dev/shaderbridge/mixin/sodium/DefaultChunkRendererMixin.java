package dev.shaderbridge.mixin.sodium;

import com.mojang.renderpearl.api.buffers.GpuBuffer;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.textures.GpuSampler;
import dev.shaderbridge.compat.sodium.SodiumTargets;
import dev.shaderbridge.compat.sodium.SodiumTerrain;
import net.caffeinemc.mods.sodium.client.render.chunk.ChunkRenderMatrices;
import net.caffeinemc.mods.sodium.client.render.chunk.DefaultChunkRenderer;
import net.caffeinemc.mods.sodium.client.render.chunk.lists.ChunkRenderListIterable;
import net.caffeinemc.mods.sodium.client.render.chunk.terrain.TerrainRenderPass;
import net.caffeinemc.mods.sodium.client.render.viewport.CameraTransform;
import net.caffeinemc.mods.sodium.client.util.FogParameters;
import net.minecraft.client.renderer.oit.OitStage;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Before Sodium draws a terrain pass into one of ShaderBridge's render passes, binds the block
 * atlas as the draw's albedo ({@link SodiumTerrain#bindAlbedo}). The pipeline Sodium binds next
 * is substituted with the pack's program by ShaderBridge's render pass hook, which sizes
 * {@code sb_Draw} by that albedo.
 */
@Mixin(value = DefaultChunkRenderer.class, remap = false)
abstract class DefaultChunkRendererMixin {
    @Inject(method = SodiumTargets.DRAW_TERRAIN, at = @At("HEAD"), require = 0)
    private void shaderbridge$bindAlbedo(ChunkRenderMatrices matrices, ChunkRenderListIterable renderLists, TerrainRenderPass terrainPass,
                                         CameraTransform camera, FogParameters fog, boolean indexedRenderingEnabled, RenderPass pass,
                                         GpuSampler terrainSampler, GpuBufferSlice uniformData, GpuBuffer sectionTimeInfo, OitStage stage,
                                         CallbackInfo ci) {
        SodiumTerrain.bindAlbedo(pass, terrainPass.getAtlas(), terrainSampler);
    }
}
