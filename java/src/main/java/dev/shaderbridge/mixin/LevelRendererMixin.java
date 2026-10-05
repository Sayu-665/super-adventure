package dev.shaderbridge.mixin;

import com.llamalad7.mixinextras.injector.ModifyExpressionValue;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import com.mojang.blaze3d.resource.GraphicsResourceAllocator;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.render.frame.RenderBridge;
import java.util.Optional;
import java.util.OptionalDouble;
import java.util.function.Supplier;
import net.minecraft.client.renderer.LevelRenderer;
import net.minecraft.client.renderer.SkyRenderer;
import net.minecraft.client.renderer.chunk.ChunkSectionsToRender;
import net.minecraft.client.renderer.feature.FeatureRenderDispatcher;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import net.minecraft.client.renderer.state.level.SkyRenderState;
import org.joml.Vector4f;
import org.joml.Vector4fc;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Frame hooks in {@link LevelRenderer} ({@link RenderBridge} describes the frame). Nothing of
 * Minecraft's level rendering is cancelled or replaced: a pack frame wraps single calls inside it,
 * so the rest of its body, and every other mod's injection into it, still runs. In particular
 * Fabric API's world render events, {@code END_MAIN} at the end of the main pass included, fire
 * during pack frames.
 *
 * <ul>
 *   <li>the head of {@code render} starts a pack frame (uniforms, clears);</li>
 *   <li>the features {@code render} prepares are handed over to run the steps before the opaque
 *   geometry, with the shadow pass;</li>
 *   <li>{@code SkyRenderer.render} in the sky pass's body ({@code lambda$addSkyPass$0}) draws into
 *   a gbuffers pass;</li>
 *   <li>in the main pass's body ({@code lambda$addMainPass$0}): its start decides the takeover,
 *   the render pass it creates is ShaderBridge's opaque gbuffers pass, and around
 *   {@code executeSolid} and {@code executeClassicTransparency} run the pack's passes between the
 *   opaque and translucent geometry and after it; its end checks that the pack frame ended.</li>
 * </ul>
 *
 * None of the injections is required. If one does not apply, the frame that misses it is detected
 * (at the end of the main pass, or at the next frame's start) and the pack is disabled with a
 * message; Minecraft renders vanilla.
 */
@Mixin(LevelRenderer.class)
abstract class LevelRendererMixin {
    @Inject(method = "render", at = @At("HEAD"), require = 0)
    private void shaderbridge$beginLevel(GraphicsResourceAllocator resourceAllocator, boolean renderOutline, CameraRenderState cameraState,
                                         GpuBufferSlice terrainFog, Vector4f fogColor, boolean shouldRenderSky, boolean consistentDepthRequired,
                                         CallbackInfo ci) {
        RenderBridge.beginLevel((LevelRenderer) (Object) this, cameraState, terrainFog, fogColor);
    }

    @ModifyExpressionValue(method = "render", at = @At(value = "INVOKE",
        target = "Lnet/minecraft/client/renderer/feature/FeatureRenderDispatcher;prepareFrame(Lnet/minecraft/client/renderer/SubmitNodeStorage;)Lnet/minecraft/client/renderer/feature/FeatureRenderDispatcher$PreparedFrame;"),
        require = 0)
    private FeatureRenderDispatcher.PreparedFrame shaderbridge$featuresPrepared(FeatureRenderDispatcher.PreparedFrame features) {
        RenderBridge.featuresPrepared(features);
        return features;
    }

    @WrapOperation(method = "lambda$addSkyPass$0", at = @At(value = "INVOKE",
        target = "Lnet/minecraft/client/renderer/SkyRenderer;render(Lcom/mojang/renderpearl/api/buffers/GpuBufferSlice;Lnet/minecraft/client/renderer/state/level/SkyRenderState;)V"),
        require = 0)
    private void shaderbridge$sky(SkyRenderer sky, GpuBufferSlice skyFog, SkyRenderState state, Operation<Void> original) {
        RenderBridge.runSky(() -> original.call(sky, skyFog, state));
    }

    @Inject(method = "lambda$addMainPass$0", at = @At("HEAD"), require = 0)
    private void shaderbridge$mainPassStart(GpuBufferSlice terrainFog, boolean useImprovedTransparency, ChunkSectionsToRender chunks,
                                            FeatureRenderDispatcher.PreparedFrame features, boolean hasAlwaysOnTopGizmos, boolean consistentDepthRequired,
                                            CallbackInfo ci) {
        RenderBridge.mainPassStarting(useImprovedTransparency);
    }

    @WrapOperation(method = "lambda$addMainPass$0", at = @At(value = "INVOKE",
        target = "Lcom/mojang/renderpearl/api/commands/CommandEncoder;createRenderPass(Ljava/util/function/Supplier;Lcom/mojang/renderpearl/api/textures/GpuTextureView;Ljava/util/Optional;Lcom/mojang/renderpearl/api/textures/GpuTextureView;Ljava/util/OptionalDouble;)Lcom/mojang/renderpearl/api/commands/RenderPass;"),
        require = 0)
    private RenderPass shaderbridge$mainPass(CommandEncoder encoder, Supplier<String> label, GpuTextureView color, Optional<Vector4fc> clearColor,
                                             GpuTextureView depth, OptionalDouble clearDepth, Operation<RenderPass> original) {
        return RenderBridge.openMainPass(() -> original.call(encoder, label, color, clearColor, depth, clearDepth));
    }

    @WrapOperation(method = "lambda$addMainPass$0", at = @At(value = "INVOKE",
        target = "Lnet/minecraft/client/renderer/LevelRenderer;executeSolid(Lnet/minecraft/client/renderer/chunk/ChunkSectionsToRender;Lnet/minecraft/client/renderer/feature/FeatureRenderDispatcher$PreparedFrame;Lcom/mojang/renderpearl/api/commands/RenderPass;)V"),
        require = 0)
    private void shaderbridge$opaque(LevelRenderer level, ChunkSectionsToRender chunks, FeatureRenderDispatcher.PreparedFrame features, RenderPass pass,
                                     Operation<Void> original) {
        RenderBridge.drawOpaque(pass, p -> original.call(level, chunks, features, p));
    }

    @WrapOperation(method = "lambda$addMainPass$0", at = @At(value = "INVOKE",
        target = "Lnet/minecraft/client/renderer/LevelRenderer;executeClassicTransparency(Lnet/minecraft/client/renderer/chunk/ChunkSectionsToRender;Lnet/minecraft/client/renderer/feature/FeatureRenderDispatcher$PreparedFrame;Lcom/mojang/renderpearl/api/commands/RenderPass;)V"),
        require = 0)
    private void shaderbridge$translucent(LevelRenderer level, ChunkSectionsToRender chunks, FeatureRenderDispatcher.PreparedFrame features,
                                          RenderPass pass, Operation<Void> original) {
        RenderBridge.drawTranslucent(pass, p -> original.call(level, chunks, features, p));
    }

    @Inject(method = "lambda$addMainPass$0", at = @At("RETURN"), require = 0)
    private void shaderbridge$mainPassEnd(GpuBufferSlice terrainFog, boolean useImprovedTransparency, ChunkSectionsToRender chunks,
                                          FeatureRenderDispatcher.PreparedFrame features, boolean hasAlwaysOnTopGizmos, boolean consistentDepthRequired,
                                          CallbackInfo ci) {
        RenderBridge.mainPassEnded();
    }
}
