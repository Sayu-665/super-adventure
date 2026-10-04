package dev.shaderbridge.mixin;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import dev.shaderbridge.render.draw.CompiledPipelineIndex;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/**
 * Records which pipeline every compiled pipeline handed to vanilla code came from, so the pipeline
 * substitution can route draws by the vanilla pipeline's location ({@link CompiledPipelineIndex}).
 * Not required: without it every draw in a ShaderBridge pass is unknown, the first one fails and
 * the pack is disabled with a message.
 */
@Mixin(value = RenderSystem.class, remap = false)
abstract class RenderSystemMixin {
    @Inject(method = "getCompiledPipelineNullable", at = @At("RETURN"), require = 0)
    private static void shaderbridge$index(RenderPipeline pipeline, CallbackInfoReturnable<CompiledRenderPipeline> cir) {
        CompiledPipelineIndex.record(pipeline, cir.getReturnValue());
    }
}
