package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.pipeline.DepthStencilState;
import com.mojang.renderpearl.api.pipeline.IndexType;
import com.mojang.renderpearl.api.pipeline.PrimitiveTopology;
import com.mojang.renderpearl.api.textures.FilterMode;
import dev.shaderbridge.dh.DhHostBlocks;
import dev.shaderbridge.dh.DhMode;
import dev.shaderbridge.dh.DistantHorizons;
import dev.shaderbridge.dh.LodBuffer;
import dev.shaderbridge.dh.LodFrame;
import dev.shaderbridge.dh.LodSelection;
import dev.shaderbridge.dh.LodUniforms;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.GeometrySlot;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.render.pipeline.AttachmentLayout;
import dev.shaderbridge.render.pipeline.PipelineShape;
import dev.shaderbridge.render.pipeline.ProfileVertexFormats;
import dev.shaderbridge.render.pipeline.ProgramResolution;
import dev.shaderbridge.render.pipeline.ProgramResolver;
import dev.shaderbridge.render.targets.TextureBinding;
import java.util.List;
import java.util.Optional;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.ClientLevel;
import org.joml.Matrix4f;
import org.joml.Matrix4fc;
import org.joml.Vector3d;

/**
 * Draws Distant Horizons' LODs ({@link DistantHorizons#lods}) with the pack's {@code dh_terrain},
 * {@code dh_water} and {@code dh_shadow} programs, each slot in a pass of its own on the pack's
 * shared gbuffers attachments (or the shadow attachments), per the frame's {@link DhMode}
 * ({@link DistantFrame}): into the LOD depth ({@link DhMode#NATIVE}, copied to
 * {@code dhDepthTex1} after the opaque LODs and to {@code dhDepthTex0} after {@code dh_water}) or
 * into Minecraft's depth with the unified projection and only beyond the vanilla area
 * ({@link DhMode#SYNTHESIZED}). ShaderBridge binds the profile's host resources itself:
 * {@code uLightMap} (Minecraft's lightmap), {@code uBlockAtlas} (Distant Horizons' block atlas),
 * {@code vertSharedUniformBlock} and one {@code vertUniqueUniformBlock} per buffer
 * ({@link DhHostBlocks}). The pack's {@code gtexture} samples white, as for the headless
 * executor's LODs.
 *
 * <p>Opaque LODs are drawn before Minecraft's opaque geometry and translucent LODs before its
 * translucent geometry, as Distant Horizons draws them for Iris. The shadow pass draws the LODs of
 * the previous frame (the current list is only known once Minecraft's level rendering has begun).
 * Programs that are still compiling or cannot run leave the LODs out. Render thread only, outside
 * any render pass.
 */
final class DistantPasses {
    private final PackResources r;
    private final GeometryPasses passes;

    /**
     * @param r      the pack's resources
     * @param passes opens the gbuffers and shadow passes
     */
    DistantPasses(PackResources r, GeometryPasses passes) {
        this.r = r;
        this.passes = passes;
    }

    /**
     * Draws the opaque LODs ({@code dh_terrain}) of this frame, then copies {@code dhDepthTex1}.
     *
     * @param flips the frame's flip state
     * @param frame the {@code sb_Frame} slice
     */
    void drawOpaque(FlipState flips, GpuBufferSlice frame) {
        current().ifPresent(lods -> draw(GeometryProgram.DH_TERRAIN, lods.opaque(), cameraClip(), false, flips, frame));
        r.distant.copyDepth(1);
    }

    /**
     * Draws the translucent LODs ({@code dh_water}) of this frame, then copies {@code dhDepthTex0}.
     *
     * @param flips the frame's flip state
     * @param frame the {@code sb_Frame} slice
     */
    void drawWater(FlipState flips, GpuBufferSlice frame) {
        current().ifPresent(lods -> draw(GeometryProgram.DH_WATER, lods.water(), cameraClip(), false, flips, frame));
        r.distant.copyDepth(0);
    }

