package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.BlendMode;
import dev.shaderbridge.model.Program;
import java.util.List;
import java.util.Map;

/** Variations of fixture programs for the tests. */
final class Programs {
    private Programs() {
    }

    /**
     * @return a copy of {@code p} with other outputs and blending
     */
    static Program outputs(Program p, List<Integer> drawBuffers, List<Integer> outputSlots, BlendMode blend, Map<Integer, BlendMode> perBuffer) {
        return new Program(p.name(), p.kind(), p.drawProfile(), p.requiresRawVulkan(), p.stages(), drawBuffers, outputSlots, p.outputTypes(), blend,
            perBuffer, p.alphaTest(), p.viewport(), p.mipmapTargets(), p.bindingsUsed(), p.vertexInputs(), p.pushConstantSize(), p.compute(),
            p.cull(), p.synthesizedFrom());
    }

    /** @return a copy of {@code p} marked as needing the raw Vulkan path */
    static Program rawOnly(Program p) {
        return new Program(p.name(), p.kind(), p.drawProfile(), true, p.stages(), p.drawBuffers(), p.outputSlots(), p.outputTypes(), p.blend(),
            p.blendPerBuffer(), p.alphaTest(), p.viewport(), p.mipmapTargets(), p.bindingsUsed(), p.vertexInputs(), p.pushConstantSize(),
            p.compute(), p.cull(), p.synthesizedFrom());
    }
}
