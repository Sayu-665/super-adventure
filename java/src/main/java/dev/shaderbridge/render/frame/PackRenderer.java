package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.commands.RenderPass;
import dev.shaderbridge.model.Pass;
import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.render.pipeline.AttachmentLayout;
import dev.shaderbridge.render.pipeline.ProgramResolution;
import dev.shaderbridge.render.shadow.ShadowPlan;
import dev.shaderbridge.render.shadow.ShadowRenderer;
import dev.shaderbridge.render.shadow.ShadowSections;
import dev.shaderbridge.render.targets.Rgba;
import dev.shaderbridge.uniforms.GameStateCapture;
import java.util.List;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.LevelRenderer;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import org.joml.Matrix4fc;
import org.joml.Vector4f;

/**
 * Renders one frame of a dimension pipeline around Minecraft's own level rendering:
 *
 * <ol>
 *   <li>{@link #beginFrame} (start of {@code LevelRenderer.render}): game state and uniforms, the
 *   frame's clears, then every step before the opaque geometry ({@code setup} on the first frame,
 *   {@code begin}, the shadow pass, {@code shadowcomp}, {@code prepare});</li>
 *   <li>Minecraft draws the sky and the opaque geometry into {@link #openGbuffers gbuffers passes};</li>
 *   <li>{@link #afterOpaque}: the {@code depthtex1}/{@code depthtex2} copies, then {@code deferred};</li>
 *   <li>Minecraft draws the translucent geometry into another gbuffers pass;</li>
 *   <li>{@link #finishFrame}: {@code composite}, {@code final} into Minecraft's main color target,
 *   the end-of-frame copies.</li>
 * </ol>
 *
 * A frame only renders with the pack once its composite-style pipelines are compiled; until then
 * Minecraft renders vanilla. Render thread only.
 */
final class PackRenderer implements FrameSteps, AutoCloseable {
    private final PackResources r;
    private final GeometryPasses geometry;
    private final FullscreenPasses fullscreen;
    private final ComputeDispatcher computes;
    private final ShadowRenderer shadows;
    private LevelRenderer level;
    private GpuBufferSlice frameSlice;
    private boolean firstFrame = true;

    /** @param resources the pack's resources for the current dimension; owned by this renderer */
    PackRenderer(PackResources resources) {
        this.r = resources;
        this.geometry = new GeometryPasses(resources);
        this.fullscreen = new FullscreenPasses(resources);
        this.computes = new ComputeDispatcher(resources);
        ShadowPlan plan = ShadowPlan.of(resources.dim.targets().shadow());
        plan.notes().forEach(n -> resources.diagnostics.report(resources.dim.folder() + ": " + n));
        this.shadows = new ShadowRenderer(plan, ShadowSections.CAMERA_VISIBLE);
    }

    /** @return the pack's resources */
    PackResources resources() {
        return r;
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
        RenderTarget main = Minecraft.getInstance().gameRenderer.mainRenderTarget();
        if (r.targets.resize(main.width, main.height)) {
            r.sinks.close();
        }
        gameState.capture(r.frameState, camera, projection, partialTick);
        r.frameState.update();
        CommandEncoder encoder = RenderSystem.getDevice().createCommandEncoder();
        r.frameUniforms.fill(r.frameState, r.frameState.timer().frameTime());
        frameSlice = r.frameUniforms.upload(encoder);
        r.drawUniforms.beginFrame();
        r.drawSlots.prepare(r.frameState, r.drawUniforms::push);
        r.drawUniforms.flush(encoder);
        r.targets.clear(encoder, new Rgba(fogColor.x, fogColor.y, fogColor.z, 1), r.depthMode);
        RenderSystem.setShaderFog(terrainFog);
        this.level = level;
        r.sequencer.begin(firstFrame);
        firstFrame = false;
        r.sequencer.runUntil(PassGroup.GBUFFERS_OPAQUE, this);
        return true;
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

    /** The opaque geometry is drawn: copies the depth, then runs everything up to the translucent geometry. */
    void afterOpaque() {
        CommandEncoder encoder = RenderSystem.getDevice().createCommandEncoder();
        RenderTarget main = Minecraft.getInstance().gameRenderer.mainRenderTarget();
        r.targets.copyMainDepth(encoder, main.getDepthTexture(), 2);
        r.targets.copyMainDepth(encoder, main.getDepthTexture(), 1);
        r.sequencer.runUntil(PassGroup.GBUFFERS_TRANSLUCENT, this);
    }

    /** The translucent geometry is drawn: runs the composite passes and {@code final}, and ends the frame. */
    void finishFrame() {
        r.sequencer.finish(this);
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

    /** Requests every composite-style pipeline; true once none is still compiling. */
    private boolean compositesReady() {
        boolean ready = true;
        for (FramePlan.Step step : r.sequencer.plan().steps()) {
            if (step instanceof FramePlan.Step.Fullscreen f
                && r.programs.program(f.program(), fullscreenLayout(f.program(), f.group())) instanceof ProgramResolution.Pending) {
                ready = false;
            }
        }
        return ready;
    }

    private AttachmentLayout fullscreenLayout(int index, PassGroup group) {
        Program program = r.dim.programs().get(index);
        return group == PassGroup.FINAL
            ? AttachmentLayout.single("final", 0, Minecraft.getInstance().gameRenderer.mainRenderTarget().getColorTexture().getFormat())
            : AttachmentLayout.fullscreen(r.dim, program);
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
        shadows.render(level, r.frameState.shadowModelView(), new ShadowRenderer.ShadowTargets() {
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
        });
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
