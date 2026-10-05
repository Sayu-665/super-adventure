package dev.shaderbridge.compat.sodium;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;

import net.caffeinemc.mods.sodium.client.render.chunk.vertex.format.ChunkMeshFormats;
import net.caffeinemc.mods.sodium.client.render.chunk.vertex.format.ChunkVertexEncoder;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;
import org.lwjgl.system.MemoryUtil;

/**
 * {@link ExtendedChunkVertex} with Sodium's real compact encoder: the first 20 bytes of every
 * vertex are exactly what Sodium writes (and decode, with the {@code sodium_terrain} profile's
 * formulas, to the vertex Sodium was given), and the extension bytes follow in the layout the
 * profile reads.
 */
class ExtendedChunkVertexTest {
    /** Section-local position of the test block. */
    private static final int BX = 3;
    private static final int BY = 15;
    private static final int BZ = 0;
    private static final int MATERIAL = 0b101;
    private static final int SECTION = (3 << 5) | (6 << 2) | 2;

    private long extended;
    private long compact;

    @AfterEach
    void free() {
        BlockContext.current().exit();
        if (extended != 0) {
            MemoryUtil.nmemFree(extended);
        }
        if (compact != 0) {
            MemoryUtil.nmemFree(compact);
        }
    }

    /** The up face of the test block, textured with the atlas region (0.25, 0.75)-(0.5, 1.0). */
    private static ChunkVertexEncoder.Vertex[] upFace() {
        ChunkVertexEncoder.Vertex[] quad = ChunkVertexEncoder.Vertex.uninitializedQuad();
        float[][] corners = {{0, 1, 0}, {0, 1, 1}, {1, 1, 1}, {1, 1, 0}};
        float[][] uvs = {{0.25f, 0.75f}, {0.25f, 1.0f}, {0.5f, 1.0f}, {0.5f, 0.75f}};
        for (int i = 0; i < 4; i++) {
            ChunkVertexEncoder.Vertex.writeVertex(quad[i], BX + corners[i][0], BY + corners[i][1], BZ + corners[i][2], 0xFF336699, 0.5f, uvs[i][0],
                uvs[i][1], (13 * 16) | (15 * 16) << 16);
        }
        return quad;
    }

    private long[] encode(ChunkVertexEncoder.Vertex[] quad) {
        extended = MemoryUtil.nmemAlloc(4L * TerrainVertexLayout.VERTEX_SIZE);
        compact = MemoryUtil.nmemAlloc(4L * TerrainVertexLayout.SODIUM_VERTEX_SIZE);
        ChunkMeshFormats.COMPACT.getEncoder().write(compact, MATERIAL, quad, SECTION);
        long end = new ExtendedChunkVertex(ChunkMeshFormats.COMPACT, BlockIdTable.EMPTY).getEncoder().write(extended, MATERIAL, quad, SECTION);
        assertEquals(extended + 4L * TerrainVertexLayout.VERTEX_SIZE, end, "the encoder returns the end of the quad");
        return new long[] {extended, compact};
    }

    private static int intAt(long base, int vertex, int offset) {
        return MemoryUtil.memGetInt(base + (long) vertex * TerrainVertexLayout.VERTEX_SIZE + offset);
    }

    @Test
    void theFirst20BytesAreSodiums() {
        encode(upFace());
        for (int i = 0; i < 4; i++) {
            for (int b = 0; b < TerrainVertexLayout.SODIUM_VERTEX_SIZE; b++) {
                assertEquals(MemoryUtil.memGetByte(compact + i * 20L + b), MemoryUtil.memGetByte(extended + i * 36L + b), "vertex " + i + " byte " + b);
            }
        }
    }

