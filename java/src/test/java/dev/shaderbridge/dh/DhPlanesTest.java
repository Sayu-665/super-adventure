package dev.shaderbridge.dh;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

/**
 * {@link DhPlanes}: {@code dhFarPlane} as Iris reports it and {@code dhNearPlane} as Distant
 * Horizons' {@code RenderUtil.getNearClipPlaneInBlocks} computes it while a shader pack is in use
 * (reference values computed independently from the formulas in the Distant Horizons 3.3 source).
 */
class DhPlanesTest {
    private static final double SIXTEEN_NINTHS = 16.0 / 9.0;

    @Test
    void farPlaneIsTheLodDistancePlusARegionTimesSqrt2() {
        assertEquals(1086.1160159025371, DhPlanes.farPlane(16), 1e-3);
        assertEquals(6516.696095415223, DhPlanes.farPlane(256), 1e-3);
        assertEquals(512 * Math.sqrt(2), DhPlanes.farPlane(0), 1e-3);
    }

    @Test
    void automaticNearPlaneUsesTheShaderPackOverdrawOfOneFifth() {
        // 12 chunks * 16 * 0.2 = 38.4 blocks, moved to the frustum corner of a 70° FOV at 16:9.
        assertEquals(22.02445020020829, DhPlanes.nearPlane(12, SIXTEEN_NINTHS, -1, false, -1), 1e-4);
    }

    @Test
    void nearPlaneIsAtLeastOneBlockBeforeTheCornerCorrection() {
        // 0 chunks * 16 * 0.2 = 0 blocks, raised to one block.
        assertEquals(0.7105647754616298, DhPlanes.nearPlane(0, 1.0, -1, false, -1), 1e-6);
    }

    @Test
    void configuredOverdrawIsClampedLikeDistantHorizons() {
        assertEquals(5.506112550052073, DhPlanes.nearPlane(12, SIXTEEN_NINTHS, 0.0f, false, -1), 1e-4, "clamped up to 0.05");
        assertEquals(DhPlanes.nearPlane(12, SIXTEEN_NINTHS, 1.0f, false, -1), DhPlanes.nearPlane(12, SIXTEEN_NINTHS, 7.0f, false, -1), 1e-6,
            "clamped down to 1");
        assertEquals(DhPlanes.nearPlane(12, SIXTEEN_NINTHS, 0.5f, false, -1) * 2, DhPlanes.nearPlane(24, SIXTEEN_NINTHS, 0.5f, false, -1), 1e-4);
    }

    @Test
    void lodOnlyModeAndHeightOverride() {
        assertEquals(0.2867766953152121, DhPlanes.nearPlane(12, SIXTEEN_NINTHS, -1, true, -1), 1e-6);
        assertEquals(286.7766953152121, DhPlanes.nearPlane(12, SIXTEEN_NINTHS, -1, false, 500), 1e-3);
        // Distant Horizons compares the player's block Y with the level's height (not its top Y) plus 1000.
        assertEquals(-1, DhPlanes.heightOverride(1384, 384));
        assertEquals(116, DhPlanes.heightOverride(1500, 384));
    }

    @Test
    void degenerateAspectFallsBackToSquare() {
        assertEquals(DhPlanes.nearPlane(12, 1.0, -1, false, -1), DhPlanes.nearPlane(12, Double.NaN, -1, false, -1));
        assertEquals(DhPlanes.nearPlane(12, 1.0, -1, false, -1), DhPlanes.nearPlane(12, 0.0, -1, false, -1));
    }
}
