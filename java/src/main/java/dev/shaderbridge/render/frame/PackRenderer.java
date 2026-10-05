package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.commands.RenderPass;
import dev.shaderbridge.compat.sodium.SodiumIntegration;
import dev.shaderbridge.dh.DhMode;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.Pass;
import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.ShadowSettings;
import dev.shaderbridge.render.chunk.ChunkMeshFormat;
import dev.shaderbridge.render.pipeline.AttachmentLayout;
import dev.shaderbridge.render.pipeline.PipelineDiagnostics;
import dev.shaderbridge.render.pipeline.ProgramResolution;
import dev.shaderbridge.render.shadow.ShadowCulling;
import dev.shaderbridge.render.shadow.ShadowPlan;
import dev.shaderbridge.render.shadow.ShadowRenderer;
import dev.shaderbridge.render.shadow.ShadowSections;
import dev.shaderbridge.render.shadow.ShadowTransforms;
import dev.shaderbridge.render.targets.Rgba;
import dev.shaderbridge.uniforms.GameStateCapture;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.LevelRenderer;
import net.minecraft.client.renderer.feature.FeatureRenderDispatcher;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import org.joml.Matrix4f;
import org.joml.Matrix4fc;
import org.joml.Vector3d;
import org.joml.Vector4f;

/**
 * Renders one frame of a dimension pipeline around Minecraft's own level rendering:
 *
 * <ol>
 *   <li>{@link #beginFrame} (start of {@code LevelRenderer.render}): game state and uniforms, the
 *   frame's clears;</li>
 *   <li>{@link #beforeGeometry} (once Minecraft has prepared the frame's features, before its sky
 *   and main passes): every step before the opaque geometry ({@code setup} on the first frame,
 *   {@code begin}, the shadow pass with the prepared entities, {@code shadowcomp},
 *   {@code prepare});</li>
 *   <li>Minecraft draws the sky and, after the opaque Distant Horizons LODs
 *   ({@link #drawDistantOpaque}), the opaque geometry into {@link #openGbuffers gbuffers passes};</li>
 *   <li>{@link #afterOpaque}: the {@code depthtex1}/{@code depthtex2} copies, then {@code deferred};</li>
 *   <li>after the translucent LODs ({@link #drawDistantWater}), Minecraft draws the translucent
 *   geometry into another gbuffers pass;</li>
 *   <li>{@link #finishFrame}: {@code composite}, {@code final} into Minecraft's main color target,
 *   the end-of-frame copies.</li>
 * </ol>
 *
 * A frame only renders with the pack once its composite-style pipelines and compute programs are compiled; until then
 * Minecraft renders vanilla. Render thread only.
 */
final class PackRenderer implements FrameSteps, AutoCloseable {
    /** Programs that draw Minecraft's chunk meshes. */
    private static final List<GeometryProgram> TERRAIN_SLOTS = List.of(GeometryProgram.TERRAIN, GeometryProgram.TERRAIN_SOLID,
        GeometryProgram.TERRAIN_CUTOUT, GeometryProgram.WATER);
    /** What terrain programs see of chunk vertices when the extended terrain vertex format cannot be used. */
    static final String TERRAIN_VERTEX_NOTE = "chunk terrain keeps Minecraft's own vertices (%s): they carry no normals, block ids or "
        + "mid-texture coordinates, so terrain and water programs read the normal (0, 1, 0), mc_Entity -1 and each vertex's own texture "
        + "coordinate, and effects that depend on them (water and foliage detection, waving plants, material ids, normal-based lighting) "
        + "do not work on terrain";

    private final PackResources r;
    private final GeometryPasses geometry;
    private final FullscreenPasses fullscreen;
    private final ComputeDispatcher computes;
    private final ShadowRenderer shadows;
    private final DistantPasses distant;
    private LevelRenderer level;
    private GpuBufferSlice frameSlice;
    private boolean firstFrame = true;
    /** The pack has a program for Minecraft's chunk terrain. */
    private final boolean drawsTerrain;
    private boolean terrainNoteReported;
    /** The frame's prepared features while {@link #beforeGeometry} runs. */
    private FeatureRenderDispatcher.PreparedFrame features;
    /** Drawing features into the shadow pass failed once: they cast no shadows for this pack. */
    private boolean featureShadowsFailed;

