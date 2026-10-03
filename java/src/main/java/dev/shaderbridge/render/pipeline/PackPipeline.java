package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import java.util.List;
import net.minecraft.resources.Identifier;

/**
 * A pack program as a renderpearl pipeline, ready to compile, with what a draw must bind.
 *
 * @param key         identity in the {@link PackPipelineCache}
 * @param pipeline    the pipeline description
 * @param bindings    what to bind for each descriptor
 * @param attachments color target states and the targets the program writes
 * @param modules     ids of the registered SPIR-V modules (vertex, fragment)
 */
public record PackPipeline(PipelineKey key, RenderPipeline pipeline, BindingPlan bindings, AttachmentPlan attachments, List<Identifier> modules) {
    public PackPipeline {
        modules = List.copyOf(modules);
    }
}
