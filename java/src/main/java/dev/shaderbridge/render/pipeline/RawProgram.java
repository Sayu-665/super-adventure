package dev.shaderbridge.render.pipeline;

/**
 * A program prepared by the {@link RawPath}: its Vulkan pipeline, descriptor set layouts and
 * pools, built from the program's SPIR-V. Executing it (dispatching a compute program between
 * passes, drawing a geometry program) is the raw path's business; this handle only reports the
 * preparation state. Render thread only.
 */
public interface RawProgram extends AutoCloseable {
    /** @return the program and the blobs it was built from */
    ProgramVariant program();

    /** @return how far the preparation is */
    State state();

    /** Releases the Vulkan objects (deferred until the GPU no longer uses them). */
    @Override
    void close();

    /** Preparation state. */
    sealed interface State {
        /** Still compiling. */
        record Preparing() implements State {
        }

        /** Ready to execute. */
        record Ready() implements State {
        }

        /** @param reason why the program cannot run */
        record Failed(String reason) implements State {
        }
    }
}
