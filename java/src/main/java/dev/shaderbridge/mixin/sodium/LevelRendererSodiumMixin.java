package dev.shaderbridge.mixin.sodium;

import dev.shaderbridge.compat.sodium.SodiumTargets;
import dev.shaderbridge.compat.sodium.SodiumTerrain;
import net.minecraft.client.renderer.LevelRenderer;
import net.minecraft.client.renderer.chunk.ChunkSectionsToRender;
import org.joml.Matrix4fc;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/**
 * With Sodium installed, Minecraft's own chunk sections are empty: Sodium replaces the call to
 * {@code prepareChunkRenders} in the level render with its own sections and never calls the
 * method. ShaderBridge's shadow pass calls it to re-draw the terrain for the shadow camera; during
 * a pack frame it gets Sodium's sections instead ({@link SodiumTerrain#shadowSections()}), whose
 * draws the pipeline substitution turns into the pack's shadow programs. Outside pack frames the
 * method is left alone.
 */
@Mixin(LevelRenderer.class)
abstract class LevelRendererSodiumMixin {
    @Inject(method = SodiumTargets.PREPARE_CHUNK_RENDERS, at = @At("HEAD"), cancellable = true, require = 0)
    private void shaderbridge$sodiumSections(Matrix4fc modelViewMatrix, boolean respectTranslucentOrder, CallbackInfoReturnable<ChunkSectionsToRender> cir) {
        ChunkSectionsToRender sodium = SodiumTerrain.shadowSections();
        if (sodium != null) {
            cir.setReturnValue(sodium);
        }
    }
}
