package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.commands.RenderPassDescriptor;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.render.draw.ActivePasses;
import dev.shaderbridge.render.draw.CompiledPipelineIndex;
import dev.shaderbridge.render.draw.DrawSubstitution;
import dev.shaderbridge.render.pipeline.AttachmentLayout;
import dev.shaderbridge.render.targets.ColorPair;
import java.util.List;
import java.util.Optional;
import net.minecraft.client.Minecraft;

/**
 * Opens the render passes Minecraft's world geometry is drawn into while a pack is active: the
 * shared gbuffers pass (the pack's {@code gbuffer_attachments} in their current textures, plus
 * Minecraft's main depth) and the shadow pass ({@code shadow_attachments} plus {@code shadowtex0}),
 * and registers them with {@link ActivePasses} so that every vanilla pipeline bound in them is
 * substituted: a pack pipeline with its descriptors, or the vanilla pipeline adapted to the pass.
 * When a pack's geometry writes more targets than one pass can hold, the gbuffers pass holds only
 * {@code fallback_tex} and all geometry draws vanilla. The same attachments also host
 * ShaderBridge's own geometry (Distant Horizons LODs) in passes that are not registered for
 * substitution ({@link #openOwnGbuffers}, {@link #openOwnShadow}). Render thread only.
 */
final class GeometryPasses {
    private final PackResources r;
    private final AttachmentLayout gbuffers;
    private final boolean gbuffersShared;
    private final AttachmentLayout shadow;

    GeometryPasses(PackResources r) {
        this.r = r;
        Optional<AttachmentLayout> shared = AttachmentLayout.shared(r.dim, false);
        int fallback = r.dim.settings().fallbackTex();
        this.gbuffersShared = shared.isPresent();
        this.gbuffers = shared.orElseGet(() -> AttachmentLayout.single("gbuffers_fallback", fallback,
            r.targets.color(fallback).or(() -> r.targets.color(0)).orElseThrow().spec().format()));
        if (!gbuffersShared) {
            r.diagnostics.report(r.dim.folder() + ": the pack's geometry writes more targets than one render pass holds; "
                + "world geometry is drawn unshaded into colortex" + fallback);
        }
        this.shadow = AttachmentLayout.shared(r.dim, true).orElse(new AttachmentLayout("shadow", List.of(), true));
    }

    /**
     * Opens the gbuffers pass.
     *
     * @param label debug label
     * @param flips the frame's flip state
     * @param frame the frame's {@code sb_Frame} slice
     * @return the open pass; close it, then call {@link #closed}
     */
    RenderPass openGbuffers(String label, FlipState flips, GpuBufferSlice frame) {
        RenderTarget main = Minecraft.getInstance().gameRenderer.mainRenderTarget();
        return open(label, gbuffers, false, flips, frame, main.width, main.height, main.getDepthTextureView());
    }

    /**
     * Opens a pass on the shared gbuffers attachments for ShaderBridge's own draws (Distant
     * Horizons LODs): not registered for pipeline substitution, nothing bound.
     *
     * @param label debug label
     * @param flips the frame's flip state
     * @param depth the depth attachment (screen-sized)
     * @return the open pass, or empty when the pack's geometry has no shared attachments
     */
    Optional<RenderPass> openOwnGbuffers(String label, FlipState flips, GpuTextureView depth) {
        if (!gbuffersShared) {
            return Optional.empty();
        }
        return Optional.of(create(label, gbuffers, false, flips, depth.getWidth(0), depth.getHeight(0), depth));
    }

    /**
     * Opens a pass on the shadow attachments and {@code shadowtex0} for ShaderBridge's own draws
     * (Distant Horizons LODs): not registered for pipeline substitution, nothing bound.
     *
     * @param label debug label
     * @param flips the frame's flip state
     * @return the open pass
     */
    RenderPass openOwnShadow(String label, FlipState flips) {
        GpuTextureView depth = r.targets.shadowDepthView(0);
        return create(label, shadow, true, flips, depth.getWidth(0), depth.getHeight(0), depth);
    }

    /**
     * Opens the shadow pass.
     *
     * @param label debug label
     * @param flips the frame's flip state
     * @param frame the frame's {@code sb_Frame} slice
     * @return the open pass; close it, then call {@link #closed}
     */
    RenderPass openShadow(String label, FlipState flips, GpuBufferSlice frame) {
        GpuTextureView depth = r.targets.shadowDepthView(0);
        return open(label, shadow, true, flips, frame, depth.getWidth(0), depth.getHeight(0), depth);
    }