    /** @param resources the pack's resources for the current dimension; owned by this renderer */
    PackRenderer(PackResources resources) {
        this.r = resources;
        this.geometry = new GeometryPasses(resources);
        this.fullscreen = new FullscreenPasses(resources);
        this.computes = new ComputeDispatcher(resources);
        this.distant = new DistantPasses(resources, geometry);
        ShadowPlan plan = ShadowPlan.of(resources.dim.targets().shadow(),
            DhMode.castsShadows(resources.dim.distantHorizons(), resources.dim.targets().shadow()));
        plan.notes().forEach(n -> resources.diagnostics.report(resources.dim.folder() + ": " + n));
        this.shadows = new ShadowRenderer(plan, ShadowSections.SHADOW_FRUSTUM);
        this.drawsTerrain = TERRAIN_SLOTS.stream().anyMatch(resources.dim.geometry()::containsKey);
    }

    /**
     * The note on chunk vertices for a pack that draws terrain: present only when the extended
     * terrain vertex (normals, block ids, mid-texture coordinates, tangents, {@code at_midBlock})
     * cannot be used. Sodium's terrain is extended by its own integration (packs are refused when
     * that cannot work), so with Sodium there is nothing to report.
     *
     * @param sodiumShadesTerrain Sodium's integration is active (it meshes and draws terrain)
     * @param unavailable         why Minecraft's chunk meshes cannot use the extended vertex, if so
     * @return the diagnostic, or empty
     */
    static Optional<String> terrainVertexNote(boolean sodiumShadesTerrain, Optional<String> unavailable) {
        if (sodiumShadesTerrain) {
            return Optional.empty();
        }
        return unavailable.map(reason -> String.format(TERRAIN_VERTEX_NOTE, reason));
    }

    /** Reports {@link #terrainVertexNote} once, as soon as the chunk mesh format knows it cannot switch. */
    private void reportTerrainVertices() {
        if (!drawsTerrain || terrainNoteReported) {
            return;
        }
        terrainVertexNote(SodiumIntegration.active(), ChunkMeshFormat.unavailableReason()).ifPresent(note -> {
            terrainNoteReported = true;
            r.diagnostics.report(r.dim.folder() + ": " + note);
        });
    }

    /**
     * Starts a frame and runs everything before the opaque geometry.
     *
     * @param level       the level renderer
     * @param camera      the frame's camera state
     * @param projection  the projection the level is rendered with
     * @param partialTick the frame's partial tick
     * @param fogColor    the fog color
     * @param terrainFog  the terrain fog uniform block
     * @param gameState   reads the game into the frame state
     * @return whether the frame renders with the pack (false while its pipelines compile)
     */
    boolean beginFrame(LevelRenderer level, CameraRenderState camera, Matrix4fc projection, float partialTick, Vector4f fogColor,
                       GpuBufferSlice terrainFog, GameStateCapture gameState) {
        r.pipelines.poll();
        if (r.pipelines.injectionInactive()) {
            throw new IllegalStateException("the SPIR-V injection hook (GlslCompilerMixin) is not active");
        }
        if (!compositesReady()) {
            return false;
        }
        reportTerrainVertices();
        RenderTarget main = Minecraft.getInstance().gameRenderer.mainRenderTarget();
        boolean resized = r.targets.resize(main.width, main.height);
        if (resized) {
            r.sinks.close();
            r.passCopies.close();
        }
        r.atlases.refresh();
        gameState.capture(r.frameState, camera, projection, partialTick);
        float centerDepth = r.centerDepth.latest();
        if (Float.isFinite(centerDepth)) {
            r.frameState.centerDepth = centerDepth;
        }
        r.frameState.update();
        CommandEncoder encoder = RenderSystem.getDevice().createCommandEncoder();
        r.frameUniforms.fill(r.frameState, r.frameState.timer().frameTime());
        frameSlice = r.frameUniforms.upload(encoder);
        r.drawUniforms.beginFrame(encoder);
        r.drawSlots.prepare(r.frameState, r.drawUniforms::push);
        r.targets.clear(encoder, new Rgba(fogColor.x, fogColor.y, fogColor.z, 1), r.depthMode);
        r.distant.beginFrame(encoder, r.frameState.dhActive);
        RenderSystem.setShaderFog(terrainFog);
        this.level = level;
        // As Iris: setup computes run again after a resize, which recreated (and cleared) the
        // screen-relative targets, custom images and storage buffers they initialize.
        r.sequencer.begin(firstFrame || resized);
        firstFrame = false;
        if (shadows.drawsFeatures() && !featureShadowsFailed) {
            ShadowTransforms.startRecording();
        } else {
            ShadowTransforms.stop();
        }
        return true;
    }

