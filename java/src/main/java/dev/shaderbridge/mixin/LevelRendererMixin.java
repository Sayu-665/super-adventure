package dev.shaderbridge.mixin;

import com.mojang.blaze3d.resource.GraphicsResourceAllocator;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import dev.shaderbridge.render.frame.RenderBridge;
import net.minecraft.client.renderer.LevelRenderer;
import net.minecraft.client.renderer.SkyRenderer;
import net.minecraft.client.renderer.chunk.ChunkSectionsToRender;
import net.minecraft.client.renderer.feature.FeatureRenderDispatcher;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import net.minecraft.client.renderer.state.level.SkyRenderState;
import org.joml.Vector4f;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Frame hooks in {@link LevelRenderer}:
 *
 * <ul>
 *   <li>the head of {@code render} starts a pack frame (uniforms, clears, setup/begin, the shadow
 *   pass, shadowcomp and prepare);</li>
 *   <li>the sky pass and the main pass are the bodies of the frame graph passes built by
 *   {@code addSkyPass} and {@code addMainPass} (the compiler names them
 *   {@code lambda$addSkyPass$0} and {@code lambda$addMainPass$0}); during a pack frame ShaderBridge
 *   runs them instead, drawing Minecraft's geometry into the pack's gbuffers passes.</li>
 * </ul>
 *
 * None of the injections is required. If one does not apply, the frame that misses it is detected
 * at the next frame's start and the pack is disabled with a message; Minecraft renders vanilla.
 */
@Mixin(LevelRenderer.class)
abstract class LevelRendererMixin {
    @Inject(method = "render", at = @At("HEAD"), require = 0)
    private void shaderbridge$beginLevel(GraphicsResourceAllocator resourceAllocator, boolean renderOutline, CameraRenderState cameraState,
                                         GpuBufferSlice terrainFog, Vector4f fogColor, boolean shouldRenderSky, boolean consistentDepthRequired,
                                         CallbackInfo ci) {
        RenderBridge.beginLevel((LevelRenderer) (Object) this, cameraState, terrainFog, fogColor);
    }

    @Inject(method = "lambda$addSkyPass$0", at = @At("HEAD"), cancellable = true, require = 0)
    private void shaderbridge$skyPass(GpuBufferSlice skyFog, SkyRenderState state, CallbackInfo ci) {
        SkyRenderer sky = ((LevelRenderer) (Object) this).skyRenderer();
        if (sky != null && RenderBridge.runSky(sky, skyFog, state)) {
            ci.cancel();
        }
    }

    @Inject(method = "lambda$addMainPass$0", at = @At("HEAD"), cancellable = true, require = 0)
    private void shaderbridge$mainPass(GpuBufferSlice terrainFog, boolean useImprovedTransparency, ChunkSectionsToRender chunks,
                                       FeatureRenderDispatcher.PreparedFrame features, boolean hasAlwaysOnTopGizmos, boolean consistentDepthRequired,
                                       CallbackInfo ci) {
        if (RenderBridge.runMainPass((LevelRenderer) (Object) this, terrainFog, useImprovedTransparency, chunks, features, hasAlwaysOnTopGizmos,
            consistentDepthRequired)) {
            ci.cancel();
        }
    }
}
