package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.device.DeviceInfo;
import dev.shaderbridge.ShaderBridge;
import dev.shaderbridge.compat.sodium.SodiumCompat;
import dev.shaderbridge.dh.CameraFarPlane;
import dev.shaderbridge.dh.DistantHorizons;
import dev.shaderbridge.gui.PackNotifier;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.pack.LoadedPack;
import dev.shaderbridge.render.draw.ActivePasses;
import dev.shaderbridge.render.raw.RawBackend;
import dev.shaderbridge.render.shadow.ShadowTransforms;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.function.Consumer;
import java.util.function.Supplier;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.client.renderer.LevelRenderer;
import net.minecraft.client.renderer.feature.FeatureRenderDispatcher;
import net.minecraft.client.renderer.state.level.CameraRenderState;
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
 * another pack (or a recompile of this one) is activated.
 *
 * <p>A pack frame takes Minecraft's level rendering over without replacing any of it, so other
 * mods' hooks in it keep running (Fabric API's world render events among them):
 *
 * <ol>
 *   <li>{@link #beginLevel} (start of {@code LevelRenderer.render}): uniforms and clears;</li>
 *   <li>{@link #featuresPrepared}: once Minecraft has prepared the frame's features, the steps
 *   before the opaque geometry, the shadow pass with those features among them;</li>
 *   <li>{@link #runSky}: the sky pass draws into a gbuffers pass ({@link #skyPass});</li>
 *   <li>the main pass runs as Minecraft wrote it, around the calls ShaderBridge wraps: the pass it
 *   creates is ShaderBridge's opaque gbuffers pass, preceded by the opaque Distant Horizons LODs
 *   ({@link #openMainPass}); {@code executeSolid} draws into it ({@link #drawOpaque}); before
 *   {@code executeClassicTransparency} that pass is closed, the deferred passes and the translucent
 *   LODs run and another gbuffers pass is opened, after it the composite passes and
 *   {@code final} run ({@link #drawTranslucent}); outlines, see-through and always-on-top
 *   features then draw as usual, into Minecraft's main target.</li>
 * </ol>
 *
 * Render thread only.
 */
public final class RenderBridge {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");

    private static final Matrix4f PROJECTION = new Matrix4f();
    private static PackRenderer renderer;
    private static LoadedPack rendererPack;
    private static String rendererFolder;
    private static LoadedPack failedPack;
    /** A pack frame is in progress: begun, neither finished nor abandoned. */
    private static boolean frameActive;
    /** The steps before the opaque geometry ran in the current frame. */
    private static boolean preGeometryDone;
    /** The current main pass is ShaderBridge's. */
    private static boolean mainTakeover;
    /** The opaque gbuffers pass handed to Minecraft's main pass, until it is closed. */
    private static RenderPass opaquePass;
    /**
     * The opaque gbuffers pass closed after a failure in it, until the main pass ends: Minecraft
     * still holds it, and nothing more may be drawn into it.
     */
    private static RenderPass abandonedPass;
    private static boolean rebuildRequested;
    private static boolean projectionCaptured;
    private static RenderPass skyPassOpen;
    /** What the last renderer reported before it stopped (shown in the pack screen). */
    private static List<String> retiredDiagnostics = List.of();

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
     * @return the rendering diagnostics of the active pack in this dimension (programs that fall
     *     back or are skipped, unsupported features, draws that cannot be shaded), or those of the
     *     last pack that stopped rendering, with the reason it stopped; render thread
     */
    public static List<String> diagnostics() {
        return renderer != null ? renderer.diagnostics().messages() : retiredDiagnostics;
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
            fail(new IllegalStateException("the previous frame did not reach the end of Minecraft's main pass; a render hook is missing"));
        }
        frameActive = false;
        preGeometryDone = false;
        mainTakeover = false;
        opaquePass = null;
        abandonedPass = null;
        ShadowTransforms.stop();
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
        if (!frameActive) {
            ShadowTransforms.stop();
        }
        // Distant Horizons renders later in this frame: hand its LODs to the pack only while the pack renders.
        DistantHorizons.get().beginFrame(frameActive);
        CameraFarPlane.request(frameActive ? renderer.unifiedFarPlane() : Float.NaN);
    }

    /**
     * Minecraft prepared the frame's features ({@code FeatureRenderDispatcher.prepareFrame} in
     * {@code LevelRenderer.render}): runs the steps before the opaque geometry, the shadow pass
     * (which draws these features again for the shadow camera) among them.
     *
     * @param features the frame's prepared features
     */
    public static void featuresPrepared(FeatureRenderDispatcher.PreparedFrame features) {
        if (frameActive && !preGeometryDone) {
            runPreGeometry(features);
        }
    }

    private static void runPreGeometry(FeatureRenderDispatcher.PreparedFrame features) {
        preGeometryDone = true;
        try {
            renderer.beforeGeometry(features);
        } catch (RuntimeException e) {
            fail(e);
        }
    }

    /** The steps before the opaque geometry when the feature hook did not run them (no entity shadows then). */
    private static void ensurePreGeometry() {
        if (frameActive && !preGeometryDone) {
            runPreGeometry(null);
        }
    }

    /**
     * The sky pass ({@code SkyRenderer.render} in the frame graph's sky pass): during a pack frame
     * the sky draws into a gbuffers pass (see {@link #skyPass}); a failure abandons the frame.
     *
     * @param vanilla draws the sky
     */
    public static void runSky(Runnable vanilla) {
        ensurePreGeometry();
        if (!frameActive) {
            vanilla.run();
            return;
        }
        try {
            vanilla.run();
        } catch (RuntimeException e) {
            fail(e);
        } finally {
            skyDone();
        }
    }

    /**
     * The render pass {@code SkyRenderer.render} draws into: a gbuffers pass during a pack frame.
     *
     * @param vanilla creates the vanilla pass
     * @return the pass to draw the sky into
     */
    public static RenderPass skyPass(Supplier<RenderPass> vanilla) {
        ensurePreGeometry();
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
     * Start of Minecraft's main pass: decides whether the pack takes it over.
     *
     * @param improvedTransparency Minecraft built the frame for order-independent transparency
     */
    public static void mainPassStarting(boolean improvedTransparency) {
        mainTakeover = false;
        opaquePass = null;
        abandonedPass = null;
        if (!frameActive) {
            return;
        }
        if (improvedTransparency) {
            fail(new IllegalStateException("Minecraft's improved transparency could not be turned off (GameRendererMixin did not apply)"));
            return;
        }
        ensurePreGeometry();
        mainTakeover = frameActive;
    }

    /**
     * The render pass Minecraft's main pass creates: during a pack frame, the opaque Distant
     * Horizons LODs are drawn and an opaque gbuffers pass is returned instead.
     *
     * @param vanilla creates Minecraft's pass
     * @return the pass the opaque geometry is drawn into
     */
    public static RenderPass openMainPass(Supplier<RenderPass> vanilla) {
        if (!mainTakeover || !frameActive) {
            return vanilla.get();
        }
        try {
            // Distant Horizons handed its LODs over in prepareTranslucents, just before.
            renderer.drawDistantOpaque();
            opaquePass = renderer.openGbuffers("ShaderBridge gbuffers (opaque)");
            return opaquePass;
        } catch (RuntimeException e) {
            fail(e);
            return vanilla.get();
        }
    }

    /**
     * Minecraft's opaque geometry ({@code executeSolid}).
     *
     * @param pass    the pass Minecraft draws into
     * @param vanilla draws the opaque geometry into a pass
     */
    public static void drawOpaque(RenderPass pass, Consumer<RenderPass> vanilla) {
        if (opaquePass == null || pass != opaquePass) {
            vanilla.accept(pass);
            return;
        }
        try {
            vanilla.accept(pass);
        } catch (RuntimeException e) {
            abandonedPass = pass;
            try {
                closeOpaque();
            } catch (RuntimeException closing) {
                e.addSuppressed(closing);
            }
            fail(e);
        }
    }

    /**
     * Minecraft's classic transparency ({@code executeClassicTransparency}). During a pack frame:
     * closes the opaque gbuffers pass, runs the depth copies and the deferred passes and draws the
     * translucent LODs, lets Minecraft draw its translucent geometry into a new gbuffers pass, then
     * runs the composite passes and {@code final}, which end the pack frame. After a failure in
     * the opaque pass (which closed it), the translucent geometry is not drawn this frame.
     *
     * @param pass    the pass Minecraft draws into (the opaque gbuffers pass during a pack frame)
     * @param vanilla draws the translucent geometry into a pass
     */
    public static void drawTranslucent(RenderPass pass, Consumer<RenderPass> vanilla) {
        if (pass != null && pass == abandonedPass) {
            return;
        }
        if (opaquePass == null || pass != opaquePass) {
            vanilla.accept(pass);
            return;
        }
        closeOpaque();
        if (!frameActive) {
            return;
        }
        try {
            renderer.afterOpaque();
            renderer.drawDistantWater();
            RenderPass translucent = renderer.openGbuffers("ShaderBridge gbuffers (translucent)");
            try (translucent) {
                vanilla.accept(translucent);
            } finally {
                renderer.closed(translucent);
            }
            renderer.finishFrame();
            frameActive = false;
        } catch (RuntimeException e) {
            fail(e);
        }
    }

    /** End of Minecraft's main pass: a pack frame must have ended by now. */
    public static void mainPassEnded() {
        abandonedPass = null;
        if (opaquePass != null) {
            // The translucent hook did not run; Minecraft closed the pass itself.
            ActivePasses.close(opaquePass);
            opaquePass = null;
        }
        if (mainTakeover && frameActive) {
            fail(new IllegalStateException("Minecraft's main pass ended before the pack frame did (LevelRendererMixin did not fully apply)"));
        }
        mainTakeover = false;
    }

    /** Closes the opaque gbuffers pass (Minecraft's own close of it is then a no-op). */
    private static void closeOpaque() {
        RenderPass pass = opaquePass;
        opaquePass = null;
        if (pass != null) {
            try {
                pass.close();
            } finally {
                ActivePasses.close(pass);
            }
        }
    }

    /** Resources were reloaded: rebuild the renderer (its textures come from resource packs too). */
    public static void onResourceReload() {
        rebuildRequested = true;
    }

    /** The world was left or the client stops: release the renderer and give Distant Horizons its output back. */
    public static void release() {
        closeRenderer();
        failedPack = null;
        retiredDiagnostics = List.of();
        ShadowTransforms.stop();
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
            retiredDiagnostics = List.of("not rendered: " + blocked.get());
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
            retiredDiagnostics = List.of();
            LOGGER.info("Rendering with {} ({})", pack.name(), dim.get().folder().isEmpty() ? "pack root" : dim.get().folder());
        } catch (Exception e) {
            failedPack = pack;
            retiredDiagnostics = List.of("render resources could not be created: " + e.getMessage());
            report(pack, e);
        }
        return renderer;
    }

    private static void fail(Exception e) {
        LoadedPack pack = rendererPack;
        List<String> diagnostics = new ArrayList<>(renderer != null ? renderer.diagnostics().messages() : List.of());
        if (renderer != null) {
            renderer.abandonFrame();
        }
        ActivePasses.close(skyPassOpen);
        skyPassOpen = null;
        ActivePasses.close(opaquePass);
        opaquePass = null;
        frameActive = false;
        mainTakeover = false;
        ShadowTransforms.stop();
        closeRenderer();
        failedPack = pack;
        if (pack != null) {
            diagnostics.add("stopped rendering: " + e.getMessage());
            retiredDiagnostics = List.copyOf(diagnostics);
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
