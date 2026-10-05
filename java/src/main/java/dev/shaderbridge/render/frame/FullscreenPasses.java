package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.commands.RenderPassDescriptor;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.api.textures.FilterMode;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.render.draw.PassViewport;
import dev.shaderbridge.render.pipeline.AttachmentLayout;
import dev.shaderbridge.render.pipeline.AttachmentPlan;
import dev.shaderbridge.render.pipeline.ProgramResolution;
import dev.shaderbridge.render.pipeline.RawDraw;
import dev.shaderbridge.render.pipeline.ViewportRect;
import dev.shaderbridge.render.targets.ColorPair;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.RenderPipelines;

/**
 * Draws composite-style programs: each in its own render pass over its output targets (the
 * {@link FlipState#write} textures, or Minecraft's main color target for {@code final}) as the
 * {@code fullscreen} profile's six vertices, with its inputs bound per {@code BindingUse.use_alt},
 * through renderpearl or, for programs only raw Vulkan can run, the raw path, in the viewport
 * its {@code scale.<program>} asks for ({@link ViewportRect}; renderpearl passes get it through
 * {@link PassViewport} on the Vulkan backend). The mipmaps a program asks for are generated just
 * before it ({@link MipGenerator}).
 * Also performs the copies around them: {@code colortex0} to the main target when no
 * {@code final} program drew, and the end-of-frame alt to main copies. Render thread only.
 */
final class FullscreenPasses {
    /** Vertices of the {@code fullscreen} profile's quad (two triangles from the vertex index). */
    static final int VERTICES = 6;

    private final PackResources r;

    FullscreenPasses(PackResources r) {
        this.r = r;
    }

    /**
     * @param index the program
     * @param group its pass group
     * @param flips the frame's flip state
     * @param frame the frame's {@code sb_Frame} slice
     * @return whether it drew, and the shadowcolor targets a {@code shadowcomp} program wrote
     */
    FrameSteps.Drawn draw(int index, PassGroup group, FlipState flips, GpuBufferSlice frame) {
        Program program = r.dim.programs().get(index);
        boolean shadow = group == PassGroup.SHADOW_COMP;
        RenderTarget main = Minecraft.getInstance().gameRenderer.mainRenderTarget();
        int[] size = group == PassGroup.FINAL ? new int[] {main.width, main.height} : passSize(program, shadow);
        int maxAttachments = r.device.getDeviceInfo().limits().maxColorAttachments();
        List<AttachmentSlot> slots = PassAttachments.fullscreen(program, group, flips, t -> sized(pair(t, shadow), size), maxAttachments);
        if (slots.isEmpty()) {
            return FrameSteps.Drawn.NOTHING;
        }
        AttachmentLayout layout = layout(index, group);
        ProgramResolution resolution = r.programs.program(index, layout);
        if (!(resolution instanceof ProgramResolution.Renderpearl) && !(resolution instanceof ProgramResolution.Raw)) {
            return FrameSteps.Drawn.NOTHING;
        }
        ViewportRect viewport = ViewportRect.of(program.viewport(), size[0], size[1]);
        for (int t : program.mipmapTargets()) {
            pair(t, shadow).ifPresent(pair -> r.mips.generate(pair, shadow ? flips.shadowRead(t) : flips.read(t)));
        }
        List<GpuTextureView> views = new ArrayList<>();
        for (int s = 0; s < slots.size(); s++) {
            views.add(switch (slots.get(s)) {
                case AttachmentSlot.Target t -> pair(t.target(), shadow).orElseThrow().attachmentView(t.alt());
                case AttachmentSlot.MainColor m -> main.getColorTextureView();
                case AttachmentSlot.Sink k -> r.sinks.view(s, layout.attachments().get(s).format(), size[0], size[1]);
            });
        }
        if (resolution instanceof ProgramResolution.Raw raw) {
            return drawRaw(raw, program, slots, views, size, viewport, shadow, flips, frame);
        }
        ProgramResolution.Renderpearl pipeline = (ProgramResolution.Renderpearl) resolution;
        RenderPassDescriptor.Builder descriptor = RenderPassDescriptor.builder(() -> "ShaderBridge " + program.name());
        views.forEach(descriptor::withColorAttachment);
        DrawKey key = DrawKey.of(pipeline.pipeline().key().toString(), program, RenderStages.NONE, false, AlbedoSize.NONE);
        try (RenderPass pass = RenderSystem.getDevice().createCommandEncoder().createRenderPass(descriptor.build())) {
            if (!viewport.covers(size[0], size[1]) && !PassViewport.set(pass, viewport)) {
                r.diagnostics.report(program.name() + ": the viewport scale/offset (scale." + program.name()
                    + ") cannot be set on this backend; drawn over the whole target");
            }
            pass.setPipeline(pipeline.compiled());
            r.binder.bind(new OwnPassUniforms(pass), pipeline.pipeline().bindings(), program, flips, frame, r.drawSlots.slice(key),
                r.host());
            pass.draw(VERTICES, 1, 0, 0);
        }
        return new FrameSteps.Drawn(true, shadow ? written(slots, pipeline.pipeline().attachments()) : List.of());
    }

