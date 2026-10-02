package dev.shaderbridge.uniforms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.joml.Matrix4f;
import org.joml.Vector4f;
import org.junit.jupiter.api.Test;

class MatrixConversionsTest {
    private static final float FOV = (float) Math.toRadians(70);
    private static final float ASPECT = 16f / 9f;
    private static final float NEAR = 0.05f;
    private static final float FAR = 512f;

    /** Minecraft 26.2+ builds its projection with near and far swapped. */
    private static Matrix4f minecraftProjection(boolean zeroToOne) {
        return new Matrix4f().setPerspective(FOV, ASPECT, FAR, NEAR, zeroToOne);
    }

    private static float ndcZ(Matrix4f projection, float viewDistance) {
        Vector4f clip = projection.transform(new Vector4f(0, 0, -viewDistance, 1));
        return clip.z / clip.w;
    }

    @Test
    void reversedZeroToOneBecomesGlForwardZ() {
        Matrix4f reversed = minecraftProjection(true);
        assertEquals(1.0f, ndcZ(reversed, NEAR), 1e-4f, "Minecraft maps the near plane to 1");
        Matrix4f gl = MatrixConversions.reversedToGl(reversed, true, new Matrix4f());
        assertTrue(gl.equals(new Matrix4f().setPerspective(FOV, ASPECT, NEAR, FAR, false), 1e-4f), gl.toString());
        assertEquals(-1.0f, ndcZ(gl, NEAR), 1e-4f);
        assertEquals(1.0f, ndcZ(gl, FAR), 1e-3f);
    }

    @Test
    void reversedNegOneToOneBecomesGlForwardZ() {
        Matrix4f reversed = minecraftProjection(false);
        Matrix4f gl = MatrixConversions.reversedToGl(reversed, false, new Matrix4f());
        assertTrue(gl.equals(new Matrix4f().setPerspective(FOV, ASPECT, NEAR, FAR, false), 1e-4f), gl.toString());
    }

    @Test
    void roundTrips() {
        for (boolean zeroToOne : new boolean[] {true, false}) {
            Matrix4f reversed = minecraftProjection(zeroToOne);
            Matrix4f back = MatrixConversions.glToReversed(MatrixConversions.reversedToGl(reversed, zeroToOne, new Matrix4f()), zeroToOne, new Matrix4f());
            assertTrue(back.equals(reversed, 1e-5f), "round trip with zeroToOne=" + zeroToOne);
        }
        Matrix4f ortho = new Matrix4f().setOrtho(-4, 4, -3, 3, FAR, NEAR, true);
        Matrix4f back = MatrixConversions.glToReversed(MatrixConversions.reversedToGl(ortho, true, new Matrix4f()), true, new Matrix4f());
        assertTrue(back.equals(ortho, 1e-6f));
    }

    @Test
    void forwardZeroToOneBecomesGl() {
        Matrix4f forward01 = new Matrix4f().setPerspective(FOV, ASPECT, NEAR, FAR, true);
        Matrix4f gl = MatrixConversions.zeroToOneToGl(forward01, new Matrix4f());
        assertTrue(gl.equals(new Matrix4f().setPerspective(FOV, ASPECT, NEAR, FAR, false), 1e-4f), gl.toString());
    }

    @Test
    void conversionsLeaveXyAndWAlone() {
        Matrix4f reversed = minecraftProjection(true);
        Matrix4f gl = MatrixConversions.reversedToGl(reversed, true, new Matrix4f());
        Vector4f p = new Vector4f(1.5f, -2, -7, 1);
        Vector4f a = reversed.transform(new Vector4f(p));
        Vector4f b = gl.transform(new Vector4f(p));
        assertEquals(a.x, b.x, 1e-6f);
        assertEquals(a.y, b.y, 1e-6f);
        assertEquals(a.w, b.w, 1e-6f);
    }
}