    @Test
    void sodiumsBytesDecodeWithTheProfileFormulas() {
        ChunkVertexEncoder.Vertex[] quad = upFace();
        encode(quad);
        for (int i = 0; i < 4; i++) {
            int hi = intAt(extended, i, 0);
            int lo = intAt(extended, i, 4);
            float[] position = new float[3];
            for (int axis = 0; axis < 3; axis++) {
                // sb_sodiumDeinterleave(a_Position) * (32 / 2^20) - 8
                int value = ((hi >>> (axis * 10) & 0x3FF) << 10) | (lo >>> (axis * 10) & 0x3FF);
                position[axis] = value * (32.0f / 1048576.0f) - 8.0f;
            }
            assertEquals(quad[i].x, position[0], 1.0e-4f);
            assertEquals(quad[i].y, position[1], 1.0e-4f);
            assertEquals(quad[i].z, position[2], 1.0e-4f);
            int texCoord = intAt(extended, i, 12);
            for (int axis = 0; axis < 2; axis++) {
                int t = texCoord >>> (axis * 16) & 0xFFFF;
                // bias * u_TexCoordShrink + (a_TexCoord & 0x7FFF) / 32768, with shrink just under 1 / 32768.
                float shrink = 1.0f / 32768.0f;
                float uv = ((t >>> 15) != 0 ? 1.0f : -1.0f) * shrink + (t & 0x7FFF) / 32768.0f;
                assertEquals(axis == 0 ? quad[i].u : quad[i].v, uv, 1.5f / 32768.0f, "vertex " + i + " uv " + axis);
            }
            int lightAndData = intAt(extended, i, 16);
            // max(a_LightAndData.xy - 8, 0): 16 * light level.
            assertEquals(13 * 16, (lightAndData & 0xFF) - 8);
            assertEquals(15 * 16, (lightAndData >>> 8 & 0xFF) - 8);
            assertEquals(MATERIAL, lightAndData >>> 16 & 0xFF);
            assertEquals(SECTION, lightAndData >>> 24);
        }
    }

    @Test
    void theExtensionCarriesTheBlockBeingMeshed() {
        BlockContext.current().enter(TerrainExtension.entity(41, false), TerrainExtension.block(BX, BY, BZ, 7));
        encode(upFace());
        int expectedMidTexCoord = TerrainExtension.midTexCoord(0.375f, 0.875f);
        float[][] corners = {{0, 1, 0}, {0, 1, 1}, {1, 1, 1}, {1, 1, 0}};
        for (int i = 0; i < 4; i++) {
            assertEquals(84, intAt(extended, i, TerrainVertexLayout.ENTITY_OFFSET), "sb_Entity of block 41");
            assertEquals(TerrainExtension.UP, intAt(extended, i, TerrainVertexLayout.NORMAL_OFFSET), "sb_Normal of an up face");
            assertEquals(expectedMidTexCoord, intAt(extended, i, TerrainVertexLayout.MID_TEX_COORD_OFFSET));
            int midBlock = intAt(extended, i, TerrainVertexLayout.MID_BLOCK_OFFSET);
            assertEquals(corners[i][0] == 0 ? 32 : -32, (byte) midBlock, "x of vertex " + i);
            assertEquals(-32, (byte) (midBlock >> 8), "y of vertex " + i);
            assertEquals(corners[i][2] == 0 ? 32 : -32, (byte) (midBlock >> 16), "z of vertex " + i);
            assertEquals(7, midBlock >>> 24, "emission");
        }
    }

    @Test
    void geometryOfNoBlockHasNoBlockData() {
        encode(upFace());
        for (int i = 0; i < 4; i++) {
            assertEquals(0, intAt(extended, i, TerrainVertexLayout.ENTITY_OFFSET), "unmapped, not a fluid");
            assertEquals(TerrainExtension.UP, intAt(extended, i, TerrainVertexLayout.NORMAL_OFFSET), "the normal comes from the geometry");
            assertEquals(TerrainExtension.midTexCoord(0.375f, 0.875f), intAt(extended, i, TerrainVertexLayout.MID_TEX_COORD_OFFSET));
            assertEquals(0, intAt(extended, i, TerrainVertexLayout.MID_BLOCK_OFFSET));
        }
    }

    @Test
    void theTypeDescribesTheExtendedVertex() {
        ExtendedChunkVertex type = new ExtendedChunkVertex(ChunkMeshFormats.COMPACT, BlockIdTable.EMPTY);
        assertSame(TerrainVertexLayout.format(), type.getVertexFormat());
        assertSame(BlockIdTable.EMPTY, type.ids());
        assertTrue(type.getEncoder() instanceof ExtendedChunkVertex.Encoder);
    }
}
