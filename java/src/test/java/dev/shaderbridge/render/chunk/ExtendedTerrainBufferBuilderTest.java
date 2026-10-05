package dev.shaderbridge.render.chunk;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import com.mojang.blaze3d.vertex.BufferBuilder;
import com.mojang.blaze3d.vertex.ByteBufferBuilder;
import com.mojang.blaze3d.vertex.DefaultVertexFormat;
import com.mojang.blaze3d.vertex.MeshData;
import com.mojang.blaze3d.vertex.VertexConsumer;
import com.mojang.renderpearl.api.pipeline.PrimitiveTopology;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import org.junit.jupiter.api.Test;

/**
 * Meshes the same quads with Minecraft's {@code BLOCK} builder and with the extended builder, as
 * the section compiler does (block quads through the 11-argument {@code addVertex}, fluids through
 * the same call, other emitters through the element-wise API), and compares the vertex memory.
 * Needs LWJGL's native memory allocator; skipped where it cannot load.
 */
class ExtendedTerrainBufferBuilderTest {
    private static ByteBufferBuilder memory() {
        try {
            return new ByteBufferBuilder(1024);
        } catch (LinkageError | RuntimeException e) {
            assumeTrue(false, "LWJGL memory is not available: " + e);
            throw new AssertionError(e);
        }
    }

    /** Two quads: an up face written vertex by vertex, and a south face written element by element. */
    private static void mesh(VertexConsumer builder) {
        float[][] up = {{2, 6, 9}, {2, 6, 10}, {3, 6, 10}, {3, 6, 9}};
        float[][] uv = {{0.25f, 0.5f}, {0.25f, 0.5625f}, {0.3125f, 0.5625f}, {0.3125f, 0.5f}};
        for (int v = 0; v < 4; v++) {
            builder.addVertex(up[v][0], up[v][1], up[v][2], 0xFF80C040 + v, uv[v][0], uv[v][1], 0, 0x00F000A0, 0f, 1f, 0f);
        }
        float[][] south = {{0, 1, 16}, {0, 0, 16}, {1, 0, 16}, {1, 1, 16}};
        for (int v = 0; v < 4; v++) {
            builder.addVertex(south[v][0], south[v][1], south[v][2]).setColor(255, 128, 64, 200).setUv(uv[v][0], uv[v][1]).setLight(0x00500030);
        }
    }

    @Test
    void vanillaElementsMatchMinecraftsBlockBuilderAndExtensionsAreFilled() {
        ByteBufferBuilder vanillaMemory = memory();
        ByteBufferBuilder extendedMemory = memory();
        try {
            BufferBuilder vanilla = new BufferBuilder(vanillaMemory, PrimitiveTopology.QUADS, DefaultVertexFormat.BLOCK);
            BufferBuilder extended = new ExtendedTerrainBufferBuilder(extendedMemory, PrimitiveTopology.QUADS, TerrainVertexFormat.EXTENDED,
                new TerrainVertexContext(), BlockIdTable.EMPTY);
            mesh(vanilla);
            mesh(extended);
            try (MeshData a = vanilla.build(); MeshData b = extended.build()) {
                assertNotNull(a);
                assertNotNull(b);
                assertEquals(8, b.drawState().vertexCount());
                assertEquals(TerrainVertexFormat.EXTENDED, b.drawState().format());
                ByteBuffer va = a.vertexBuffer().order(ByteOrder.nativeOrder());
                ByteBuffer vb = b.vertexBuffer().order(ByteOrder.nativeOrder());
                assertEquals(8 * 28, va.remaining());
                assertEquals(8 * 52, vb.remaining());
                for (int v = 0; v < 8; v++) {
                    byte[] block = new byte[28];
                    byte[] ext = new byte[28];
                    va.get(v * 28, block);
                    vb.get(v * 52, ext);
                    assertArrayEquals(block, ext, "BLOCK elements of vertex " + v);
                }
                // Up face: normal +Y, tangent +X, no block (the context names none).
                assertArrayEquals(new byte[] {0, 127, 0, 0}, new byte[] {vb.get(28), vb.get(29), vb.get(30), vb.get(31)});
                assertEquals(-1, vb.getShort(32));
                assertEquals(0.28125f, vb.getFloat(36), 1e-6f);
                assertArrayEquals(new byte[] {127, 0, 0, 127}, new byte[] {vb.get(44), vb.get(45), vb.get(46), vb.get(47)});
                // South face: normal +Z.
                int south = 4 * 52;
                assertArrayEquals(new byte[] {0, 0, 127, 0}, new byte[] {vb.get(south + 28), vb.get(south + 29), vb.get(south + 30), vb.get(south + 31)});
            }
        } finally {
            vanillaMemory.close();
            extendedMemory.close();
        }
    }
}
