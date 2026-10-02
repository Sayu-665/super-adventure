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
        Matrix4f m = ShadowMatrices.orthographic(32, 0.05f, 256, new Matrix4f());
        assertEquals(0.03125f, m.m00(), EPS);
        assertEquals(0.03125f, m.m11(), EPS);
        assertEquals(-2f / 255.95f, m.m22(), EPS);
        assertEquals(-256.05f / 255.95f, m.m32(), EPS);
        assertEquals(1f, m.m33(), EPS);
        assertEquals(0.00909090880304575f, ShadowMatrices.orthographic(110, 0.05f, 256, new Matrix4f()).m00(), EPS);
        Vector4f near = m.transform(new Vector4f(0, 0, -0.05f, 1));
        assertEquals(-1f, near.z, EPS);
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
