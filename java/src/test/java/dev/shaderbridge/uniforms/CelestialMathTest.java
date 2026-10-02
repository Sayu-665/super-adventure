package dev.shaderbridge.uniforms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.joml.Matrix4f;
import org.joml.Vector3f;
import org.junit.jupiter.api.Test;

class CelestialMathTest {
    private static final float EPS = 1e-4f;

    @Test
    void anglesFollowIris() {
        assertEquals(0.25f, CelestialMath.sunAngle(0), EPS, "attribute 0 is noon");
        assertEquals(0.75f, CelestialMath.sunAngle(180), EPS);
        assertEquals(0.0f, CelestialMath.sunAngle(-90), EPS);
        assertEquals(355f / 360f, CelestialMath.sunAngle(-95), EPS, "negative angles wrap");
        assertTrue(CelestialMath.isDay(0));
        assertFalse(CelestialMath.isDay(180));
        assertEquals(0.25f, CelestialMath.shadowAngle(0, 180), EPS, "the sun casts shadows by day");
        assertEquals(0.25f, CelestialMath.shadowAngle(180, 0), EPS, "the moon by night");
    }

    @Test
    void positionsAreViewSpaceAtLength100() {
        Matrix4f identity = new Matrix4f();
        Vector3f noon = CelestialMath.celestialPosition(identity, 0, 0, new Vector3f());
        assertEquals(0, noon.x, EPS);
        assertEquals(100, noon.y, EPS);
        assertEquals(0, noon.z, EPS);
        Vector3f sunset = CelestialMath.celestialPosition(identity, 0, 90, new Vector3f());
        assertEquals(-100, sunset.x, EPS);
        assertEquals(0, sunset.y, EPS);
        Matrix4f view = new Matrix4f().rotateXYZ(0.3f, 1.2f, -0.4f);
        for (float angle = -180; angle <= 180; angle += 37) {
            assertEquals(100, CelestialMath.celestialPosition(view, 25, angle, new Vector3f()).length(), 1e-3f);
        }
        Vector3f tilted = CelestialMath.celestialPosition(identity, 30, 0, new Vector3f());
        assertEquals(100 * Math.cos(Math.toRadians(30)), tilted.y, 1e-3);
    }

    @Test
    void upPositionFollowsTheView() {
        assertEquals(new Vector3f(0, 100, 0), CelestialMath.upPosition(new Matrix4f(), new Vector3f()));
        Vector3f lookingUp = CelestialMath.upPosition(new Matrix4f().rotateX((float) Math.toRadians(90)), new Vector3f());
        assertEquals(100, lookingUp.z, EPS);
    }

    @Test
    void endFlashPositionHasLength100() {
        assertEquals(100, CelestialMath.endFlashPosition(new Matrix4f(), 30, 70, new Vector3f()).length(), EPS);
    }
}
