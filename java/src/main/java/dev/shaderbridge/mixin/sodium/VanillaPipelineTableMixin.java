package dev.shaderbridge.mixin.sodium;

import dev.shaderbridge.compat.sodium.SodiumPipelines;
import dev.shaderbridge.compat.sodium.SodiumTargets;
import dev.shaderbridge.render.mapping.PipelineMapping;
import dev.shaderbridge.render.mapping.VanillaPipelineTable;
import net.minecraft.resources.Identifier;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/**
 * Routes Sodium's terrain pipelines ({@code sodium:pipeline/*_terrain}) to the pack's terrain
 * programs compiled for the {@code sodium_terrain} profile ({@link SodiumPipelines}), so that the
 * pipeline substitution in ShaderBridge's render passes (the router, the program resolution, the
 * descriptor binding) treats Sodium's terrain draws like Minecraft's. Applied with the rest of the
 * Sodium integration only, so without Sodium the table is unchanged.
 */
@Mixin(value = VanillaPipelineTable.class, remap = false)
abstract class VanillaPipelineTableMixin {
    @Inject(method = SodiumTargets.TABLE_LOOKUP, at = @At("HEAD"), cancellable = true, require = 0)
    private static void shaderbridge$sodiumPipelines(Identifier location, CallbackInfoReturnable<PipelineMapping> cir) {
        SodiumPipelines.lookup(location).ifPresent(cir::setReturnValue);
    }
}