    /**
     * Forgets a pass opened here once it is closed.
     *
     * @param pass the pass
     */
    void closed(RenderPass pass) {
        ActivePasses.close(pass);
    }

    private RenderPass open(String label, AttachmentLayout layout, boolean shadowPass, FlipState flips, GpuBufferSlice frame, int width, int height,
                            GpuTextureView depth) {
        RenderPass pass = create(label, layout, shadowPass, flips, width, height, depth);
        RenderSystem.bindDefaultUniforms(pass);
        ActivePasses.open(pass, new Draws(layout, shadowPass, shadowPass || gbuffersShared, flips, frame));
        return pass;
    }

    private RenderPass create(String label, AttachmentLayout layout, boolean shadowPass, FlipState flips, int width, int height, GpuTextureView depth) {
        List<Integer> targets = layout.attachments().stream().map(AttachmentLayout.Attachment::target).toList();
        List<AttachmentSlot> slots = PassAttachments.geometry(targets, shadowPass, flips, t -> sized(pair(t, shadowPass), width, height));
        RenderPassDescriptor.Builder descriptor = RenderPassDescriptor.builder(() -> label);
        for (int s = 0; s < slots.size(); s++) {
            switch (slots.get(s)) {
                case AttachmentSlot.Target t -> descriptor.withColorAttachment(pair(t.target(), shadowPass).orElseThrow().attachmentView(t.alt()));
                case AttachmentSlot.Sink k -> descriptor.withColorAttachment(r.sinks.view(layout.attachments().get(s).format(), width, height));
                default -> descriptor.withUnusedColorAttachment();
            }
        }
        descriptor.withDepthAttachment(depth);
        return RenderSystem.getDevice().createCommandEncoder().createRenderPass(descriptor.build());
    }

    private Optional<ColorPair> pair(int target, boolean shadowPass) {
        return shadowPass ? r.targets.shadowColor(target) : r.targets.color(target);
    }

    private static boolean sized(Optional<ColorPair> pair, int width, int height) {
        return pair.isPresent() && pair.get().spec().width() == width && pair.get().spec().height() == height;
    }

    /** Chooses the pipeline of every vanilla draw in one pass. */
    private final class Draws implements ActivePasses.PassDraws {
        private final AttachmentLayout layout;
        private final boolean shadowPass;
        private final boolean packPrograms;
        private final FlipState flips;
        private final GpuBufferSlice frame;
        private final MinecraftHost host = r.host();

        Draws(AttachmentLayout layout, boolean shadowPass, boolean packPrograms, FlipState flips, GpuBufferSlice frame) {
            this.layout = layout;
            this.shadowPass = shadowPass;
            this.packPrograms = packPrograms;
            this.flips = flips;
            this.frame = frame;
        }

        @Override
        public ActivePasses.Substitution substitute(CompiledRenderPipeline requested) {
            RenderPipeline vanilla = CompiledPipelineIndex.lookup(requested);
            if (vanilla == null) {
                r.diagnostics.report("A pipeline that did not come from Minecraft's pipeline cache was drawn during world rendering; it cannot be adapted");
                return ActivePasses.Substitution.unchanged(requested);
            }
            if (packPrograms && r.substitution.decide(vanilla, shadowPass) instanceof DrawSubstitution.Decision.Pack p) {
                DrawKey key = DrawKey.of(p.resolution().pipeline().key().toString(), p.program(), RenderStages.of(p.routed()), shadowPass);
                GpuBufferSlice draw = r.drawSlots.slice(key);
                return new ActivePasses.Substitution(p.resolution().compiled(), target -> r.binder.bind(target, p.resolution().pipeline().bindings(),
                    p.program(), flips, frame, draw, target.boundTexture("Sampler0").map(host::withAlbedo).orElse(host)));
            }
            Optional<CompiledRenderPipeline> clone = shadowPass ? r.clones.discard(vanilla, layout)
                : r.clones.fallback(vanilla, layout, r.dim.settings().fallbackTex());
            if (clone.isEmpty()) {
                r.diagnostics.report("Pipeline " + vanilla.getLocation() + " cannot be adapted to the shader pack's render pass");
            }
            return ActivePasses.Substitution.unchanged(clone.orElse(requested));
        }
    }
}
