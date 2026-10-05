package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.commands.RenderPassDescriptor;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import com.mojang.renderpearl.frontend.FrontendRenderPipeline;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.render.draw.ActivePasses;
import dev.shaderbridge.render.draw.CompiledPipelineIndex;
import dev.shaderbridge.render.draw.DrawSubstitution;
import dev.shaderbridge.render.draw.UniformTarget;
import dev.shaderbridge.render.draw.VanillaClones;
import dev.shaderbridge.render.pipeline.AttachmentLayout;
import dev.shaderbridge.render.targets.ColorPair;
import dev.shaderbridge.render.targets.TextureBinding;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
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
 * substitution ({@link #openOwnGbuffers}, {@link #openOwnShadow}). Before each pass, the attached
 * targets its programs sample are copied ({@link FeedbackReads}, {@link PassCopies}). Render
 * thread only.
 */
final class GeometryPasses {
    /** The uniform block of Minecraft's feature draws that holds their model-view. */
    static final String DYNAMIC_TRANSFORMS = "DynamicTransforms";

    private final PackResources r;
    private final AttachmentLayout gbuffers;
    private final boolean gbuffersShared;
    private final AttachmentLayout shadow;
    private final Set<ResourceRef> gbufferReads;
    private final Set<ResourceRef> shadowReads;

    /**
     * A pass for ShaderBridge's own draws.
     *
     * @param pass the open pass
     * @param host the game's textures for its draws (with the copies of its attached targets)
     */
    record OwnPass(RenderPass pass, MinecraftHost host) {
    }

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
        this.gbufferReads = FeedbackReads.of(r.dim, false, targets(gbuffers));
        this.shadowReads = FeedbackReads.of(r.dim, true, targets(shadow));
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
        return open(label, gbuffers, false, flips, frame, main.width, main.height, main.getDepthTextureView(), null);
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
    Optional<OwnPass> openOwnGbuffers(String label, FlipState flips, GpuTextureView depth) {
        if (!gbuffersShared) {
            return Optional.empty();
        }
        MinecraftHost host = hostFor(false, flips);
        return Optional.of(new OwnPass(create(label, gbuffers, false, flips, depth.getWidth(0), depth.getHeight(0), depth), host));
    }

    /**
     * Opens a pass on the shadow attachments and {@code shadowtex0} for ShaderBridge's own draws
     * (Distant Horizons LODs): not registered for pipeline substitution, nothing bound.
     *
     * @param label debug label
     * @param flips the frame's flip state
     * @return the open pass
     */
    OwnPass openOwnShadow(String label, FlipState flips) {
        GpuTextureView depth = r.targets.shadowDepthView(0);
        MinecraftHost host = hostFor(true, flips);
        return new OwnPass(create(label, shadow, true, flips, depth.getWidth(0), depth.getHeight(0), depth), host);
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
        return open(label, shadow, true, flips, frame, depth.getWidth(0), depth.getHeight(0), depth, null);
    }

    /**
     * Opens a shadow pass for the frame's prepared features (entities, block entities): like
     * {@link #openShadow}, but the features' {@code DynamicTransforms} blocks are replaced by their
     * shadow-camera copies as Minecraft binds them, and draws that cast no shadows in Iris
     * (particles, weather) draw nothing.
     *
     * @param label      debug label
     * @param flips      the frame's flip state
     * @param frame      the frame's {@code sb_Frame} slice
     * @param transforms each feature transform block to its shadow-camera copy
     * @return the open pass; close it, then call {@link #closed}
     */
    RenderPass openShadowFeatures(String label, FlipState flips, GpuBufferSlice frame, Map<GpuBufferSlice, GpuBufferSlice> transforms) {
        GpuTextureView depth = r.targets.shadowDepthView(0);
        return open(label, shadow, true, flips, frame, depth.getWidth(0), depth.getHeight(0), depth, transforms);
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
                            GpuTextureView depth, Map<GpuBufferSlice, GpuBufferSlice> transforms) {
        MinecraftHost host = hostFor(shadowPass, flips);
        RenderPass pass = create(label, layout, shadowPass, flips, width, height, depth);
        RenderSystem.bindDefaultUniforms(pass);
        ActivePasses.open(pass, new Draws(layout, shadowPass, shadowPass || gbuffersShared, flips, frame, host, transforms));
        return pass;
    }

    private RenderPass create(String label, AttachmentLayout layout, boolean shadowPass, FlipState flips, int width, int height, GpuTextureView depth) {
        List<Integer> targets = targets(layout);
        List<AttachmentSlot> slots = PassAttachments.geometry(targets, shadowPass, flips, t -> sized(pair(t, shadowPass), width, height));
        RenderPassDescriptor.Builder descriptor = RenderPassDescriptor.builder(() -> label);
        for (int s = 0; s < slots.size(); s++) {
            descriptor.withColorAttachment(switch (slots.get(s)) {
                case AttachmentSlot.Target t -> pair(t.target(), shadowPass).orElseThrow().attachmentView(t.alt());
                case AttachmentSlot.Sink k -> r.sinks.view(s, layout.attachments().get(s).format(), width, height);
                case AttachmentSlot.MainColor m -> throw new IllegalStateException("geometry passes do not draw into the main color target");
            });
        }
        descriptor.withDepthAttachment(depth);
        return RenderSystem.getDevice().createCommandEncoder().createRenderPass(descriptor.build());
    }

    /**
     * Copies the attached targets the pass's programs sample. Call outside any render pass.
     *
     * @return the game's textures for the pass's draws, handing out the copies
     */
    private MinecraftHost hostFor(boolean shadowPass, FlipState flips) {
        Set<ResourceRef> reads = shadowPass ? shadowReads : gbufferReads;
        if (reads.isEmpty()) {
            return r.host();
        }
        return r.host().withPassCopies(r.passCopies.take(RenderSystem.getDevice().createCommandEncoder(), reads, ref -> source(ref, flips)));
    }

    /** The texture a geometry pass attaches for a resource ({@link FeedbackReads#key} forms). */
    private Optional<GpuTexture> source(ResourceRef resource, FlipState flips) {
        return switch (resource) {
            case ResourceRef.ColorTex c -> r.targets.color(c.index()).map(p -> p.texture(flips.read(c.index())));
            case ResourceRef.ShadowColor c -> r.targets.shadowColor(c.index()).map(p -> p.texture(flips.shadowRead(c.index())));
            case ResourceRef.DepthTex d -> Optional.of(Minecraft.getInstance().gameRenderer.mainRenderTarget().getDepthTexture());
            case ResourceRef.ShadowTex s -> Optional.of(r.targets.shadowDepth(0));
            default -> Optional.empty();
        };
    }

    /** @return a compiled pipeline's name, for messages */
    private static String name(CompiledRenderPipeline pipeline) {
        return pipeline instanceof FrontendRenderPipeline f ? f.name() : String.valueOf(pipeline);
    }

    private static List<Integer> targets(AttachmentLayout layout) {
        return layout.attachments().stream().map(AttachmentLayout.Attachment::target).toList();
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
        private final MinecraftHost host;
        /** Feature transform blocks to their shadow-camera copies (the feature shadow pass), else null. */
        private final Map<GpuBufferSlice, GpuBufferSlice> transforms;

        Draws(AttachmentLayout layout, boolean shadowPass, boolean packPrograms, FlipState flips, GpuBufferSlice frame, MinecraftHost host,
              Map<GpuBufferSlice, GpuBufferSlice> transforms) {
            this.layout = layout;
            this.shadowPass = shadowPass;
            this.packPrograms = packPrograms;
            this.flips = flips;
            this.frame = frame;
            this.host = host;
            this.transforms = transforms;
        }

        @Override
        public ActivePasses.Substitution substitute(CompiledRenderPipeline requested) {
            RenderPipeline vanilla = CompiledPipelineIndex.lookup(requested);
            if (vanilla == null) {
                return asIsOrSkip(requested, "a pipeline that did not come from Minecraft's pipeline cache (" + name(requested) + ")");
            }
            boolean casts = transforms == null || r.substitution.castsFeatureShadow(vanilla);
            if (casts && packPrograms && r.substitution.decide(vanilla, shadowPass) instanceof DrawSubstitution.Decision.Pack p) {
                return new ActivePasses.Substitution(p.resolution().compiled(), Optional.of(new PackDraw(p)));
            }
            Optional<CompiledRenderPipeline> clone = shadowPass ? r.clones.discard(vanilla, layout)
                : r.clones.fallback(vanilla, layout, r.dim.settings().fallbackTex());
            return clone.map(ActivePasses.Substitution::unchanged).orElseGet(() -> asIsOrSkip(requested, "pipeline " + vanilla.getLocation()));
        }

        @Override
        public GpuBufferSlice uniform(String name, GpuBufferSlice value) {
            return transforms != null && DYNAMIC_TRANSFORMS.equals(name) ? transforms.getOrDefault(value, value) : value;
        }

        /**
         * A pipeline that cannot be adapted is bound as it is when it fits the gbuffers pass;
         * otherwise its draws are skipped (reported once) rather than failing in Mojang's
         * attachment check, which would end the pack. Nothing unknown draws into the shadow map.
         */
        private ActivePasses.Substitution asIsOrSkip(CompiledRenderPipeline requested, String what) {
            if (!shadowPass && requested instanceof FrontendRenderPipeline f && VanillaClones.fits(f.colorTargetStates(), layout)) {
                return ActivePasses.Substitution.unchanged(requested);
            }
            r.diagnostics.report(what + " was drawn during world rendering and cannot be adapted to the shader pack's "
                + (shadowPass ? "shadow" : "gbuffers") + " render pass; its draws are skipped");
            return ActivePasses.Substitution.skip();
        }

        /** The descriptors of a pack pipeline replacing a vanilla draw. */
        private final class PackDraw implements ActivePasses.PackBinding {
            private final DrawSubstitution.Decision.Pack pack;

            PackDraw(DrawSubstitution.Decision.Pack pack) {
                this.pack = pack;
            }

            @Override
            public void bind(UniformTarget target) {
                Optional<TextureBinding> albedo = target.boundTexture(ActivePasses.ALBEDO_SAMPLER);
                r.binder.bind(target, pack.resolution().pipeline().bindings(), pack.program(), flips, frame, draw(albedo), host(albedo));
            }

            @Override
            public void albedoChanged(UniformTarget target) {
                Optional<TextureBinding> albedo = target.boundTexture(ActivePasses.ALBEDO_SAMPLER);
                r.binder.bindAlbedo(target, pack.resolution().pipeline().bindings(), pack.program(), draw(albedo), host(albedo));
            }

            private GpuBufferSlice draw(Optional<TextureBinding> albedo) {
                AlbedoSize size = AlbedoSize.of(albedo.map(TextureBinding::view), r.atlases::contains);
                return r.drawSlots.slice(DrawKey.of(pack.resolution().pipeline().key().toString(), pack.program(), RenderStages.of(pack.routed()),
                    shadowPass, size));
            }

            private MinecraftHost host(Optional<TextureBinding> albedo) {
                return albedo.map(host::withAlbedo).orElse(host);
            }
        }
    }
}
