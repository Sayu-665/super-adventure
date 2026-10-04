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
import java.util.Optional;

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

    /**
     * @param enabled default back-face culling
     * @return this shape with another default culling (the shadow pass draws without culling, as
     *     Iris and the headless executor do)
     */
    public PipelineShape withCull(boolean enabled) {
        return new PipelineShape(id, vertexBindings, topology, depth, enabled, polygonMode, pushConstantSize, hostUniforms);
    }

    /**
     * The shape of geometry ShaderBridge draws from a draw profile's vertex buffers rather than in
     * place of a vanilla pipeline: shadow-pass re-renders, Distant Horizons LODs, Sodium terrain.
     *
     * @param profile  the draw profile (also the shape id)
     * @param formats  vertex layouts of the profiles
     * @param topology primitive topology of the draws
     * @param depth    depth test (see {@link DepthStates#standard})
     * @param cull     default back-face culling, overridden by {@code program.cull}
     * @return the shape, or empty if the profile has no known vertex layout
     */
    public static Optional<PipelineShape> ofProfile(String profile, ProfileVertexFormats formats, PrimitiveTopology topology, DepthStencilState depth,
                                                    boolean cull) {
        return formats.bindings(profile).map(bindings -> new PipelineShape(profile, bindings, topology, depth, cull, PolygonMode.FILL, 0, Map.of()));
    }

    /**
     * @return the shape of composite-style passes: the {@code fullscreen} profile's six vertices
     *     (two triangles) generated from the vertex index, no buffers, no depth test
     */
    public static PipelineShape fullscreen() {
        return new PipelineShape("fullscreen", List.of(), PrimitiveTopology.TRIANGLES, null, false, PolygonMode.FILL, 0, Map.of());
    }
}
