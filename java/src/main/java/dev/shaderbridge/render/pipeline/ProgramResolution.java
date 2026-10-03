package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import java.util.List;

/** How a program runs this frame. */
public sealed interface ProgramResolution {
    /**
     * Draw with a compiled renderpearl pipeline.
     *
     * @param pipeline what to bind for its descriptors and which targets it writes
     * @param compiled the pipeline to pass to {@code RenderPass.setPipeline}; owned by the cache
     */
    record Renderpearl(PackPipeline pipeline, CompiledRenderPipeline compiled) implements ProgramResolution {
    }

    /** @param program the prepared raw-path program; owned by the resolver */
    record Raw(RawProgram program) implements ProgramResolution {
    }

    /**
     * Not ready yet (a variant or a pipeline is compiling): draw vanilla, or skip the pass, this
     * frame.
     *
     * @param what what is being waited for
     */
    record Pending(String what) implements ProgramResolution {
    }

    /** @param reasons why the program cannot run at all */
    record Unavailable(List<String> reasons) implements ProgramResolution {
        public Unavailable {
            reasons = List.copyOf(reasons);
        }
    }
}
