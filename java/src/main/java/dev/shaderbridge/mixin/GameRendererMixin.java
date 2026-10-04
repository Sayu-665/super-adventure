package dev.shaderbridge.mixin;

import dev.shaderbridge.render.frame.RenderBridge;
import net.minecraft.client.renderer.GameRenderer;
import org.joml.Matrix4f;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.ModifyArg;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/**
 * Hooks in {@link GameRenderer}: classic transparency while a pack is active (packs draw
 * translucent geometry into their gbuffers pass; Minecraft's order-independent transparency
 * renders into targets packs know nothing about), and the projection the level is rendered with
 * (view bobbing and nausea applied), which {@code gbufferProjection} is derived from.
 *
 * <p>Neither injection is required: without the first the main pass hook disables the pack with a
 * message; without the second the camera projection without view bobbing is used.
 */
@Mixin(GameRenderer.class)
abstract class GameRendererMixin {
    @Inject(method = "useImprovedTransparency", at = @At("HEAD"), cancellable = true, require = 0)
    private void shaderbridge$classicTransparency(CallbackInfoReturnable<Boolean> cir) {
        if (RenderBridge.packActive()) {
            cir.setReturnValue(false);
        }
    }

    @ModifyArg(
        method = "renderLevel",
        at = @At(value = "INVOKE", target = "Lnet/minecraft/client/renderer/ProjectionMatrixBuffer;getBuffer(Lorg/joml/Matrix4f;)"
            + "Lcom/mojang/renderpearl/api/buffers/GpuBufferSlice;"),
        require = 0
    )
    private Matrix4f shaderbridge$captureProjection(Matrix4f projection) {
        RenderBridge.captureProjection(projection);
        return projection;
    }
}
