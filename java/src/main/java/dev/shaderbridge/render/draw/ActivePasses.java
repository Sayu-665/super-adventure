package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import java.util.Optional;

/**
 * The ShaderBridge render pass vanilla code is currently drawing into, if any, and what decides
 * its pipelines. Mojang's command encoders hold one open render pass at a time, so one slot
 * suffices; a pass is identified by the object Minecraft's code calls {@code setPipeline} on.
 * Render thread only.
 */
public final class ActivePasses {
    /** The sampler Minecraft binds a draw's albedo texture to. */
    public static final String ALBEDO_SAMPLER = "Sampler0";

    private static RenderPass current;
    private static PassDraws draws;

    private ActivePasses() {
    }

    /** Chooses the pipeline of each vanilla draw in a ShaderBridge pass. */
    @FunctionalInterface
    public interface PassDraws {
        /**
         * @param requested the pipeline vanilla code binds
         * @return the pipeline to bind instead and what to bind with it, or
         *     {@link Substitution#skip()} when the draws must not happen
         */
        Substitution substitute(CompiledRenderPipeline requested);

        /**
         * Replaces a uniform buffer vanilla code binds in the pass (by default, nothing is
         * replaced).
         *
         * @param name  the uniform name
         * @param value the slice vanilla code binds
         * @return the slice to bind instead
         */
        default GpuBufferSlice uniform(String name, GpuBufferSlice value) {
            return value;
        }
    }

    /** Binds the descriptors of a pack pipeline in a render pass. */
    public interface PackBinding {
        /**
         * Binds every descriptor of the pipeline the pass just bound.
         *
         * @param target the pass
         */
        void bind(UniformTarget target);

        /**
         * Rebinds what depends on the draw's albedo once vanilla code bound another texture as
         * {@link #ALBEDO_SAMPLER} (Minecraft binds a draw's textures after its pipeline).
         *
         * @param target the pass
         */
        void albedoChanged(UniformTarget target);
    }

    /**
     * @param pipeline the pipeline to bind, or null when the vanilla draws are skipped
     * @param binding  the descriptors to bind with it: present for pack pipelines, empty for
     *                 vanilla pipelines (bound by vanilla code)
     */
    public record Substitution(CompiledRenderPipeline pipeline, Optional<PackBinding> binding) {
        private static final Substitution SKIP = new Substitution(null, Optional.empty());

        /**
         * @param pipeline a pipeline
         * @return the pipeline bound as is, with nothing more to bind
         */
        public static Substitution unchanged(CompiledRenderPipeline pipeline) {
            return new Substitution(pipeline, Optional.empty());
        }

        /**
         * @return the pipeline is not bound, and every draw (and push constant) until the next
         *     pipeline is dropped: for pipelines that cannot be drawn in the pass at all
         */
        public static Substitution skip() {
            return SKIP;
        }

        /** @return whether the draws of this pipeline are dropped */
        public boolean skipped() {
            return pipeline == null;
        }
    }

    /**
     * Makes a pass the current ShaderBridge pass.
     *
     * @param pass  a pass ShaderBridge created
     * @param draws decides its pipelines
     */
    public static void open(RenderPass pass, PassDraws draws) {
        ActivePasses.current = pass;
        ActivePasses.draws = draws;
    }

    /**
     * Forgets a pass (when it is closed).
     *
     * @param pass the pass
     */
    public static void close(RenderPass pass) {
        if (pass != null && current == pass) {
            current = null;
            draws = null;
        }
    }

    /**
     * @param pass a render pass
     * @return whether it is the current ShaderBridge pass
     */
    public static boolean owns(Object pass) {
        return pass != null && pass == current;
    }

    /**
     * @param pass      the pass {@code setPipeline} is called on
     * @param requested the pipeline vanilla code binds
     * @return the substitution, or null when the pass is not a ShaderBridge pass
     */
    public static Substitution substitute(Object pass, CompiledRenderPipeline requested) {
        if (!owns(pass) || draws == null) {
            return null;
        }
        return draws.substitute(requested);
    }

    /**
     * @param pass  the pass {@code setUniform} is called on
     * @param name  the uniform name
     * @param value the slice vanilla code binds
     * @return the slice to bind: replaced in ShaderBridge passes that replace it, else {@code value}
     */
    public static GpuBufferSlice uniform(Object pass, String name, GpuBufferSlice value) {
        if (!owns(pass) || draws == null || value == null) {
            return value;
        }
        return draws.uniform(name, value);
    }
}
