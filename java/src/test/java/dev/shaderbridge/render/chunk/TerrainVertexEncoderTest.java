package dev.shaderbridge.render.chunk;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import org.junit.jupiter.api.Test;

class TerrainVertexEncoderTest {
    private static final float EPS = 1e-6f;
    private static final TerrainVertexEncoder.Layout LAYOUT = TerrainVertexFormat.LAYOUT;

    /** The six faces of a unit cube in Minecraft's vertex order ({@code FaceInfo}) with the default atlas mapping. */
    private enum Face {
        DOWN(new float[] {0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 1}, new float[] {0, -1, 0}, new float[] {1, 0, 0}),
        UP(new float[] {0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0}, new float[] {0, 1, 0}, new float[] {1, 0, 0}),
        NORTH(new float[] {1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0}, new float[] {0, 0, -1}, new float[] {-1, 0, 0}),
        SOUTH(new float[] {0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 1}, new float[] {0, 0, 1}, new float[] {1, 0, 0}),
        WEST(new float[] {0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1}, new float[] {-1, 0, 0}, new float[] {0, 0, 1}),
        EAST(new float[] {1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0}, new float[] {1, 0, 0}, new float[] {0, 0, -1});

        /** FaceBakery's default mapping: vertex 0 (u0, v0), 1 (u0, v1), 2 (u1, v1), 3 (u1, v0). */
        static final float[] UV = {0.25f, 0.5f, 0.25f, 0.5625f, 0.3125f, 0.5625f, 0.3125f, 0.5f};

        final float[] pos;
        final float[] normal;
        final float[] tangent;

        Face(float[] pos, float[] normal, float[] tangent) {
            this.pos = pos;
            this.normal = normal;
            this.tangent = tangent;
        }
    }

    @Test
    void cubeFacesGetOutwardNormalsAndVanillaTangents() {
        for (Face face : Face.values()) {
            float[] n = new float[3];
            TerrainVertexEncoder.faceNormal(face.pos, n);
            assertArrayEquals(face.normal, n, EPS, face.name());
            float[] t = new float[4];
            TerrainVertexEncoder.tangent(face.pos, Face.UV, n, t);
            assertArrayEquals(new float[] {face.tangent[0], face.tangent[1], face.tangent[2], 1f}, t, EPS, face.name());
            // The degenerate-mapping fallback agrees with the real mapping on every cube face.
            float[] axis = new float[4];
            TerrainVertexEncoder.axisTangent(n, axis);
            assertArrayEquals(t, axis, EPS, face.name() + " fallback");
        }
    }

    @Test
    void mirroredMappingFlipsTheHandedness() {
        // South face with u running right to left: the tangent points to -X and v still runs down,
        // so the bitangent is -cross(tangent, normal).
        float[] uv = {0.3125f, 0.5f, 0.3125f, 0.5625f, 0.25f, 0.5625f, 0.25f, 0.5f};
        float[] t = new float[4];
        TerrainVertexEncoder.tangent(Face.SOUTH.pos, uv, Face.SOUTH.normal, t);
        assertArrayEquals(new float[] {-1, 0, 0, -1}, t, EPS);
    }

    @Test
    void rotatedMappingFollowsTheAtlasU() {
        // Up face with its texture rotated by 90 degrees: u grows along +Z.
        float[] uv = {0.25f, 0.5625f, 0.3125f, 0.5625f, 0.3125f, 0.5f, 0.25f, 0.5f};
        float[] t = new float[4];
        TerrainVertexEncoder.tangent(Face.UP.pos, uv, Face.UP.normal, t);
        assertEquals(0f, t[0], EPS);
        assertEquals(0f, t[1], EPS);
        assertEquals(1f, t[2], EPS);
        assertEquals(1f, Math.abs(t[3]), EPS);
    }

    @Test
    void degenerateMappingsFallBack() {
        // First triangle maps no area (vertices 0 and 1 share their atlas coordinates): the second is used.
        float[] uv = {0.25f, 0.5f, 0.25f, 0.5f, 0.3125f, 0.5625f, 0.3125f, 0.5f};
        float[] t = new float[4];
        TerrainVertexEncoder.tangent(Face.SOUTH.pos, uv, Face.SOUTH.normal, t);
        assertArrayEquals(new float[] {1, 0, 0, 1}, t, EPS);
        // No area at all: the cube-face tangent.
        float[] flat = {0.25f, 0.5f, 0.25f, 0.5f, 0.25f, 0.5f, 0.25f, 0.5f};
        TerrainVertexEncoder.tangent(Face.WEST.pos, flat, Face.WEST.normal, t);
        assertArrayEquals(new float[] {0, 0, 1, 1}, t, EPS);
    }

