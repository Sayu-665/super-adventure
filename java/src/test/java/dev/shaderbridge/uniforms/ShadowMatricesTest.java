package dev.shaderbridge.uniforms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.joml.Matrix4f;
import org.joml.Vector3f;
import org.joml.Vector4f;
import org.junit.jupiter.api.Test;

class ShadowMatricesTest {
    private static final float EPS = 1e-5f;

    @Test
    void orthographicIsGlStyle() {
        // shadowDistance 32, near 0.05, far 256: m00 = 1/32, m22 = -2/(far-near), m32 = -(far+near)/(far-near).
        Matrix4f m = ShadowMatrices.orthographic(32, new ShadowMatrices.Planes(0.05f, 256), new Matrix4f());
        assertEquals(0.03125f, m.m00(), EPS);
        assertEquals(0.03125f, m.m11(), EPS);
        assertEquals(-2f / 255.95f, m.m22(), EPS);
        assertEquals(-256.05f / 255.95f, m.m32(), EPS);
        assertEquals(1f, m.m33(), EPS);
        assertEquals(0.00909090880304575f, ShadowMatrices.orthographic(110, new ShadowMatrices.Planes(0.05f, 256), new Matrix4f()).m00(), EPS);
        Vector4f near = m.transform(new Vector4f(0, 0, -0.05f, 1));
        assertEquals(-1f, near.z, EPS);
    }

    @Test
    void planesFollowIris26() {
        assertEquals(new ShadowMatrices.Planes(-100.05f, 156f), ShadowMatrices.planes(0.05f, 256f, 512),
            "the model's OptiFine defaults become Iris 26.3's defaults");
        assertEquals(new ShadowMatrices.Planes(-512f, 512f), ShadowMatrices.planes(-1, -1, 512), "-1 = the given distance");
        assertEquals(192f, ShadowMatrices.minusOneDistance(false, 192, 192), "without DH: the render distance in blocks");
        assertEquals(8192f * 16, ShadowMatrices.minusOneDistance(true, 8192, 192), "with DH: Iris scales the DH distance in blocks by 16 again");
        assertEquals(new ShadowMatrices.Planes(-10f, 512f), ShadowMatrices.planes(-10, -1, 512));
        assertEquals(new ShadowMatrices.Planes(1f, 300f), ShadowMatrices.planes(1, 300, 512), "explicit planes are kept");
        assertEquals(new ShadowMatrices.Planes(-100.05f, 156f), ShadowMatrices.planes(5, 5, 512), "coinciding planes");
        assertEquals(new ShadowMatrices.Planes(-100.05f, 156f), ShadowMatrices.planes(Float.NaN, 5, 512));
        Matrix4f degenerate = ShadowMatrices.orthographic(0, ShadowMatrices.planes(0, 0, 0), new Matrix4f());
        assertTrue(degenerate.isFinite() && degenerate.determinant() != 0, "never singular");
    }

    /**
     * Iris 26.3 dropped the old 100-block light offset from the shadow model-view and moved the
     * default planes instead; a caster 100 blocks towards the sun must still land in the shadow
     * map, at almost exactly the depth the old OptiFine matrices gave it.
     */
    @Test
    void defaultPlanesCoverCastersAboveThePlayer() {
        Matrix4f view = ShadowMatrices.celestialModelView(0.25f, 0, 0, 0, 0, 0, new Matrix4f());
        Matrix4f projection = ShadowMatrices.orthographic(160, ShadowMatrices.planes(0.05f, 256, 0), new Matrix4f());
        Matrix4f old = new Matrix4f().setOrthoSymmetric(320, 320, 0.05f, 256, false).mul(new Matrix4f().translation(0, 0, -100)).mul(view);
        Matrix4f current = new Matrix4f(projection).mul(view);
        for (float height : new float[] {-150, -20, 0, 30, 99}) {
            Vector4f a = current.transform(new Vector4f(3, height, -7, 1));
            Vector4f b = old.transform(new Vector4f(3, height, -7, 1));
            assertTrue(Math.abs(a.z) <= 1, "height " + height + " is inside the shadow map: " + a.z);
            assertEquals(b.z, a.z, 1e-3f, "same depth as the old convention at height " + height);
        }
    }

    /**
     * The dawn case of Iris' own ShadowMatrices self-test. Its expected values predate the 26.x
     * change (the translation still contains the old -100 on z); rotation and grid snapping agree.
     */
    @Test
    void modelViewAtDawnMatchesIris() {
        Matrix4f m = ShadowMatrices.celestialModelView(0.03451777f, 0.0f, 2.0f, 0.646045982837677, 82.53274536132812, -514.0264282226562, new Matrix4f());
        Matrix4f expected = new Matrix4f(
            0.21545040607452393f, 5.820481518981069E-8f, 0.9765146970748901f, 0,
            -0.9765147466795349f, 1.2841844920785661E-8f, 0.21545039117336273f, 0,
            0, -0.9999999403953552f, 5.960464477539063E-8f, 0,
            0.38002151250839233f, 1.0264281034469604f, -100.4463119506836f + 100.0f, 1);
        assertTrue(m.equals(expected, 5e-4f), m + "\n" + expected);
    }

