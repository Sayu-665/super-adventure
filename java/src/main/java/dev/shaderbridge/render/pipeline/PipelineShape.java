package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.pipeline.BindGroupLayout;
import com.mojang.renderpearl.api.pipeline.DepthStencilState;
import com.mojang.renderpearl.api.pipeline.PolygonMode;
import com.mojang.renderpearl.api.pipeline.PrimitiveTopology;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Everything about a pack pipeline that comes from the draw it replaces rather than from the pack:
 * vertex buffer layouts, primitive topology, depth test template, default culling, polygon mode,
 * push constant size, and the descriptors the host binds itself (their declared types, e.g. the
 * format of a texel buffer). A pack program substituted for a vanilla pipeline draws the vanilla
 * vertex data, so the shape is copied from that pipeline.
 *
 * @param id               short name, part of the pack pipeline's location
 * @param vertexBindings   vertex format per buffer slot (null for unused slots)
 * @param topology         primitive topology
 * @param depth            depth state template (vanilla, reversed-Z), or null for no depth test
 * @param cull             default back-face culling, overridden by {@code program.cull}
 * @param polygonMode      polygon mode
 * @param pushConstantSize push constant bytes the host pushes (0 if none)
 * @param hostUniforms     descriptors the host declares, by name
 */
public record PipelineShape(
    String id,
    List<VertexFormat> vertexBindings,
    PrimitiveTopology topology,
    DepthStencilState depth,
    boolean cull,
    PolygonMode polygonMode,
    int pushConstantSize,
    Map<String, BindGroupLayout.UniformDescription> hostUniforms
) {
    public PipelineShape {
        List<VertexFormat> trimmed = new ArrayList<>(vertexBindings);
        while (!trimmed.isEmpty() && trimmed.getLast() == null) {
            trimmed.removeLast();
        }
        vertexBindings = Collections.unmodifiableList(trimmed);
        hostUniforms = Collections.unmodifiableMap(new LinkedHashMap<>(hostUniforms));
    }

    /**
     * The shape of a vanilla pipeline a pack program replaces.
     *
     * @param vanilla the vanilla pipeline
     * @return its shape; the id is the pipeline's location path
     */
    public static PipelineShape of(RenderPipeline vanilla) {
        Map<String, BindGroupLayout.UniformDescription> host = new LinkedHashMap<>();
        for (BindGroupLayout.UniformDescription u : BindGroupLayout.flattenUniforms(vanilla.getBindGroupLayouts())) {
            host.putIfAbsent(u.name(), u);
        }
        return new PipelineShape(vanilla.getLocation().getNamespace() + "/" + vanilla.getLocation().getPath(), vanilla.getVertexFormatBindings(),
            vanilla.getPrimitiveTopology(), vanilla.getDepthStencilState(), vanilla.isCull(), vanilla.getPolygonMode(), vanilla.pushConstantSize(),
            host);
    }

    /** @return the shape of composite-style passes: three vertices, no buffers, no depth test */
    public static PipelineShape fullscreen() {
        return new PipelineShape("fullscreen", List.of(), PrimitiveTopology.TRIANGLES, null, false, PolygonMode.FILL, 0, Map.of());
    }
}
