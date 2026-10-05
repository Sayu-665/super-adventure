package dev.shaderbridge.render.chunk;

import com.mojang.blaze3d.vertex.DefaultVertexFormat;
import com.mojang.renderpearl.api.pipeline.ColorTargetState;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import dev.shaderbridge.render.mapping.VanillaPipelineTable;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Vanilla terrain pipelines drawing the extended chunk vertex: the same shaders, defines, bind
 * groups, color targets, depth state, culling, topology, push constants and instance data, with
 * {@link TerrainVertexFormat#EXTENDED} in vertex buffer slot 0. The vanilla shaders read only the
 * {@code BLOCK} elements, which keep their offsets; Mojang's pipeline builder binds vertex
 * elements to shader inputs by name and skips elements no input reads, so the clones draw the
 * extended meshes exactly as the vanilla pipelines draw {@code BLOCK} meshes. They are what
 * Minecraft binds for chunk sections while the extended format is active: in frames without the
 * pack (its pipelines still compiling, the wireframe view, order-independent transparency) and as
 * the vanilla pipeline ShaderBridge's passes substitute or adapt ({@code VanillaClones} copies
 * their vertex layout).
 *
 * <p>Clones are located at {@link VanillaPipelineTable#extendedTerrainLocation}, which routes them
 * to the extended terrain profiles, and kept for the life of the process (Mojang's pipeline cache
 * compiles them by identity, and recompiles them after a resource reload). Thread-safe.
 */
public final class ExtendedTerrainPipelines {
    private static final Map<RenderPipeline, RenderPipeline> CLONES = new ConcurrentHashMap<>();

    private ExtendedTerrainPipelines() {
    }

    /**
     * @param pipeline a render pipeline (or null)
     * @return its extended-format clone if it draws {@code BLOCK} vertices from slot 0, else the
     *     pipeline itself (also for clones, so the call is idempotent)
     */
    public static RenderPipeline extended(RenderPipeline pipeline) {
        if (pipeline == null || !drawsBlockVertices(pipeline)) {
            return pipeline;
        }
        return CLONES.computeIfAbsent(pipeline, ExtendedTerrainPipeline::new);
    }

    /**
     * @param pipeline a render pipeline
     * @return whether its vertex buffer slot 0 has Mojang's {@code BLOCK} format
     */
    public static boolean drawsBlockVertices(RenderPipeline pipeline) {
        List<VertexFormat> bindings = pipeline.getVertexFormatBindings();
        return !bindings.isEmpty() && bindings.getFirst() == DefaultVertexFormat.BLOCK;
    }

    /** A vanilla pipeline with the extended vertex format in slot 0. */
    private static final class ExtendedTerrainPipeline extends RenderPipeline {
        ExtendedTerrainPipeline(RenderPipeline vanilla) {
            super(VanillaPipelineTable.extendedTerrainLocation(vanilla.getLocation()), vanilla.getShaders(), vanilla.getShaderDefines(),
                vanilla.getBindGroupLayouts(), vanilla.getColorTargetStates().toArray(ColorTargetState[]::new), vanilla.getDepthStencilState(),
                vanilla.getPolygonMode(), vanilla.isCull(), extendedBindings(vanilla), vanilla.getPrimitiveTopology(), vanilla.pushConstantSize(),
                vanilla.getSortKey());
        }

        private static VertexFormat[] extendedBindings(RenderPipeline vanilla) {
            VertexFormat[] bindings = vanilla.getVertexFormatBindings().toArray(VertexFormat[]::new);
            bindings[0] = TerrainVertexFormat.EXTENDED;
            return bindings;
        }
    }
}
