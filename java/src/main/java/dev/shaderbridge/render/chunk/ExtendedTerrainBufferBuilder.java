package dev.shaderbridge.render.chunk;

import com.mojang.blaze3d.vertex.BufferBuilder;
import com.mojang.blaze3d.vertex.ByteBufferBuilder;
import com.mojang.blaze3d.vertex.MeshData;
import com.mojang.blaze3d.vertex.VertexConsumer;
import com.mojang.renderpearl.api.pipeline.PrimitiveTopology;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import java.util.Arrays;
import java.util.concurrent.atomic.AtomicBoolean;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * The buffer builder of a chunk section layer meshed in the extended terrain vertex format.
 * Minecraft's code writes the {@code BLOCK} elements as usual (with the extended format,
 * {@link BufferBuilder} takes its generic per-element path, which starts every vertex with
 * {@link #addVertex(float, float, float)}); this builder notes, at the first vertex of each quad,
 * which block the quad belongs to ({@link TerrainVertexContext}), and fills the extension
 * attributes of the whole mesh when it is built ({@link TerrainVertexEncoder}), before the
 * translucent layer's quads are sorted. Used by one meshing thread.
 */
final class ExtendedTerrainBufferBuilder extends BufferBuilder {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");
    private static final AtomicBoolean ENCODE_FAILURE_LOGGED = new AtomicBoolean();

    private final PrimitiveTopology topology;
    private final TerrainVertexEncoder.Layout layout;
    private final TerrainVertexContext context;
    private final BlockIdTable ids;
    private long[] quads = new long[64];
    private int vertices;

    /**
     * @param buffer   the layer's memory
     * @param topology the primitive topology (Minecraft meshes chunk sections as quads)
     * @param format   {@link TerrainVertexFormat#EXTENDED}
     * @param context  the meshing thread's block context
     * @param ids      the pack's block ids
     */
    ExtendedTerrainBufferBuilder(ByteBufferBuilder buffer, PrimitiveTopology topology, VertexFormat format, TerrainVertexContext context,
                                 BlockIdTable ids) {
        super(buffer, topology, format);
        this.topology = topology;
        this.layout = TerrainVertexFormat.layoutOf(format);
        this.context = context;
        this.ids = ids;
    }

    @Override
    public VertexConsumer addVertex(float x, float y, float z) {
        if ((vertices & 3) == 0) {
            int quad = vertices >>> 2;
            if (quad == quads.length) {
                quads = Arrays.copyOf(quads, quads.length * 2);
            }
            quads[quad] = context.record(ids);
        }
        vertices++;
        return super.addVertex(x, y, z);
    }

    @Override
    public MeshData build() {
        MeshData mesh = super.build();
        if (mesh != null) {
            int count = Math.min(vertices, mesh.drawState().vertexCount());
            try {
                if (topology == PrimitiveTopology.QUADS) {
                    TerrainVertexEncoder.encode(mesh.vertexBuffer(), layout, count, quads);
                } else {
                    TerrainVertexEncoder.encodeNeutral(mesh.vertexBuffer(), layout, 0, count);
                }
            } catch (RuntimeException e) {
                // Never take down chunk meshing: the mesh keeps its vanilla attributes.
                if (ENCODE_FAILURE_LOGGED.compareAndSet(false, true)) {
                    LOGGER.error("Could not fill the extended terrain vertex attributes of a chunk mesh", e);
                }
            }
        }
        return mesh;
    }
}
