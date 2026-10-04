package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.api.pipeline.ColorTargetState;
import com.mojang.renderpearl.api.pipeline.DepthStencilState;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import java.util.List;
import net.minecraft.resources.Identifier;

/**
 * A vanilla pipeline with other color targets and depth state: the same shaders, defines, bind
 * groups, vertex layout, topology, culling and push constants, so Mojang's own shader source
 * compiles it. Lets a vanilla draw run inside a ShaderBridge render pass whose attachments differ
 * from the main target's.
 */
final class ClonedPipeline extends RenderPipeline {
    /**
     * @param location     the clone's location
     * @param vanilla      the pipeline to copy
     * @param colorTargets one state per attachment slot of the pass (null: unused slot)
     * @param depth        the depth state, or null for no depth test and no depth writes
     */
    ClonedPipeline(Identifier location, RenderPipeline vanilla, List<ColorTargetState> colorTargets, DepthStencilState depth) {
        super(location, vanilla.getShaders(), vanilla.getShaderDefines(), vanilla.getBindGroupLayouts(), colorTargets.toArray(ColorTargetState[]::new),
            depth, vanilla.getPolygonMode(), vanilla.isCull(), vanilla.getVertexFormatBindings().toArray(VertexFormat[]::new),
            vanilla.getPrimitiveTopology(), vanilla.pushConstantSize(), vanilla.getSortKey());
    }
}