    /**
     * Runs everything before the opaque geometry: {@code setup} on the first frame, {@code begin},
     * the shadow pass, {@code shadowcomp} and {@code prepare}. Runs once Minecraft has prepared
     * the frame's features, so the shadow pass can draw them, and before its sky and main passes.
     *
     * @param features the frame's prepared features, or null when they are not known (the shadow
     *                 pass then draws no entities)
     */
    void beforeGeometry(FeatureRenderDispatcher.PreparedFrame features) {
        this.features = features;
        try {
            r.sequencer.runUntil(PassGroup.GBUFFERS_OPAQUE, this);
        } finally {
            this.features = null;
            ShadowTransforms.stop();
        }
    }

    /** @return the diagnostics of the pack in this dimension (deduplicated, also shown in the pack screen) */
    PipelineDiagnostics diagnostics() {
        return r.diagnostics;
    }

    /**
     * Opens a gbuffers pass for Minecraft's geometry.
     *
     * @param label debug label
     * @return the pass; close it, then call {@link #closed}
     */
    RenderPass openGbuffers(String label) {
        return geometry.openGbuffers(label, r.sequencer.flips(), frameSlice);
    }

    /**
     * A pass from {@link #openGbuffers} was closed.
     *
     * @param pass the pass
     */
    void closed(RenderPass pass) {
        geometry.closed(pass);
    }

    /** Draws the opaque Distant Horizons LODs of this frame (before Minecraft's opaque geometry). */
    void drawDistantOpaque() {
        distant.drawOpaque(r.sequencer.flips(), frameSlice);
    }

    /** Draws the translucent Distant Horizons LODs of this frame (before Minecraft's translucent geometry). */
    void drawDistantWater() {
        distant.drawWater(r.sequencer.flips(), frameSlice);
    }

    /**
     * @return the far plane Minecraft's projection must reach in the next frames (synthesized
     *     Distant Horizons LODs), or NaN for its own
     */
    float unifiedFarPlane() {
        return r.distant.unifiedFarPlane();
    }

    /** The opaque geometry is drawn: copies the depth, then runs everything up to the translucent geometry. */
    void afterOpaque() {
        CommandEncoder encoder = RenderSystem.getDevice().createCommandEncoder();
        RenderTarget main = Minecraft.getInstance().gameRenderer.mainRenderTarget();
        // centerDepthSmooth: the centre depth after the opaque geometry, read back for a later frame.
        r.centerDepth.sample(encoder, main.getDepthTexture());
        r.targets.copyMainDepth(encoder, main.getDepthTexture(), 2);
        r.targets.copyMainDepth(encoder, main.getDepthTexture(), 1);
        r.sequencer.runUntil(PassGroup.GBUFFERS_TRANSLUCENT, this);
    }

    /** The translucent geometry is drawn: runs the composite passes and {@code final}, and ends the frame. */
    void finishFrame() {
        r.sequencer.finish(this);
        r.drawUniforms.endFrame(RenderSystem.getDevice().createCommandEncoder());
        level = null;
    }

    /** Gives up the current frame after a failure. */
    void abandonFrame() {
        r.sequencer.abandon();
        level = null;
    }

    /** @return whether a frame was begun and not finished */
    boolean inFrame() {
        return r.sequencer.inFrame();
    }

    /**
     * Requests every composite-style pipeline and every compute program; true once none is still
     * compiling ({@code setup} computes run on the first frame only, so they must be ready by then).
     */
    private boolean compositesReady() {
        boolean ready = true;
        for (FramePlan.Step step : r.sequencer.plan().steps()) {
            switch (step) {
                case FramePlan.Step.Fullscreen f -> ready &= !(r.programs.program(f.program(), fullscreen.layout(f.program(), f.group()))
                    instanceof ProgramResolution.Pending);
                case FramePlan.Step.Computes c -> ready &= computesReady(c.pass());
                case FramePlan.Step.Geometry g when g.pass() != null -> ready &= computesReady(g.pass());
                default -> {
                }
            }
        }
        return ready;
    }

