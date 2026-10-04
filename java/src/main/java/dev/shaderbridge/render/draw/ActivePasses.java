package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import java.util.function.Consumer;

/**
 * The ShaderBridge render pass vanilla code is currently drawing into, if any, and what decides
 * its pipelines. Mojang's command encoders hold one open render pass at a time, so one slot
 * suffices; a pass is identified by the object Minecraft's code calls {@code setPipeline} on.
 * Render thread only.
 */
public final class ActivePasses {
    private static RenderPass current;
    private static PassDraws draws;

    private ActivePasses() {
    }

    /** Chooses the pipeline of each vanilla draw in a ShaderBridge pass. */
    @FunctionalInterface
    public interface PassDraws {
        /**
         * @param requested the pipeline vanilla code binds
         * @return the pipeline to bind instead and what to bind after it
         */
        Substitution substitute(CompiledRenderPipeline requested);
    }

    /**
     * @param pipeline the pipeline to bind
     * @param bind     binds its descriptors once it is bound (does nothing for vanilla pipelines)
     */
    public record Substitution(CompiledRenderPipeline pipeline, Consumer<UniformTarget> bind) {
        /**
         * @param pipeline a pipeline
         * @return the pipeline bound as is, with nothing more to bind
         */
        public static Substitution unchanged(CompiledRenderPipeline pipeline) {
            return new Substitution(pipeline, target -> { });
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
        if (current == pass) {
            current = null;
            draws = null;
        }
    }

    /**
     * @param pass      the pass {@code setPipeline} is called on
     * @param requested the pipeline vanilla code binds
     * @return the substitution, or null when the pass is not a ShaderBridge pass
     */
    public static Substitution substitute(Object pass, CompiledRenderPipeline requested) {
        if (pass != current || draws == null) {
            return null;
        }
        return draws.substitute(requested);
    }
}
