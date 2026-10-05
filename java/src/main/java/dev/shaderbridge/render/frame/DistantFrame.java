package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.dh.DhDepthTargets;
import dev.shaderbridge.dh.DhMode;
import dev.shaderbridge.dh.DhPlanes;
import dev.shaderbridge.dh.DhSettings;
import dev.shaderbridge.dh.DistantHorizons;
import dev.shaderbridge.dh.LodUniforms;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.DhPipeline;
import dev.shaderbridge.render.pipeline.DepthStates;
import dev.shaderbridge.render.targets.TextureBinding;
import java.util.Optional;
import java.util.function.Consumer;
import net.minecraft.client.Minecraft;

/**
 * The Distant Horizons state of one dimension pipeline's frames: the {@link DhMode} of the current
 * frame, the LOD depth textures of {@link DhMode#NATIVE} and the uniform space of the LOD host
 * blocks. It also provides {@code dhDepthTex0/1} and the LOD block atlas to every pack program
 * ({@link MinecraftHost}). Render thread only.
 */
final class DistantFrame implements AutoCloseable {
    /** Frames the unified projection may lag behind its request before the camera hook counts as missing. */
    private static final int UNIFIED_GRACE_FRAMES = 3;

    private final DhPipeline pipeline;
    private final DepthMode depthMode;
    private final DistantHorizons dh = DistantHorizons.get();
    private final Consumer<String> diagnostics;
    private final DhDepthTargets depth;
    private final LodUniforms uniforms;
    private DhMode mode = DhMode.OFF;
    /** Consecutive frames that wanted the unified projection without Minecraft's projection reaching the DH far plane. */
    private int unifiedMisses;

    /**
     * @param device      the GPU device
     * @param pipeline    the dimension's Distant Horizons configuration
     * @param depthMode   the pack's depth convention
     * @param diagnostics receives problems (deduplicated by the receiver)
     */
    DistantFrame(GpuDevice device, DhPipeline pipeline, DepthMode depthMode, Consumer<String> diagnostics) {
        this.pipeline = pipeline;
        this.depthMode = depthMode;
        this.diagnostics = diagnostics;
        this.depth = new DhDepthTargets(device);
        this.uniforms = new LodUniforms(device, device.getDeviceInfo().limits().minUniformOffsetAlignment());
    }

    /** @return the mode the pack would draw LODs with, given Distant Horizons' current state */
    DhMode wanted() {
        return DhMode.select(pipeline, dh.settings().isPresent());
    }

    /**
     * @return the far plane Minecraft's projection must reach in the next frames
     *     ({@link DhMode#SYNTHESIZED}), or NaN for its own
     */
    float unifiedFarPlane() {
        Optional<DhSettings> settings = dh.settings();
        return wanted() == DhMode.SYNTHESIZED && settings.isPresent() ? DhPlanes.farPlane(settings.get().lodChunks()) : Float.NaN;
    }

    /**
     * Starts a frame after the frame state is captured: decides the frame's mode and clears the
     * LOD depth.
     *
     * @param encoder  a command encoder
     * @param dhActive the frame state reports Distant Horizons as rendering ({@code FrameState.dhActive})
     */
    void beginFrame(CommandEncoder encoder, boolean dhActive) {
        DhMode wanted = wanted();
        mode = dhActive ? wanted : DhMode.OFF;
        // The projection reaches the DH far plane one frame after it is requested; longer means the camera hook is missing.
        unifiedMisses = wanted == DhMode.SYNTHESIZED && !dhActive ? unifiedMisses + 1 : 0;
        if (unifiedMisses == UNIFIED_GRACE_FRAMES) {
            diagnostics.accept("Minecraft's projection does not reach the Distant Horizons far plane (CameraMixin did not apply); "
                + "the pack's synthesized LODs are not drawn");
        }
        uniforms.beginFrame();
        if (mode.separateDepth()) {
            RenderTarget main = Minecraft.getInstance().gameRenderer.mainRenderTarget();
            depth.beginFrame(encoder, main.width, main.height, DepthStates.clearValue(depthMode));
        }
    }

    /** @return the current frame's mode */
    DhMode mode() {
        return mode;
    }

    /** @return the integration with Distant Horizons */
    DistantHorizons dh() {
        return dh;
    }

    /** @return the uniform space of the LOD host blocks */
    LodUniforms uniforms() {
        return uniforms;
    }

    /** @return the depth attachment of the gbuffers LOD passes: the LOD depth, or Minecraft's when shared */
    GpuTextureView gbuffersDepth() {
        return mode.separateDepth() ? depth.attachment() : Minecraft.getInstance().gameRenderer.mainRenderTarget().getDepthTextureView();
    }

    /**
     * Copies the LOD depth into {@code dhDepthTex<index>} ({@link DhMode#NATIVE} only).
     *
     * @param index 1 after the opaque LODs, 0 after the translucent ones
     */
    void copyDepth(int index) {
        if (mode.separateDepth()) {
            depth.copy(RenderSystem.getDevice().createCommandEncoder(), index);
        }
    }

    /**
     * @param index 0 or 1
     * @return {@code dhDepthTex<index>} while native LODs are drawn
     */
    Optional<GpuTextureView> depthTexture(int index) {
        return mode.separateDepth() ? Optional.ofNullable(depth.sampled(index)) : Optional.empty();
    }

    /** @return Distant Horizons' LOD block atlas while LODs are drawn */
    Optional<TextureBinding> blockAtlas() {
        return mode.drawsLods() ? dh.blockAtlas() : Optional.empty();
    }

    @Override
    public void close() {
        uniforms.close();
        depth.close();
    }
}
