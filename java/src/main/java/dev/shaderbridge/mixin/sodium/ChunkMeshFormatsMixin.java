package dev.shaderbridge.mixin.sodium;

import dev.shaderbridge.compat.sodium.SodiumTargets;
import dev.shaderbridge.compat.sodium.SodiumTerrain;
import net.caffeinemc.mods.sodium.client.render.chunk.vertex.format.ChunkMeshFormats;
import net.caffeinemc.mods.sodium.client.render.chunk.vertex.format.ChunkVertexType;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/**
 * Makes Sodium mesh ShaderBridge's extended terrain vertex while a pack is active
 * ({@link SodiumTerrain#meshFormat()}). Sodium reads the format when it creates its section
 * manager, chunk builder, region buffers and buffer arenas, all of which belong to one renderer,
 * and ShaderBridge changes its answer only when that renderer is recreated.
 */
@Mixin(value = ChunkMeshFormats.class, remap = false)
abstract class ChunkMeshFormatsMixin {
    @Inject(method = SodiumTargets.GET_CURRENT, at = @At("HEAD"), cancellable = true, require = 0)
    private static void shaderbridge$extendedVertex(CallbackInfoReturnable<ChunkVertexType> cir) {
        ChunkVertexType extended = SodiumTerrain.meshFormat();
        if (extended != null) {
            cir.setReturnValue(extended);
        }
    }
}