    /**
     * Draws a program on the raw Vulkan path, in a render pass of its own over the same
     * attachments.
     *
     * @return whether it drew, and the shadowcolor targets a {@code shadowcomp} program wrote
     */
    private FrameSteps.Drawn drawRaw(ProgramResolution.Raw raw, Program program, List<AttachmentSlot> slots, List<GpuTextureView> views,
                                     int[] size, ViewportRect viewport, boolean shadow, FlipState flips, GpuBufferSlice frame) {
        GpuBufferSlice draw = r.drawSlots.slice(DrawKey.of("raw " + program.name(), program, RenderStages.NONE, false, AlbedoSize.NONE));
        List<Optional<GpuTextureView>> attachments = views.stream().map(Optional::of).toList();
        List<Boolean> wrote;
        try {
            wrote = r.raw.draw(raw.program(), new RawDraw(attachments, size[0], size[1], r.frameState.timer().frameCounter(), frame, draw,
                flips.colorState(), flips.shadowState(), viewport));
        } catch (RuntimeException e) {
            r.diagnostics.report(program.name() + " was not drawn: " + e.getMessage());
            return FrameSteps.Drawn.NOTHING;
        }
        List<Integer> written = new ArrayList<>();
        for (int s = 0; s < slots.size() && s < wrote.size(); s++) {
            if (slots.get(s) instanceof AttachmentSlot.Target t && wrote.get(s)) {
                written.add(t.target());
            }
        }
        return new FrameSteps.Drawn(true, shadow ? written : List.of());
    }

    /**
     * @param index a composite-style program
     * @param group its pass group
     * @return the attachments its pipeline is built for: its draw buffers, or Minecraft's main
     *     color target for {@code final}
     */
    AttachmentLayout layout(int index, PassGroup group) {
        return group == PassGroup.FINAL
            ? AttachmentLayout.single("final", 0, Minecraft.getInstance().gameRenderer.mainRenderTarget().getColorTexture().getFormat())
            : AttachmentLayout.fullscreen(r.dim, r.dim.programs().get(index));
    }

    /**
     * Without a {@code final} program: draws the current {@code colortex0} into Minecraft's main
     * color target with Minecraft's screen blit.
     *
     * @param flips the frame's flip state
     */
    void copyToOutput(FlipState flips) {
        Optional<ColorPair> color0 = r.targets.color(0);
        CompiledRenderPipeline blit = RenderSystem.getCompiledPipelineNullable(RenderPipelines.TRACY_BLIT);
        RenderTarget main = Minecraft.getInstance().gameRenderer.mainRenderTarget();
        if (color0.isEmpty() || blit == null || main.getColorTexture().getFormat() != RenderPipelines.TRACY_BLIT.getColorTargetStates().getFirst().format()) {
            r.diagnostics.report("colortex0 cannot be shown: the pack has no final program and the screen blit is unavailable");
            return;
        }
        try (RenderPass pass = RenderSystem.getDevice().createCommandEncoder()
            .createRenderPass(() -> "ShaderBridge colortex0 to screen", main.getColorTextureView(), Optional.empty())) {
            RenderSystem.bindDefaultUniforms(pass);
            pass.setPipeline(blit);
            pass.setUniform("InSampler", color0.get().sampleView(flips.read(0)), RenderSystem.getSamplerCache().getClampToEdge(FilterMode.NEAREST));
            pass.draw(3, 1, 0, 0);
        }
    }

    /**
     * Copies the alternate texture of buffers left in it over the main one.
     *
     * @param color  colortex indices
     * @param shadow shadowcolor indices
     */
    void endOfFrame(List<Integer> color, List<Integer> shadow) {
        CommandEncoder encoder = RenderSystem.getDevice().createCommandEncoder();
        color.forEach(i -> r.targets.copyAltToMain(encoder, i));
        for (int i : shadow) {
            r.targets.shadowColor(i).ifPresent(pair -> {
                GpuTexture alt = pair.texture(true);
                encoder.copyTextureToTexture(alt, pair.texture(false), 0, 0, 0, 0, 0, alt.getWidth(0), alt.getHeight(0));
            });
        }
    }

    /**
     * The size of a composite-style pass: that of the program's first existing output target
     * (targets scaled with {@code size.buffer} are drawn at their own size, as in Iris), else the
     * screen or the shadow map. Outputs of another size get no texture.
     */
    private int[] passSize(Program program, boolean shadow) {
        for (int t : program.drawBuffers()) {
            Optional<ColorPair> pair = pair(t, shadow);
            if (pair.isPresent()) {
                return new int[] {pair.get().spec().width(), pair.get().spec().height()};
            }
        }
        if (shadow) {
            GpuTextureView depth = r.targets.shadowDepthView(0);
            return new int[] {depth.getWidth(0), depth.getHeight(0)};
        }
        RenderTarget main = Minecraft.getInstance().gameRenderer.mainRenderTarget();
        return new int[] {main.width, main.height};
    }

    private Optional<ColorPair> pair(int target, boolean shadow) {
        return shadow ? r.targets.shadowColor(target) : r.targets.color(target);
    }

    private static boolean sized(Optional<ColorPair> pair, int[] size) {
        return pair.isPresent() && pair.get().spec().width() == size[0] && pair.get().spec().height() == size[1];
    }

    /** Targets the program really writes: an attachment with a texture and a written slot. */
    private static List<Integer> written(List<AttachmentSlot> slots, AttachmentPlan plan) {
        List<Integer> out = new ArrayList<>();
        for (int s = 0; s < slots.size() && s < plan.slots().size(); s++) {
            if (slots.get(s) instanceof AttachmentSlot.Target t && plan.slots().get(s).write()) {
                out.add(t.target());
            }
        }
        return out;
    }
}
