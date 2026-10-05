package dev.shaderbridge.mixin.sodium;

import dev.shaderbridge.compat.sodium.SodiumTargets;
import dev.shaderbridge.compat.sodium.SodiumTerrain;
import net.caffeinemc.mods.sodium.client.model.quad.properties.ModelQuadFacing;
import net.caffeinemc.mods.sodium.client.render.chunk.translucent_sorting.TranslucentGeometryCollector;
import net.caffeinemc.mods.sodium.client.render.chunk.vertex.format.ChunkVertexEncoder;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/**
 * Tags the vertices of each translucent quad entering Sodium's sorter with the block being meshed
 * ({@link SodiumTerrain#tagQuad}) before the sorter copies them: quads the sorter splits are
 * encoded after their block was meshed and take their extension data from these tags.
 */
@Mixin(value = TranslucentGeometryCollector.class, remap = false)
abstract class TranslucentGeometryCollectorMixin {
    @Inject(method = SodiumTargets.APPEND_QUAD, at = @At("HEAD"), require = 0)
    private void shaderbridge$tagQuad(ChunkVertexEncoder.Vertex[] vertices, ModelQuadFacing facing, int packedNormal,
                                     CallbackInfoReturnable<Boolean> cir) {
        SodiumTerrain.tagQuad(vertices);
    }
}
