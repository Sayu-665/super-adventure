package dev.shaderbridge.mixin;

import dev.shaderbridge.render.chunk.ChunkMeshFormat;
import net.minecraft.client.Camera;
import net.minecraft.client.DeltaTracker;
import net.minecraft.client.renderer.extract.LevelExtractor;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Switches the chunk mesh format when a pack starts or stops rendering, with a full rebuild of the
 * chunk sections ({@link ChunkMeshFormat#beforeExtract}). The start of
 * {@code LevelExtractor.extract} is where Minecraft itself releases and recreates its section
 * meshes and dispatcher after a world change, before anything of the frame uses them. Not
 * required: without it sections stay in Minecraft's format.
 */
@Mixin(LevelExtractor.class)
abstract class LevelExtractorMixin {
    @Inject(method = "extract", at = @At("HEAD"), require = 0)
    private void shaderbridge$chunkMeshFormat(DeltaTracker deltaTracker, Camera camera, float worldPartialTicks, CallbackInfo ci) {
        ChunkMeshFormat.beforeExtract((LevelExtractor) (Object) this);
    }
}
