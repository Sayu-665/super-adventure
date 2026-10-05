package dev.shaderbridge.render.shadow;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.joml.FrustumIntersection;
import org.joml.Matrix4f;
import org.junit.jupiter.api.Test;

/** {@link ShadowCulling}: the shadow pass's caster distance and frustum test; {@link ShadowSections.ShadowView#culls}. */
class ShadowCullingTest {
    @Test
    void shadowDistanceRenderMulLimitsCastersOnlyWhenPositive() {
        assertEquals(512, ShadowCulling.renderDistance(160, -1, 512), "the default draws casters as far as the world");
        assertEquals(512, ShadowCulling.renderDistance(160, 0, 512));
        assertEquals(160, ShadowCulling.renderDistance(160, 1, 512));
        assertEquals(240, ShadowCulling.renderDistance(120, 2, 512));
        assertEquals(256, ShadowCulling.renderDistance(160, 4, 256), "never beyond the render distance");
        assertEquals(256, ShadowCulling.renderDistance(160, Float.NaN, 256));
        assertEquals(256, ShadowCulling.renderDistance(0, 1, 256));
        assertEquals(256, ShadowCulling.renderDistance(160, Double.POSITIVE_INFINITY, 256));
    }

    @Test
    void theSectionRadiusRoundsUpAndStaysWithinTheViewArea() {
        assertEquals(10, ShadowCulling.sectionRadius(160, 32));
        assertEquals(11, ShadowCulling.sectionRadius(161, 32));
        assertEquals(12, ShadowCulling.sectionRadius(512, 12));
        assertEquals(0, ShadowCulling.sectionRadius(0, 12));
        assertEquals(0, ShadowCulling.sectionRadius(-5, 12));
        assertEquals(12, ShadowCulling.sectionRadius(Double.NaN, 12));
        assertEquals(0, ShadowCulling.sectionRadius(100, -1));
    }

    @Test
    void columnsFormACylinderAroundTheCamera() {
        assertTrue(ShadowCulling.columnInRange(0, 0, 0));
        assertTrue(ShadowCulling.columnInRange(1, 0, 0), "a neighbour's corner may be within reach");
        assertTrue(ShadowCulling.columnInRange(8, 0, 8));
        assertTrue(ShadowCulling.columnInRange(6, 6, 8));
        assertFalse(ShadowCulling.columnInRange(8, 8, 8), "the corners of the square are out of range");
        assertFalse(ShadowCulling.columnInRange(10, 0, 8));
        assertFalse(ShadowCulling.columnInRange(0, 0, -1));
        assertFalse(ShadowCulling.columnInRange(Integer.MAX_VALUE, Integer.MAX_VALUE, 4), "no overflow");
    }

    @Test
    void boxesOutsideTheShadowCameraAreCulled() {
        // A sun straight overhead: the shadow camera looks down -Y with an orthographic box of
        // +-32 blocks around the camera and 256 blocks deep each way.
        Matrix4f modelView = new Matrix4f().rotateX((float) Math.toRadians(90));
        Matrix4f projection = new Matrix4f().setOrtho(-32, 32, -32, 32, -256, 256);
        FrustumIntersection frustum = new FrustumIntersection(new Matrix4f(projection).mul(modelView));
        assertTrue(ShadowCulling.visible(frustum, -8, -8, -8, 8, 8, 8), "the camera's own section");
        assertTrue(ShadowCulling.visible(frustum, 0, 100, 0, 16, 116, 16), "far above the camera but inside the box");
        assertTrue(ShadowCulling.visible(frustum, 24, 0, 24, 40, 16, 40), "straddling the edge");
        assertFalse(ShadowCulling.visible(frustum, 48, 0, 0, 64, 16, 16), "beside the box");
        assertFalse(ShadowCulling.visible(frustum, 0, 300, 0, 16, 316, 16), "beyond the far plane");
        assertTrue(ShadowCulling.visible(null, 1e6, 1e6, 1e6, 1e6 + 16, 1e6 + 16, 1e6 + 16), "no frustum: distance culling only");
    }

    @Test
    void onlyDistanceCullingTurnsTheFrustumTestOff() {
        assertTrue(ShadowSections.ShadowView.culls("default"));
        assertTrue(ShadowSections.ShadowView.culls("advanced"));
        assertTrue(ShadowSections.ShadowView.culls("safe_zone"));
        assertTrue(ShadowSections.ShadowView.culls(null));
        assertFalse(ShadowSections.ShadowView.culls("distance"));
    }
}
