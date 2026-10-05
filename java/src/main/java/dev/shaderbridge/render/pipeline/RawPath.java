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

    /**
     * Dispatches an admitted compute program between two of Minecraft's render passes, with full
     * memory barriers around it. Called by the frame orchestration on the render thread, outside
     * any render pass, for programs this path {@linkplain #admit admitted} and reports
     * {@linkplain RawProgram.State.Ready ready}. A raw path that admits compute programs must
     * override this; the default refuses, which the caller reports and skips.
     *
     * @param program  the prepared program
     * @param dispatch what to dispatch and the frame state it sees
     * @throws UnsupportedOperationException if this raw path does not dispatch compute programs
     */
    default void dispatch(RawProgram program, RawDispatch dispatch) {
        throw new UnsupportedOperationException("the raw Vulkan path does not dispatch compute programs");
    }

    /**
     * Draws an admitted composite-style program in its own render pass, between two of
     * Minecraft's passes, with full memory barriers around it. Called by the frame orchestration
     * on the render thread, outside any render pass, for programs this path
     * {@linkplain #admit admitted} and reports {@linkplain RawProgram.State.Ready ready}. A raw
     * path that admits composite-style programs must override this; the default refuses, which
     * the caller reports and skips.
     *
     * @param program the prepared program
     * @param draw    the attachments and the frame state the draw sees
     * @return per attachment slot, whether the draw wrote it
     * @throws UnsupportedOperationException if this raw path does not draw composite-style programs
     */
    default List<Boolean> draw(RawProgram program, RawDraw draw) {
        throw new UnsupportedOperationException("the raw Vulkan path does not draw composite-style programs");
    }

    /**
     * @return the largest work group counts {@code [x, y, z]} a dispatch may have; the default is
     *     the minimum every Vulkan device supports
     */
    default int[] maxWorkGroups() {
        return new int[] {65535, 65535, 65535};
    }

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