    /**
     * Draws the opaque LODs of the latest frame into the shadow map ({@code dh_shadow}).
     *
     * @param flips the frame's flip state
     * @param frame the {@code sb_Frame} slice
     */
    void drawShadow(FlipState flips, GpuBufferSlice frame) {
        if (r.distant.mode().drawsLods()) {
            Matrix4f clip = new Matrix4f(r.frameState.shadowProjection()).mul(r.frameState.shadowModelView());
            draw(GeometryProgram.DH_SHADOW, r.distant.dh().lods().opaque(), clip, true, flips, frame);
        }
    }

    /** The LODs Distant Horizons handed over this frame, if LODs are drawn. */
    private Optional<LodFrame> current() {
        DistantHorizons dh = r.distant.dh();
        LodFrame lods = dh.lods();
        return r.distant.mode().drawsLods() && lods.frame() == dh.frame() ? Optional.of(lods) : Optional.empty();
    }

    private Matrix4f cameraClip() {
        return new Matrix4f(r.frameState.dhProjection()).mul(r.frameState.gbufferModelView());
    }

    /**
     * A LOD slot's program that runs, after walking its fallback chain.
     *
     * @param program  the program (of the slot or of the fallback that runs instead)
     * @param pipeline its compiled pipeline
     */
    private record Resolved(Program program, ProgramResolution.Renderpearl pipeline) {
    }

    private void draw(GeometryProgram slot, List<LodBuffer> all, Matrix4fc clip, boolean shadow, FlipState flips, GpuBufferSlice frame) {
        Optional<Resolved> resolved = resolve(slot, shadow);
        if (resolved.isEmpty()) {
            return;
        }
        Program program = resolved.get().program();
        ProgramResolution.Renderpearl pipeline = resolved.get().pipeline();
        List<LodBuffer> buffers = selection(clip).select(all).stream().filter(LodBuffer::drawable).toList();
        DistantBindings bindings = DistantBindings.of(pipeline.pipeline().bindings());
        if (buffers.isEmpty()) {
            return;
        }
        if (shadow && AttachmentLayout.shared(r.dim, true).isEmpty() && !program.drawBuffers().isEmpty()) {
            r.diagnostics.report(slot.fileName() + ": the pack's shadow programs write more targets than one render pass holds; LODs cast no shadows");
            return;
        }
        if (!bindings.supported()) {
            r.diagnostics.report(slot.fileName() + " uses Distant Horizons host resources ShaderBridge does not provide (" + bindings.unknown()
                + "); LODs are not drawn");
            return;
        }
        LodUniforms.PassSlots slots = r.distant.uniforms().write(RenderSystem.getDevice().createCommandEncoder(), shared(clip), buffers,
            r.frameState.cameraPosition);
        String label = "ShaderBridge " + slot.fileName();
        Optional<RenderPass> pass = shadow ? Optional.of(passes.openOwnShadow(label, flips))
            : passes.openOwnGbuffers(label, flips, r.distant.gbuffersDepth());
        if (pass.isEmpty()) {
            r.diagnostics.report(slot.fileName() + ": the pack's geometry writes more targets than one render pass holds; LODs are not drawn");
            return;
        }
        try (RenderPass p = pass.get()) {
            bind(p, slot, program, pipeline, bindings, slots, shadow, flips, frame);
            for (int i = 0; i < buffers.size(); i++) {
                LodBuffer b = buffers.get(i);
                if (bindings.unique()) {
                    p.setUniform(DhHostBlocks.UNIQUE_BLOCK, slots.unique().get(i));
                }
                p.setVertexBuffer(0, b.vertices().slice());
                p.setIndexBuffer(b.indices(), IndexType.INT);
                p.drawIndexed(b.indexCount(), 1, 0, 0, 0);
            }
        }
    }