    @Test
    void slopedAndDegenerateQuadsGetUsableNormals() {
        // A water surface sloping down towards +X (heights 0.875 and 0.5): the normal tilts towards +X.
        float[] slope = {0, 0.875f, 0, 0, 0.875f, 1, 1, 0.5f, 1, 1, 0.5f, 0};
        float[] n = new float[3];
        TerrainVertexEncoder.faceNormal(slope, n);
        float length = (float) Math.sqrt(0.375 * 0.375 + 1);
        assertArrayEquals(new float[] {0.375f / length, 1f / length, 0f}, n, EPS);
        float[] t = new float[4];
        TerrainVertexEncoder.tangent(slope, Face.UV, n, t);
        assertEquals(0f, t[0] * n[0] + t[1] * n[1] + t[2] * n[2], EPS, "the tangent lies in the surface");
        // A triangle stored as a quad (last vertex repeated): its first triangle's normal.
        float[] triangle = {0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1};
        TerrainVertexEncoder.faceNormal(triangle, n);
        assertArrayEquals(new float[] {0, 1, 0}, n, EPS);
        // A collapsed quad faces up.
        TerrainVertexEncoder.faceNormal(new float[12], n);
        assertArrayEquals(new float[] {0, 1, 0}, n, EPS);
    }

    @Test
    void scalarEncodings() {
        assertEquals(127, TerrainVertexEncoder.snorm8(1f));
        assertEquals(-127, TerrainVertexEncoder.snorm8(-1f));
        assertEquals(0, TerrainVertexEncoder.snorm8(0f));
        assertEquals(64, TerrainVertexEncoder.snorm8(0.5f), "rounded to the nearest step");
        assertEquals(127, TerrainVertexEncoder.snorm8(3f));
        assertEquals(0, TerrainVertexEncoder.snorm8(Float.NaN));
        // at_midBlock: (block centre - vertex) * 64.
        assertEquals(32, TerrainVertexEncoder.midBlock(3, 3f));
        assertEquals(-32, TerrainVertexEncoder.midBlock(3, 4f));
        assertEquals(0, TerrainVertexEncoder.midBlock(3, 3.5f));
        assertEquals(12, TerrainVertexEncoder.midBlock(0, 0.3125f), "offset plant: 0.1875 * 64");
        assertEquals(127, TerrainVertexEncoder.midBlock(0, -2f), "clamped");
        assertEquals(-128, TerrainVertexEncoder.midBlock(0, 3f), "clamped");
    }

    @Test
    void quadRecordsKeepTheBlock() {
        long q = TerrainVertexEncoder.quad(10008, TerrainVertexEncoder.RENDER_TYPE_FLUID, 15, 0, 7, 12);
        assertEquals(10008, TerrainVertexEncoder.blockId(q));
        assertEquals(1, TerrainVertexEncoder.renderType(q));
        assertEquals(-1, TerrainVertexEncoder.blockId(TerrainVertexEncoder.quad(-1, 0, 0, 0, 0, 0)));
        assertEquals((short) 40000, TerrainVertexEncoder.blockId(TerrainVertexEncoder.quad(40000, 0, 0, 0, 0, 0)),
            "ids beyond 16 bits wrap like Iris' short attribute");
        assertEquals(-1, TerrainVertexEncoder.blockId(TerrainVertexEncoder.NO_BLOCK));
        assertEquals(-1, TerrainVertexEncoder.renderType(TerrainVertexEncoder.NO_BLOCK));
    }

    /** Writes a quad's positions, a color, atlas coordinates and light, as Minecraft's builder does. */
    private static void putQuad(ByteBuffer buffer, int firstVertex, float[] pos, float[] uv, float dx, float dy, float dz) {
        for (int v = 0; v < 4; v++) {
            int at = (firstVertex + v) * LAYOUT.stride();
            buffer.putFloat(at, pos[3 * v] + dx);
            buffer.putFloat(at + 4, pos[3 * v + 1] + dy);
            buffer.putFloat(at + 8, pos[3 * v + 2] + dz);
            buffer.putInt(at + 12, 0x11223344);
            buffer.putFloat(at + 16, uv[2 * v]);
            buffer.putFloat(at + 20, uv[2 * v + 1]);
            buffer.putShort(at + 24, (short) 240);
            buffer.putShort(at + 26, (short) 80);
        }
    }

