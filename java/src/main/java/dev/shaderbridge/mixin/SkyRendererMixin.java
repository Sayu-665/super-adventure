package dev.shaderbridge.mixin;

import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.render.frame.RenderBridge;
import java.util.Optional;
import java.util.OptionalDouble;
import java.util.function.Supplier;
import net.minecraft.client.renderer.SkyRenderer;
import org.joml.Vector4fc;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;

/**
 * During a pack frame the sky draws into the pack's gbuffers pass instead of Minecraft's main
 * target, so its pipelines are substituted with {@code gbuffers_skybasic} and
 * {@code gbuffers_skytextured}. Not required: without it the sky is drawn vanilla into the main
 * target, which the pack's {@code final} pass then overwrites.
 */
@Mixin(SkyRenderer.class)
abstract class SkyRendererMixin {
    @WrapOperation(
        method = "render",
        at = @At(value = "INVOKE", target = "Lcom/mojang/renderpearl/api/commands/CommandEncoder;createRenderPass(Ljava/util/function/Supplier;"
            + "Lcom/mojang/renderpearl/api/textures/GpuTextureView;Ljava/util/Optional;Lcom/mojang/renderpearl/api/textures/GpuTextureView;"
            + "Ljava/util/OptionalDouble;)Lcom/mojang/renderpearl/api/commands/RenderPass;"),
        require = 0
    )
    private RenderPass shaderbridge$skyPass(CommandEncoder encoder, Supplier<String> label, GpuTextureView color, Optional<Vector4fc> clearColor,
                                            GpuTextureView depth, OptionalDouble clearDepth, Operation<RenderPass> original) {
        return RenderBridge.skyPass(() -> original.call(encoder, label, color, clearColor, depth, clearDepth));
    }
}