    private boolean computesReady(Pass pass) {
        boolean ready = true;
        for (int index : pass.computes()) {
            ready &= !(r.programs.program(index, AttachmentLayout.fullscreen(r.dim, r.dim.programs().get(index))) instanceof ProgramResolution.Pending);
        }
        return ready;
    }

    @Override
    public void passStarted(Pass pass) {
        // Passes need no setup of their own; their steps follow.
    }

    @Override
    public void implicitGeometry(PassGroup group) {
        // Geometry without a pass of its own is drawn like geometry with one.
    }

    @Override
    public void computes(Pass pass, FlipState flips) {
        computes.dispatch(pass, flips, frameSlice);
    }

    @Override
    public void shadow(FlipState flips) {
        shadows.render(level, shadowView(), new ShadowRenderer.ShadowTargets() {
            @Override
            public RenderPass open(String label) {
                return geometry.openShadow(label, flips, frameSlice);
            }

            @Override
            public void closed(RenderPass pass) {
                geometry.closed(pass);
            }

            @Override
            public void copyDepth() {
                r.targets.copyShadowDepth(RenderSystem.getDevice().createCommandEncoder());
            }

            @Override
            public void drawDistant() {
                distant.drawShadow(flips, frameSlice);
            }

            @Override
            public void drawFeatures() {
                drawShadowFeatures(flips);
            }
        });
        if (r.dim.targets().shadow().enabled()) {
            r.targets.shadowColorTargets().forEach(pair -> r.mips.generate(pair, flips.shadowRead(pair.spec().index())));
        }
    }

    /** @return the frame's shadow camera, for culling the shadow pass's terrain */
    private ShadowSections.ShadowView shadowView() {
        ShadowSettings settings = r.dim.targets().shadow();
        Vector3d camera = r.frameState.cameraPosition;
        double distance = ShadowCulling.renderDistance(settings.distance(), settings.distanceRenderMul(), r.frameState.renderDistanceBlocks);
        return new ShadowSections.ShadowView(new Matrix4f(r.frameState.shadowModelView()), new Matrix4f(r.frameState.shadowProjection()),
            camera.x, camera.y, camera.z, distance, ShadowSections.ShadowView.culls(settings.culling()));
    }

    /**
     * The shadow pass's feature step: the frame's prepared opaque features drawn again with the
     * shadow camera's model-view ({@link ShadowTransforms}). A failure here (a feature renderer
     * that cannot draw into the shadow pass) turns feature shadows off for this pack and is
     * reported; the frame goes on.
     */
    private void drawShadowFeatures(FlipState flips) {
        FeatureRenderDispatcher.PreparedFrame prepared = features;
        if (featureShadowsFailed) {
            return;
        }
        if (prepared == null) {
            r.diagnostics.report(r.dim.folder() + ": Minecraft's prepared features were not handed over (LevelRendererMixin did not apply); "
                + "entities cast no shadows");
            return;
        }
        if (prepared.isEmpty()) {
            ShadowTransforms.stop();
            return;
        }
        Matrix4f viewInverse = new Matrix4f(r.frameState.gbufferModelView()).invert();
        Map<GpuBufferSlice, GpuBufferSlice> transforms = ShadowTransforms.writeShadowCopies(r.frameState.shadowModelView(), viewInverse);
        if (transforms.isEmpty()) {
            r.diagnostics.report(r.dim.folder() + ": the features' transforms were not recorded (DynamicGpuDataMixin did not apply); "
                + "entities cast no shadows");
            return;
        }
        RenderPass pass = geometry.openShadowFeatures("ShaderBridge shadow features", flips, frameSlice, transforms);
        try (pass) {
            prepared.executeSolid(pass);
        } catch (RuntimeException e) {
            featureShadowsFailed = true;
            r.diagnostics.report(r.dim.folder() + ": entities stopped casting shadows, drawing them into the shadow pass failed: " + e);
        } finally {
            geometry.closed(pass);
        }
    }

    @Override
    public Drawn fullscreen(int program, PassGroup group, FlipState flips) {
        return fullscreen.draw(program, group, flips, frameSlice);
    }

    @Override
    public void copyToOutput(FlipState flips) {
        fullscreen.copyToOutput(flips);
    }

    @Override
    public void endOfFrame(List<Integer> color, List<Integer> shadow) {
        fullscreen.endOfFrame(color, shadow);
    }

    @Override
    public void warn(String message) {
        r.diagnostics.report(r.dim.folder() + ": " + message);
    }

    @Override
    public void close() {
        r.close();
    }
}
