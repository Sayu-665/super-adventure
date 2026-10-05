package dev.shaderbridge.compat.sodium;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

/**
 * {@link TerrainExtension}: the encodings of the extension attributes, checked against what the
 * {@code sodium_terrain} profile decodes ({@code mc_Entity = (int(e >> 1) - 1, e & 1, 0, 1)},
 * {@code normalize(sb_Normal.xyz)}, {@code sb_MidTexCoord / 32768}, {@code sb_MidBlock * 127}
 * from {@code RGBA8_SNORM}) and against Minecraft's face vertex orders.
 */
class TerrainExtensionTest {
    private static int x(int packed) {
        return (byte) packed;
    }

    private static int y(int packed) {
        return (byte) (packed >> 8);
    }

    private static int z(int packed) {
        return (byte) (packed >> 16);
    }

    private static int w(int packed) {
        return (byte) (packed >> 24);
    }

    private static int[] normal(float... positions) {
        int packed = TerrainExtension.normal(positions);
        return new int[] {x(packed), y(packed), z(packed), w(packed)};
    }

    @Test
    void entityDecodesLikeIrisMcEntity() {
        int mapped = TerrainExtension.entity(41, false);
        assertEquals(84, mapped);
        assertEquals(41, (mapped >>> 1) - 1);
        assertEquals(0, mapped & 1);
        int water = TerrainExtension.entity(8, true);
        assertEquals(8, (water >>> 1) - 1);
        assertEquals(1, water & 1);
        assertEquals(0, TerrainExtension.entity(-1, false), "unmapped blocks are id -1");
        assertEquals(1, TerrainExtension.entity(-1, true), "unmapped fluids keep the fluid flag");
        assertEquals(0, TerrainExtension.entity(-7, false), "ids below -1 read as unmapped");
    }

    @Test
    void normalsOfMinecraftFacesPointOutward() {
        // Vertex orders of Minecraft's FaceInfo for a unit cube (counter-clockwise seen from outside).
        assertArrayEquals(new int[] {0, 127, 0, 0}, normal(0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0));
        assertArrayEquals(new int[] {0, -127, 0, 0}, normal(0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 1));
        assertArrayEquals(new int[] {0, 0, -127, 0}, normal(1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0));
        assertArrayEquals(new int[] {0, 0, 127, 0}, normal(0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 1));
        assertArrayEquals(new int[] {-127, 0, 0, 0}, normal(0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1));
        assertArrayEquals(new int[] {127, 0, 0, 0}, normal(1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0));
    }

    @Test
    void normalsOfDiagonalAndFlippedQuads() {
        // A cross-plant quad along the (0,0,0)-(1,1,1) diagonal: normal (-1, 0, 1) / sqrt 2.
        assertArrayEquals(new int[] {-90, 0, 90, 0}, normal(0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1));
        // The same quad wound the other way (Sodium's flipped fluid faces) faces the other side.
        assertArrayEquals(new int[] {90, 0, -90, 0}, normal(1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0));
        // A sloped water surface keeps its tilt.
        int[] slope = normal(0, 0.875f, 0, 0, 0.875f, 1, 1, 0.75f, 1, 1, 0.75f, 0);
        assertEquals(0, slope[2]);
        assertEquals(16, slope[0]);
        assertEquals(126, slope[1]);
    }

    @Test
    void trianglesStoredAsQuadsKeepTheirNormal() {
        // Sodium stores triangles as quads with a repeated vertex (translucent sorting splits).
        assertArrayEquals(new int[] {0, 127, 0, 0}, normal(0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1));
        assertArrayEquals(new int[] {0, 127, 0, 0}, normal(0, 1, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1));
    }

    @Test
    void degenerateQuadsPointUp() {
        assertEquals(TerrainExtension.UP, TerrainExtension.normal(new float[12]));
        assertEquals(TerrainExtension.UP, TerrainExtension.normal(new float[] {0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3}));
        assertEquals(TerrainExtension.UP, TerrainExtension.normal(new float[] {Float.NaN, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0}));
        assertArrayEquals(new int[] {0, 127, 0, 0}, new int[] {x(TerrainExtension.UP), y(TerrainExtension.UP), z(TerrainExtension.UP), w(TerrainExtension.UP)});
    }

    @Test
    void midTexCoordIsTheCentreTimes32768() {
        int packed = TerrainExtension.midTexCoord(0.5f, 0.125f);
        assertEquals(16384, packed & 0xFFFF);
        assertEquals(4096, packed >>> 16);
        assertEquals(0xFFFF, TerrainExtension.midTexCoord(3.0f, -1.0f) & 0xFFFF, "clamped to 16 bits");
        assertEquals(0, TerrainExtension.midTexCoord(3.0f, -1.0f) >>> 16);
        assertEquals(1, TerrainExtension.midTexCoord(1.0f / 40000.0f, 0.0f), "rounded to nearest");
    }

    @Test
    void midBlockIsTheOffsetToTheBlockCentreTimes64() {
        int block = TerrainExtension.block(3, 15, 0, 14);
        // A vertex at the block's lower north-west corner: centre - vertex = (0.5, 0.5, 0.5).
        int corner = TerrainExtension.midBlock(block, 3, 15, 0);
        assertArrayEquals(new int[] {32, 32, 32, 14}, new int[] {x(corner), y(corner), z(corner), w(corner)});
        // The upper south-east corner.
        int other = TerrainExtension.midBlock(block, 4, 16, 1);
        assertArrayEquals(new int[] {-32, -32, -32, 14}, new int[] {x(other), y(other), z(other), w(other)});
        // Rounded to nearest, clamped to [-127, 127].
        int fraction = TerrainExtension.midBlock(block, 3.49f, 13.0f, -1.5f);
        assertArrayEquals(new int[] {1, 127, 127, 14}, new int[] {x(fraction), y(fraction), z(fraction), w(fraction)});
        int nearCentre = TerrainExtension.midBlock(block, 3.4999f, 15.5f, 0.5f);
        assertEquals(0, x(nearCentre));
        int far = TerrainExtension.midBlock(block, 10, 15.5f, 0.5f);
        assertEquals(-127, x(far));
        assertEquals(0, y(far));
    }

    @Test
    void quadsOfNoBlockHaveNoMidBlock() {
        assertEquals(0, TerrainExtension.midBlock(TerrainExtension.NO_BLOCK, 3, 4, 5));
        assertEquals(0, TerrainExtension.midBlock(TerrainExtension.block(0, 0, 0, 0), Float.NaN, Float.NaN, Float.NaN) & 0xFFFFFF);
    }

    @Test
    void blockReferencesKeepPositionAndEmission() {
        int block = TerrainExtension.block(15, 0, 9, 99);
        int mid = TerrainExtension.midBlock(block, 15.5f, 0.5f, 9.5f);
        assertEquals(0, mid & 0xFFFFFF, "the centre of block (15, 0, 9)");
        assertEquals(15, w(mid), "emission is clamped to 15");
        assertEquals(TerrainExtension.block(15, 0, 9, 0), TerrainExtension.block(31, 16, 25, 0), "positions are section-local");
    }

    private static void assertArrayEquals(int[] expected, int[] actual) {
        org.junit.jupiter.api.Assertions.assertArrayEquals(expected, actual, () -> java.util.Arrays.toString(actual));
    }
}
