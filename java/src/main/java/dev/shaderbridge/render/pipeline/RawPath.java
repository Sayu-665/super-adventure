package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.DimensionPipeline;
import java.util.List;

/**
 * Runs programs that Mojang's pipeline API cannot express directly on the Vulkan device: compute
 * programs, geometry and tessellation stages, storage images and buffers, 1D/3D textures, more
 * descriptors than renderpearl binds. Implemented by the raw-Vulkan integration; {@link #NONE}
 * when it is unavailable (OpenGL backend, missing device features, failed initialization), in
 * which case such programs are skipped and geometry falls back along the pack's program chain.
 */
public interface RawPath {
    /** A raw path that accepts nothing. */
    RawPath NONE = (dim, program, renderpearlProblems) -> new Admission.Rejected("the raw Vulkan path is not available");

    /**
     * Offers a program renderpearl cannot run. The raw path starts preparing it (asynchronously
     * if it likes) or declines. Called at most once per program and draw shape by
     * {@link ProgramResolver}, on the render thread.
     *
     * @param dim                 the program's dimension pipeline (binding table, targets, uniforms)
     * @param program             the program with its blobs
     * @param renderpearlProblems why renderpearl cannot run it
     * @return the prepared program, or why the raw path cannot run it either
     */
    Admission admit(DimensionPipeline dim, ProgramVariant program, List<String> renderpearlProblems);

    /** Outcome of {@link #admit}. */
    sealed interface Admission {
        /** @param program the program being prepared; the caller owns it */
        record Accepted(RawProgram program) implements Admission {
        }

        /** @param reason why the raw path cannot run the program */
        record Rejected(String reason) implements Admission {
        }
    }
}