    /** Binds the pipeline, the host resources and the pack's descriptors. */
    private void bind(RenderPass pass, GeometryProgram slot, Program program, ProgramResolution.Renderpearl pipeline, DistantBindings bindings,
                      LodUniforms.PassSlots slots, boolean shadow, FlipState flips, GpuBufferSlice frame) {
        pass.setPipeline(pipeline.compiled());
        OwnPassUniforms target = new OwnPassUniforms(pass);
        MinecraftHost host = r.host()
            .withAlbedo(new TextureBinding(r.textures.white(), RenderSystem.getSamplerCache().getRepeat(FilterMode.NEAREST)));
        if (bindings.lightmap()) {
            target.bind(DistantBindings.LIGHTMAP, host.lightmap());
        }
        if (bindings.atlas()) {
            target.bind(DistantBindings.BLOCK_ATLAS, r.distant.blockAtlas().orElse(host.atlas()));
        }
        if (bindings.shared()) {
            target.bind(DhHostBlocks.SHARED_BLOCK, slots.shared());
        }
        DrawKey key = DrawKey.of(pipeline.pipeline().key().toString(), program, RenderStages.of(slot), shadow);
        r.binder.bind(target, pipeline.pipeline().bindings(), program, flips, frame, r.drawSlots.slice(key), host);
    }

    /** The program that draws a slot, compiled for the draw profile the slot's program was translated for. */
    private Optional<Resolved> resolve(GeometryProgram slot, boolean shadow) {
        Program own = programOf(slot);
        if (own == null) {
            return Optional.empty();
        }
        String profile = own.drawProfile();
        Optional<PipelineShape> shape = profile == null ? Optional.empty()
            : PipelineShape.ofProfile(profile, ProfileVertexFormats.get(), PrimitiveTopology.TRIANGLES, DepthStencilState.DEFAULT, true);
        if (shape.isEmpty()) {
            r.diagnostics.report(slot.fileName() + ": draw profile " + profile + " has no vertex layout; LODs are not drawn");
            return Optional.empty();
        }
        // The shadow pass draws without back-face culling, as for vanilla geometry.
        PipelineShape drawn = shadow ? shape.get().withCull(false) : shape.get();
        ProgramResolver.GeometryResolution resolution = r.programs.geometry(slot, profile, drawn, shadow);
        Program program = programOf(resolution.program());
        return resolution.resolution() instanceof ProgramResolution.Renderpearl rp && program != null ? Optional.of(new Resolved(program, rp))
            : Optional.empty();
    }

    private Program programOf(GeometryProgram slot) {
        GeometrySlot g = r.dim.geometry().get(slot);
        return g == null ? null : r.dim.programs().get(g.program());
    }

    /** Culls against the pass's view; synthesized LODs also skip the vanilla area. */
    private LodSelection selection(Matrix4fc clip) {
        ClientLevel level = Minecraft.getInstance().level;
        int minY = level == null ? 0 : level.getMinY();
        int maxY = level == null ? 0 : level.getMinY() + level.getHeight();
        // Earth curvature bends LODs below their boxes: no culling then.
        boolean curved = r.distant.dh().settings().map(s -> DhHostBlocks.earthRadius(s.earthCurvature()) != 0).orElse(false);
        double vanilla = r.distant.mode().unifiedProjection()
            ? LodSelection.vanillaRadius(Minecraft.getInstance().options.getEffectiveRenderDistance()) : 0;
        Vector3d camera = r.frameState.cameraPosition;
        return new LodSelection(curved || level == null ? null : clip, camera.x, camera.y, camera.z, minY, maxY, vanilla);
    }

    private DhHostBlocks.Shared shared(Matrix4fc combined) {
        ClientLevel level = Minecraft.getInstance().level;
        RenderTarget main = Minecraft.getInstance().gameRenderer.mainRenderTarget();
        float earth = r.distant.dh().settings().map(s -> DhHostBlocks.earthRadius(s.earthCurvature())).orElse(0.0f);
        return new DhHostBlocks.Shared(level == null ? 0 : level.getMinY(), earth, r.frameState.timer().frameCounter() % 8, main.width, main.height,
            combined);
    }
}
