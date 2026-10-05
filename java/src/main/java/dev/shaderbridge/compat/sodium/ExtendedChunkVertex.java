package dev.shaderbridge.compat.sodium;

import com.mojang.renderpearl.api.vertex.VertexFormat;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import net.caffeinemc.mods.sodium.client.render.chunk.vertex.format.ChunkVertexEncoder;
import net.caffeinemc.mods.sodium.client.render.chunk.vertex.format.ChunkVertexType;
import org.lwjgl.system.MemoryUtil;

/**
 * Sodium chunk vertex type of the extended terrain vertex ({@link TerrainVertexLayout}). Its
 * encoder lets Sodium's own compact encoder write the first 20 bytes of each vertex (so they are
 * exactly what Sodium's shader and the {@value SodiumPipelines#PROFILE} profile decode), then
 * appends the extension attributes ({@link TerrainExtension}): from the block being meshed on the
 * encoding thread ({@link BlockContext}) when there is one, otherwise from the tags of the
 * vertices ({@link VertexTags}: translucent quads Sodium split while sorting).
 */
final class ExtendedChunkVertex implements ChunkVertexType {
    /** Room for Sodium's four compact vertices, with a margin. */
    private static final int SCRATCH_BYTES = 4 * 64;
    private static final ThreadLocal<Scratch> SCRATCH = ThreadLocal.withInitial(Scratch::new);

    private final ChunkVertexType compact;
    private final BlockIdTable ids;

    /**
     * @param compact Sodium's compact vertex type (writes the first 20 bytes)
     * @param ids     the pack's block ids ({@link SodiumTerrain#enterBlock} resolves blocks with them)
     */
    ExtendedChunkVertex(ChunkVertexType compact, BlockIdTable ids) {
        this.compact = compact;
        this.ids = ids;
    }

    /** @return the pack's block ids */
    BlockIdTable ids() {
        return ids;
    }

    @Override
    public VertexFormat getVertexFormat() {
        return TerrainVertexLayout.format();
    }

    @Override
    public ChunkVertexEncoder getEncoder() {
        return new Encoder(compact.getEncoder());
    }

    /**
     * @param vertices a quad's vertices
     * @return the {@code sb_MidTexCoord} value of the quad
     */
    static int midTexCoord(ChunkVertexEncoder.Vertex[] vertices) {
        float u = 0.0f;
        float v = 0.0f;
        for (ChunkVertexEncoder.Vertex vertex : vertices) {
            u += vertex.u;
            v += vertex.v;
        }
        return TerrainExtension.midTexCoord(u * 0.25f, v * 0.25f);
    }

    /** Per-thread memory of the encoder. */
    private static final class Scratch {
        final ByteBuffer buffer = ByteBuffer.allocateDirect(SCRATCH_BYTES).order(ByteOrder.nativeOrder());
        final long address = MemoryUtil.memAddress(buffer);
        final float[] positions = new float[12];
    }

    /** Writes quads in the extended vertex. Thread-safe (Sodium encodes on its worker threads). */
    static final class Encoder implements ChunkVertexEncoder {
        private final ChunkVertexEncoder compact;

        /** @param compact Sodium's compact encoder */
        Encoder(ChunkVertexEncoder compact) {
            this.compact = compact;
        }

        @Override
        public long write(long ptr, int materialBits, Vertex[] vertices, int sectionIndex) {
            Scratch scratch = SCRATCH.get();
            compact.write(scratch.address, materialBits, vertices, sectionIndex);

            int entity;
            int block;
            int midTexCoord;
            BlockContext context = BlockContext.current();
            if (context.active()) {
                entity = context.entity();
                block = context.block();
                midTexCoord = midTexCoord(vertices);
            } else if (vertices[0] instanceof VertexTags tags) {
                entity = tags.shaderbridge$entity();
                block = tags.shaderbridge$block();
                midTexCoord = tags.shaderbridge$midTexCoord() != 0 ? tags.shaderbridge$midTexCoord() : midTexCoord(vertices);
            } else {
                entity = 0;
                block = TerrainExtension.NO_BLOCK;
                midTexCoord = midTexCoord(vertices);
            }
            float[] positions = scratch.positions;
            for (int i = 0; i < 4; i++) {
                positions[i * 3] = vertices[i].x;
                positions[i * 3 + 1] = vertices[i].y;
                positions[i * 3 + 2] = vertices[i].z;
            }
            int normal = TerrainExtension.normal(positions);

            for (int i = 0; i < 4; i++) {
                long dst = ptr + (long) i * TerrainVertexLayout.VERTEX_SIZE;
                MemoryUtil.memCopy(scratch.address + (long) i * TerrainVertexLayout.SODIUM_VERTEX_SIZE, dst, TerrainVertexLayout.SODIUM_VERTEX_SIZE);
                MemoryUtil.memPutInt(dst + TerrainVertexLayout.ENTITY_OFFSET, entity);
                MemoryUtil.memPutInt(dst + TerrainVertexLayout.NORMAL_OFFSET, normal);
                MemoryUtil.memPutInt(dst + TerrainVertexLayout.MID_TEX_COORD_OFFSET, midTexCoord);
                Vertex vertex = vertices[i];
                MemoryUtil.memPutInt(dst + TerrainVertexLayout.MID_BLOCK_OFFSET, TerrainExtension.midBlock(block, vertex.x, vertex.y, vertex.z));
            }
            return ptr + 4L * TerrainVertexLayout.VERTEX_SIZE;
        }
    }
}
