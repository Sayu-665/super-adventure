package dev.shaderbridge.mixin.sodium;

import dev.shaderbridge.compat.sodium.SodiumTargets;
import dev.shaderbridge.compat.sodium.SodiumTerrain;
import net.caffeinemc.mods.sodium.client.render.SodiumWorldRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Keeps Sodium's terrain vertex in step with the active pack:
 *
 * <ul>
 *   <li>when Sodium creates its section manager and chunk builder ({@code initRenderer}, on world
 *   load, render distance change, {@code F3+A} and ShaderBridge's reloads), the vertex is chosen
 *   ({@link SodiumTerrain#latch()});</li>
 *   <li>at the start of every terrain update, right after chunk load events were processed (where
 *   Sodium itself reloads when the render distance changed), Sodium's renderer is reloaded when
 *   the active pack needs another vertex or other block ids ({@link SodiumTerrain#reloadNeeded()}).</li>
 * </ul>
 */
@Mixin(value = SodiumWorldRenderer.class, remap = false)
abstract class SodiumWorldRendererMixin {
    @Inject(method = SodiumTargets.INIT_RENDERER, at = @At("HEAD"), require = 0)
    private void shaderbridge$chooseVertex(CallbackInfo ci) {
        SodiumTerrain.latch();
    }

    @Inject(method = SodiumTargets.SETUP_TERRAIN, at = @At(value = "INVOKE", target = SodiumTargets.PROCESS_CHUNK_EVENTS, shift = At.Shift.AFTER),
        require = 0)
    private void shaderbridge$followPack(CallbackInfo ci) {
        if (SodiumTerrain.reloadNeeded()) {
            ((SodiumWorldRenderer) (Object) this).reload();
        }
    }
}
