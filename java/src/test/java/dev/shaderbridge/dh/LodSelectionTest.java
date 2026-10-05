package dev.shaderbridge.dh;

import static org.junit.jupiter.api.Assertions.assertEquals;

import java.util.List;
import org.joml.Matrix4f;
import org.junit.jupiter.api.Test;

/** {@link LodSelection}: per-pass culling of LOD sections and the vanilla-area restriction of synthesized LODs. */
class LodSelectionTest {
    private static final double CAMERA_X = 1000.5;
    private static final double CAMERA_Y = 70;
    private static final double CAMERA_Z = -2000.25;
    /** GL projection looking down -Z (Minecraft's view rotation for yaw 180 is the identity). */
    private static final Matrix4f CLIP = new Matrix4f().perspective((float) Math.toRadians(70), 1.0f, 0.05f, 5000f);

    private static LodBuffer section(int minX, int minZ, int width) {
        return new LodBuffer(minX, -64, minZ, width, null, null, 6);
    }

    private static final LodBuffer AHEAD = section(968, -2512, 64);
    private static final LodBuffer BEHIND = section(968, -1744, 64);
    private static final LodBuffer AROUND_CAMERA = section(992, -2016, 16);
    private static final LodBuffer ACROSS_THE_EDGE = section(1128, -2032, 64);

    @Test
    void sectionsOutsideThePassViewAreCulled() {
        LodSelection selection = new LodSelection(CLIP, CAMERA_X, CAMERA_Y, CAMERA_Z, -64, 320, 0);
        assertEquals(List.of(AHEAD, AROUND_CAMERA), selection.select(List.of(AHEAD, BEHIND, AROUND_CAMERA)));
    }

    @Test
    void withoutAMatrixEverySectionIsDrawn() {
        LodSelection selection = new LodSelection(null, CAMERA_X, CAMERA_Y, CAMERA_Z, -64, 320, 0);
        assertEquals(List.of(AHEAD, BEHIND, AROUND_CAMERA), selection.select(List.of(AHEAD, BEHIND, AROUND_CAMERA)));
    }

    @Test
    void sectionsCompletelyInsideTheVanillaAreaAreSkipped() {
        LodSelection selection = new LodSelection(null, CAMERA_X, CAMERA_Y, CAMERA_Z, -64, 320, LodSelection.vanillaRadius(11));
        assertEquals(List.of(AHEAD, BEHIND, ACROSS_THE_EDGE), selection.select(List.of(AHEAD, BEHIND, AROUND_CAMERA, ACROSS_THE_EDGE)),
            "a section reaching beyond the vanilla area is kept (it overlaps vanilla terrain at the edge rather than leaving a hole)");
    }

    @Test
    void sectionsAboveOrBelowTheCameraAreTestedOverTheWholeLevelHeight() {
        // The camera far above the world: looking horizontally the sections are below the frustum, looking down they are in it.
        LodSelection selection = new LodSelection(CLIP, CAMERA_X, 3000, CAMERA_Z, -64, 320, 0);
        assertEquals(List.of(), selection.select(List.of(AHEAD)), "the level's whole height is below the horizontal frustum");
        Matrix4f down = new Matrix4f(CLIP).rotateX((float) Math.toRadians(80));
        assertEquals(List.of(AHEAD), new LodSelection(down, CAMERA_X, 3000, CAMERA_Z, -64, 320, 0).select(List.of(AHEAD)));
    }

    @Test
    void vanillaRadiusLeavesOneChunkOfMargin() {
        assertEquals(176.0, LodSelection.vanillaRadius(12));
        assertEquals(0.0, LodSelection.vanillaRadius(1));
        assertEquals(0.0, LodSelection.vanillaRadius(0));
    }
}
