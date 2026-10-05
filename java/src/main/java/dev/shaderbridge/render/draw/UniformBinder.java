package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.render.frame.ColorReads;
import dev.shaderbridge.render.frame.FlipState;
import dev.shaderbridge.render.pipeline.BindingPlan;
import dev.shaderbridge.render.targets.HostTextures;
import dev.shaderbridge.render.targets.TextureBinding;
import java.util.Optional;
import java.util.function.Consumer;

/**
 * Binds the descriptors of a pack pipeline per its {@link BindingPlan}: {@code sb_Frame},
 * {@code sb_Draw}, and every pack sampler in the texture {@link ColorReads} chooses (main or alt).
 * Host blocks and samplers are left to the host draw path; a host sampler with a known fallback
 * resource gets the fallback only when the pass has no value for it yet. Render thread only.
 */
public final class UniformBinder {
    /** Resolves a pack resource to a texture ({@code TextureResolver::resolve}). */
    @FunctionalInterface
    public interface Textures {
        /**
         * @param resource the resource
         * @param alt      bind the alternate texture of a ping-ponged target
         * @param program  the sampling program
         * @param host     the game's textures
         * @return what to bind
         */
        TextureBinding resolve(ResourceRef resource, boolean alt, Program program, HostTextures host);
    }

    private final Textures textures;
    private final Consumer<String> warnings;

    /**
     * @param textures resolves pack resources
     * @param warnings receives main/alt disagreements (deduplicated by the receiver)
     */
    public UniformBinder(Textures textures, Consumer<String> warnings) {
        this.textures = textures;
        this.warnings = warnings;
    }

    /**
     * @param target  the render pass
     * @param plan    the pipeline's binding plan
     * @param program the program the pipeline runs
     * @param flips   the frame's flip state
     * @param frame   the {@code sb_Frame} slice
     * @param draw    the {@code sb_Draw} slice
     * @param host    the game's textures for this draw
     */
    public void bind(UniformTarget target, BindingPlan plan, Program program, FlipState flips, GpuBufferSlice frame, GpuBufferSlice draw,
                     HostTextures host) {
        for (BindingPlan.Binding binding : plan.bindings()) {
            switch (binding.source()) {
                case BindingPlan.Source.FrameBlock f -> target.bind(binding.name(), frame);
                case BindingPlan.Source.DrawBlock d -> target.bind(binding.name(), draw);
                case BindingPlan.Source.Host h -> hostFallback(target, binding.name(), h.fallback(), program, flips, host);
                case BindingPlan.Source.Pack p -> {
                    boolean alt = ColorReads.alt(program.kind(), p.resource(), p.useAlt(), flips, w -> warnings.accept(program.name() + ": " + w));
                    target.bind(binding.name(), textures.resolve(p.resource(), alt, program, host));
                }
                case BindingPlan.Source.Unresolved u -> {
                    // Pipelines with unresolved descriptors are never built.
                }
            }
        }
    }

    /**
     * Binds again what depends on the draw's albedo, after the host bound another texture as
     * {@code Sampler0}: {@code sb_Draw} (its texture sizes) and the pack samplers that sample the
     * albedo.
     *
     * @param target  the render pass
     * @param plan    the pipeline's binding plan
     * @param program the program the pipeline runs
     * @param draw    the {@code sb_Draw} slice for the new albedo
     * @param host    the game's textures with the new albedo
     */
    public void bindAlbedo(UniformTarget target, BindingPlan plan, Program program, GpuBufferSlice draw, HostTextures host) {
        for (BindingPlan.Binding binding : plan.bindings()) {
            switch (binding.source()) {
                case BindingPlan.Source.DrawBlock d -> target.bind(binding.name(), draw);
                case BindingPlan.Source.Pack p when samplesAlbedo(p.resource(), program) ->
                    target.bind(binding.name(), textures.resolve(p.resource(), false, program, host));
                default -> {
                    // Independent of the albedo.
                }
            }
        }
    }

    /**
     * @param resource a sampled resource
     * @param program  the sampling program
     * @return whether it resolves to the draw's albedo: the atlas, and samplers the pack did not
     *     name in a geometry program (GL texture unit 0)
     */
    static boolean samplesAlbedo(ResourceRef resource, Program program) {
        return resource instanceof ResourceRef.Atlas || resource instanceof ResourceRef.Unknown && program.kind() instanceof ProgramKind.Geometry;
    }

    private void hostFallback(UniformTarget target, String name, Optional<ResourceRef> fallback, Program program, FlipState flips, HostTextures host) {
        if (fallback.isPresent() && !target.isBound(name)) {
            ResourceRef resource = fallback.get();
            target.bind(name, textures.resolve(resource, ColorReads.alt(program.kind(), resource, false, flips, w -> { }), program, host));
        }
    }
}