    @Test
    void encodeFillsEveryExtensionAttributeAndKeepsTheVanillaOnes() {
        int vertices = 9; // two quads and a stray vertex
        ByteBuffer buffer = ByteBuffer.allocate(vertices * LAYOUT.stride() + 52).order(ByteOrder.nativeOrder());
        buffer.position(52); // the encoder works from the buffer's position
        ByteBuffer mesh = buffer.slice().order(ByteOrder.nativeOrder());
        // Quad 0: the top of the block at (2, 5, 9) in its section. Quad 1: a water quad of block (0, 0, 15).
        putQuad(mesh, 0, Face.UP.pos, Face.UV, 2, 5, 9);
        putQuad(mesh, 4, Face.SOUTH.pos, Face.UV, 0, 0, 15);
        mesh.putFloat(8 * LAYOUT.stride() + 16, 0.75f);
        mesh.putFloat(8 * LAYOUT.stride() + 20, 0.125f);
        byte[] vanilla = new byte[28];
        mesh.get(4 * LAYOUT.stride(), vanilla);
        long[] quads = {
            TerrainVertexEncoder.quad(10001, TerrainVertexEncoder.RENDER_TYPE_BLOCK, 2, 5, 9, 15),
            TerrainVertexEncoder.quad(10008, TerrainVertexEncoder.RENDER_TYPE_FLUID, 0, 0, 15, 0),
        };
        TerrainVertexEncoder.encode(buffer, LAYOUT, vertices, quads);

        for (int v = 0; v < 4; v++) {
            int at = v * LAYOUT.stride();
            assertArrayEquals(new byte[] {0, 127, 0, 0}, bytes(mesh, at + 28), "normal of vertex " + v);
            assertEquals(10001, mesh.getShort(at + 32));
            assertEquals(0, mesh.getShort(at + 34));
            assertEquals(0.28125f, mesh.getFloat(at + 36), EPS);
            assertEquals(0.53125f, mesh.getFloat(at + 40), EPS);
            assertArrayEquals(new byte[] {127, 0, 0, 127}, bytes(mesh, at + 44), "tangent of vertex " + v);
            assertEquals(15, mesh.get(at + 51), "emission");
        }
        // Vertex 0 of the top face is the block's (min x, max y, min z) corner.
        assertArrayEquals(new byte[] {32, -32, 32, 15}, bytes(mesh, 48));
        // Vertex 2 is its (max x, max y, max z) corner.
        assertArrayEquals(new byte[] {-32, -32, -32, 15}, bytes(mesh, 2 * LAYOUT.stride() + 48));

        int water = 4 * LAYOUT.stride();
        assertArrayEquals(new byte[] {0, 0, 127, 0}, bytes(mesh, water + 28));
        assertEquals(10008, mesh.getShort(water + 32));
        assertEquals(1, mesh.getShort(water + 34));
        assertArrayEquals(new byte[] {32, -32, -32, 0}, bytes(mesh, water + 48));
        byte[] after = new byte[28];
        mesh.get(water, after);
        assertArrayEquals(vanilla, after, "the BLOCK elements are untouched");

        int stray = 8 * LAYOUT.stride();
        assertArrayEquals(new byte[] {0, 127, 0, 0}, bytes(mesh, stray + 28));
        assertEquals(-1, mesh.getShort(stray + 32));
        assertEquals(-1, mesh.getShort(stray + 34));
        assertEquals(0.75f, mesh.getFloat(stray + 36), EPS);
        assertEquals(0.125f, mesh.getFloat(stray + 40), EPS);
        assertArrayEquals(new byte[] {127, 0, 0, 127}, bytes(mesh, stray + 44));
        assertArrayEquals(new byte[] {0, 0, 0, 0}, bytes(mesh, stray + 48));
        assertEquals(52, buffer.position(), "the caller's buffer keeps its position");
    }

    @Test
    void quadsWithoutARecordCarryNoBlock() {
        ByteBuffer mesh = ByteBuffer.allocate(4 * LAYOUT.stride()).order(ByteOrder.nativeOrder());
        putQuad(mesh, 0, Face.EAST.pos, Face.UV, 4, 4, 4);
        TerrainVertexEncoder.encode(mesh, LAYOUT, 4, new long[0]);
        assertEquals(-1, mesh.getShort(32));
        assertEquals(-1, mesh.getShort(34));
        assertArrayEquals(new byte[] {0, 0, 0, 0}, bytes(mesh, 48));
        assertArrayEquals(new byte[] {127, 0, 0, 0}, bytes(mesh, 28));
        assertArrayEquals(new byte[] {0, 0, -127, 127}, bytes(mesh, 44));
    }

    private static byte[] bytes(ByteBuffer buffer, int at) {
        return new byte[] {buffer.get(at), buffer.get(at + 1), buffer.get(at + 2), buffer.get(at + 3)};
    }
}
