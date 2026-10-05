package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.device.DeviceInfo;
import dev.shaderbridge.ShaderBridge;
import dev.shaderbridge.compat.sodium.SodiumCompat;
import dev.shaderbridge.dh.CameraFarPlane;
import dev.shaderbridge.dh.DistantHorizons;
import dev.shaderbridge.gui.PackNotifier;
import dev.shaderbridge.mixin.LevelRendererAccess;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.pack.LoadedPack;
import dev.shaderbridge.render.draw.ActivePasses;
import dev.shaderbridge.render.raw.RawBackend;
import java.util.Optional;
import java.util.function.Supplier;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.client.renderer.LevelRenderer;
import net.minecraft.client.renderer.SkyRenderer;
import net.minecraft.client.renderer.chunk.ChunkSectionsToRender;
import net.minecraft.client.renderer.feature.FeatureRenderDispatcher;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import net.minecraft.client.renderer.state.level.SkyRenderState;
import org.joml.Matrix4f;
import org.joml.Matrix4fc;
import org.joml.Vector4f;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Where the render hooks (mixins) enter ShaderBridge. It keeps the renderer of the active pack in
 * the current dimension in step with {@link ShaderBridge#activePack()}, and makes every hook fail
 * soft: a failure while rendering, or a hook that did not run when it should have, abandons the
 * frame, releases the pack's render resources and tells the user; Minecraft renders vanilla until
 * another pack (or a recompile of this one) is activated. Render thread only.
 */
public final class RenderBridge {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");

    private static final Matrix4f PROJECTION = new Matrix4f();
    private static PackRenderer renderer;
    private static LoadedPack rendererPack;
    private static String rendererFolder;
    private static LoadedPack failedPack;
    private static boolean frameActive;
    private static boolean rebuildRequested;
    private static boolean projectionCaptured;
    private static RenderPass skyPassOpen;

    private RenderBridge() {
    }

    /**
     * Records the projection Minecraft renders the level with (view bobbing and nausea applied).
     *
     * @param projection the level projection
     */
    public static void captureProjection(Matrix4fc projection) {
        PROJECTION.set(projection);
        projectionCaptured = true;
    }

    /**
     * @return whether a pack is active in the current dimension, rendering or about to (Minecraft
     *     must then use classic transparency: the pack draws translucents in its gbuffers pass)
     */
    public static boolean packActive() {
        return renderer != null;
    }

    /**
     * Start of {@code LevelRenderer.render}: starts a pack frame if a pack is active.
     *
     * @param level      the level renderer
     * @param camera     the camera state
     * @param terrainFog the terrain fog block
     * @param fogColor   the fog color
     */
    public static void beginLevel(LevelRenderer level, CameraRenderState camera, GpuBufferSlice terrainFog, Vector4f fogColor) {
        if (frameActive || (renderer != null && renderer.inFrame())) {
            fail(new IllegalStateException("the previous frame did not reach Minecraft's main pass; a render hook is missing"));
        }
        frameActive = false;
        PackRenderer current = sync();
        if (current != null) {
            try {
                float partialTick = Minecraft.getInstance().gameRenderer.gameRenderState().levelRenderState.worldPartialTicks;
                Matrix4fc projection = projectionCaptured ? PROJECTION : camera.projectionMatrix;
                frameActive = current.beginFrame(level, camera, projection, partialTick, fogColor, terrainFog, ShaderBridge.get().gameState());
            } catch (RuntimeException e) {
                fail(e);
            } finally {
                projectionCaptured = false;
            }
        }
        // Distant Horizons renders later in this frame: hand its LODs to the pack only while the pack renders.
        DistantHorizons.get().beginFrame(frameActive);
        CameraFarPlane.request(frameActive ? renderer.unifiedFarPlane() : Float.NaN);
    }

    /**
     * The sky pass: draws the sky into the gbuffers pass (see {@link #skyPass}).
     *
     * @param sky   the sky renderer
     * @param fog   the sky fog block
     * @param state the sky state
     * @return whether the pass was handled (skip the vanilla body)
     */
    public static boolean runSky(SkyRenderer sky, GpuBufferSlice fog, SkyRenderState state) {
        if (!frameActive) {
            return false;
        }
        try {
            sky.render(fog, state);
        } catch (RuntimeException e) {
            fail(e);
        } finally {
            skyDone();
        }
        return true;
    }

    /**
     * The render pass {@code SkyRenderer.render} draws into: a gbuffers pass during a pack frame.
     *
     * @param vanilla creates the vanilla pass
     * @return the pass to draw the sky into
     */
    public static RenderPass skyPass(Supplier<RenderPass> vanilla) {
        if (!frameActive) {
            return vanilla.get();
        }
        RenderPass pass = renderer.openGbuffers("ShaderBridge gbuffers (sky)");
        skyPassOpen = pass;
        return pass;
    }

    /** Ends the sky pass's registration once {@code SkyRenderer.render} returned (it closes the pass itself). */
    private static void skyDone() {
        if (skyPassOpen != null) {
            ActivePasses.close(skyPassOpen);
            skyPassOpen = null;
        }
    }

    /**
     * The main pass: draws the frame with the pack, then the vanilla features that follow it.
     *
     * @param level                   the level renderer
     * @param terrainFog              the terrain fog block
     * @param improvedTransparency    Minecraft built the frame for order-independent transparency
     * @param chunks                  the frame's chunk draws
     * @param features                the frame's feature draws
     * @param hasAlwaysOnTopGizmos    the frame has always-on-top gizmos
     * @param consistentDepthRequired post effects need the depth of the world alone
     * @return whether the pass was handled (skip the vanilla body)
     */
    public static boolean runMainPass(LevelRenderer level, GpuBufferSlice terrainFog, boolean improvedTransparency, ChunkSectionsToRender chunks,
                                      FeatureRenderDispatcher.PreparedFrame features, boolean hasAlwaysOnTopGizmos, boolean consistentDepthRequired) {
        if (!frameActive) {
            return false;
        }
        if (improvedTransparency) {
            fail(new IllegalStateException("Minecraft's improved transparency could not be turned off (GameRendererMixin did not apply)"));
            return false;
        }
        LevelRendererAccess access = (LevelRendererAccess) level;
        try {
            MainPass.run(renderer, access, terrainFog, chunks, features);
        } catch (RuntimeException e) {
            fail(e);
        }
        frameActive = false;
        RenderTarget main = Minecraft.getInstance().gameRenderer.mainRenderTarget();
        access.shaderbridge$executeOutline(features);
        if (features.hasAnySeeThrough()) {
            access.shaderbridge$executeSeeThrough(features, main);
        }
        if (hasAlwaysOnTopGizmos) {
            access.shaderbridge$executeAlwaysOnTop(features, main, consistentDepthRequired);
        }
        return true;
    }

    /** Resources were reloaded: rebuild the renderer (its textures come from resource packs too). */
    public static void onResourceReload() {
        rebuildRequested = true;
    }

    /** The world was left or the client stops: release the renderer and give Distant Horizons its output back. */
    public static void release() {
        closeRenderer();
        failedPack = null;
        DistantHorizons.get().release();
        CameraFarPlane.request(Float.NaN);
    }

    /** @return the renderer of the active pack in the current dimension, created on demand, or null */
    private static PackRenderer sync() {
        Optional<LoadedPack> active = ShaderBridge.get().activePack();
        ClientLevel world = Minecraft.getInstance().level;
        if (active.isEmpty() || active.get().isClosed() || world == null || active.get() == failedPack) {
            closeRenderer();
            return null;
        }
        LoadedPack pack = active.get();
        DeviceInfo device = RenderSystem.getDevice().getDeviceInfo();
        Optional<String> blocked = SodiumCompat.blocker()
            .or(() -> DepthSupport.problem(pack.model().info().environment().depthMode(), device.isZZeroToOne(), device.backendName()));
        if (blocked.isPresent()) {
            closeRenderer();
            failedPack = pack;
            LOGGER.warn("Shader pack {} is not rendered: {}", pack.name(), blocked.get());
            PackNotifier.error(pack.name(), blocked.get());
            return null;
        }
        Optional<DimensionPipeline> dim = DimensionSelector.select(pack.model(), world.dimension().identifier().toString());
        if (dim.isEmpty()) {
            closeRenderer();
            return null;
        }
        if (renderer != null && rendererPack == pack && rendererFolder.equals(dim.get().folder()) && !rebuildRequested) {
            return renderer;
        }
        closeRenderer();
        rebuildRequested = false;
        try {
            renderer = new PackRenderer(new PackResources(pack, dim.get(), RawBackend.of(RenderSystem.getDevice())));
            rendererPack = pack;
            rendererFolder = dim.get().folder();
            LOGGER.info("Rendering with {} ({})", pack.name(), dim.get().folder().isEmpty() ? "pack root" : dim.get().folder());
        } catch (Exception e) {
            failedPack = pack;
            report(pack, e);
        }
        return renderer;
    }

    private static void fail(Exception e) {
        LoadedPack pack = rendererPack;
        if (renderer != null) {
            renderer.abandonFrame();
        }
        ActivePasses.close(skyPassOpen);
        skyPassOpen = null;
        frameActive = false;
        closeRenderer();
        failedPack = pack;
        if (pack != null) {
            report(pack, e);
        }
    }

    private static void report(LoadedPack pack, Exception e) {
        LOGGER.error("Shader pack {} stopped rendering", pack.name(), e);
        PackNotifier.error(pack.name(), "rendering failed, vanilla rendering is used: " + e.getMessage());
    }

    private static void closeRenderer() {
        if (renderer != null) {
            renderer.close();
            renderer = null;
            rendererPack = null;
            rendererFolder = null;
        }
    }
}