    @Test
    void frameStateUsesTheResolvedPlanes() {
        FrameState frame = new FrameState();
        frame.update();
        Matrix4f expected = ShadowMatrices.orthographic(160, new ShadowMatrices.Planes(-100.05f, 156f), new Matrix4f());
        assertTrue(frame.shadowProjection().equals(expected, 1e-6f), frame.shadowProjection().toString());
    }

    @Test
    void noonLooksStraightDown() {
        // shadowAngle 0.25 (noon): only the fixed 90 degree tilt about X remains.
        Matrix4f m = ShadowMatrices.celestialModelView(0.25f, 0, 0, 0, 0, 0, new Matrix4f());
        Vector3f up = m.transformDirection(new Vector3f(0, 1, 0));
        assertEquals(0, up.x, EPS);
        assertEquals(0, up.y, EPS);
        assertEquals(1, up.z, EPS);
        Vector3f east = m.transformDirection(new Vector3f(1, 0, 0));
        assertEquals(1, east.x, EPS);
    }

    @Test
    void snapsToTheIntervalGrid() {
        // At (0, 0, 0) with interval 2 the offset is -1 on every axis, rotated by the 90 degree tilt.
        Matrix4f m = ShadowMatrices.celestialModelView(0.25f, 0, 2, 0, 0, 0, new Matrix4f());
        Vector3f t = m.getTranslation(new Vector3f());
        assertEquals(-1, t.x, EPS);
        assertEquals(1, t.y, EPS);
        assertEquals(-1, t.z, EPS);
        // Java's remainder keeps the sign: -3 % 2 = -1, so the offset is -2 on x.
        Matrix4f negative = ShadowMatrices.celestialModelView(0.25f, 0, 2, -3, 0.5, 5, new Matrix4f());
        Vector3f tn = negative.getTranslation(new Vector3f());
        assertEquals(-2, tn.x, EPS);
        assertEquals(0, tn.y, EPS);
        assertEquals(-0.5f, tn.z, EPS);
        assertTrue(ShadowMatrices.celestialModelView(0.25f, 0, 0, 123.4, 5, 6, new Matrix4f()).getTranslation(new Vector3f()).length() < EPS);
    }

    @Test
    void sunPathRotationTiltsTheLightDirection() {
        Matrix4f m = ShadowMatrices.celestialModelView(0.25f, 30, 0, 0, 0, 0, new Matrix4f());
        Vector3f toLight = m.invert(new Matrix4f()).transformDirection(new Vector3f(0, 0, 1));
        assertEquals(Math.cos(Math.toRadians(30)), toLight.y, EPS);
        assertEquals(30, Math.toDegrees(Math.acos(toLight.y)), 1e-3);
    }

    private static Vector3f towardsLight(Matrix4f modelView) {
        return modelView.invert(new Matrix4f()).transformDirection(new Vector3f(0, 0, 1));
    }

    @Test
    void theLightFollowsTheSunAcrossTheSky() {
        // Shadow angle 0 is sunrise (light from the east, +X), 0.5 sunset (west, -X).
        Vector3f sunrise = towardsLight(ShadowMatrices.celestialModelView(0.0f, 0, 0, 0, 0, 0, new Matrix4f()));
        assertEquals(1, sunrise.x, EPS);
        assertEquals(0, sunrise.y, EPS);
        Vector3f sunset = towardsLight(ShadowMatrices.celestialModelView(0.5f, 0, 0, 0, 0, 0, new Matrix4f()));
        assertEquals(-1, sunset.x, EPS);
        Vector3f morning = towardsLight(ShadowMatrices.celestialModelView(0.125f, 0, 0, 0, 0, 0, new Matrix4f()));
        assertEquals(Math.sqrt(0.5), morning.x, EPS);
        assertEquals(Math.sqrt(0.5), morning.y, EPS);
        Vector3f flash = towardsLight(ShadowMatrices.endFlashModelView(0, 0, 0, 0, 0, 0, new Matrix4f()));
        assertEquals(1, flash.z, EPS);
    }

    @Test
    void perspectiveForLegacyPacks() {
        Matrix4f m = ShadowMatrices.perspective(90, new Matrix4f());
        assertEquals(1, m.m00(), EPS);
        assertEquals(1, m.m11(), EPS);
        assertEquals(-1, m.m23(), EPS);
        assertEquals((156f - 100.05f) / (-100.05f - 156f), m.m22(), EPS);
    }
}
