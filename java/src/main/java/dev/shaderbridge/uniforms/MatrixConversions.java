package dev.shaderbridge.uniforms;

import org.joml.Matrix4f;
import org.joml.Matrix4fc;

/**
 * Converts projection matrices between depth conventions by rewriting the row that produces clip
 * z. Shader packs always receive GL-style forward-Z matrices (NDC z in [-1,1], near maps to -1),
 * while Minecraft 26.2+ builds reversed-Z matrices (near maps to 1) whose NDC z range is [0,1] on
 * devices with zero-to-one clip control and [-1,1] otherwise.
 */
public final class MatrixConversions {
    private MatrixConversions() {
    }

    /**
     * Minecraft's reversed-Z projection to a GL forward-Z projection.
     * With [0,1] clip z: {@code z_gl = w - 2 z}; with [-1,1] clip z: {@code z_gl = -z}.
     *
     * @param reversed  a reversed-Z projection
     * @param zeroToOne the device clips z to [0,1]
     * @param dest      receives the result (may be {@code reversed})
     * @return {@code dest}
     */
    public static Matrix4f reversedToGl(Matrix4fc reversed, boolean zeroToOne, Matrix4f dest) {
        dest.set(reversed);
        if (zeroToOne) {
            return setZRow(dest, dest.m03() - 2 * dest.m02(), dest.m13() - 2 * dest.m12(), dest.m23() - 2 * dest.m22(), dest.m33() - 2 * dest.m32());
        }
        return setZRow(dest, -dest.m02(), -dest.m12(), -dest.m22(), -dest.m32());
    }

    /**
     * The inverse of {@link #reversedToGl}: a GL forward-Z projection to reversed-Z.
     * With [0,1] clip z: {@code z = (w - z_gl) / 2}; with [-1,1] clip z: {@code z = -z_gl}.
     *
     * @param gl        a GL forward-Z projection
     * @param zeroToOne the device clips z to [0,1]
     * @param dest      receives the result (may be {@code gl})
     * @return {@code dest}
     */
    public static Matrix4f glToReversed(Matrix4fc gl, boolean zeroToOne, Matrix4f dest) {
        dest.set(gl);
        if (zeroToOne) {
            return setZRow(dest, (dest.m03() - dest.m02()) / 2, (dest.m13() - dest.m12()) / 2, (dest.m23() - dest.m22()) / 2, (dest.m33() - dest.m32()) / 2);
        }
        return setZRow(dest, -dest.m02(), -dest.m12(), -dest.m22(), -dest.m32());
    }

    /**
     * A forward-Z [0,1] projection to GL [-1,1]: {@code z_gl = 2 z - w}.
     *
     * @param zeroToOne a projection with NDC z in [0,1], near at 0
     * @param dest      receives the result (may be the input)
     * @return {@code dest}
     */
    public static Matrix4f zeroToOneToGl(Matrix4fc zeroToOne, Matrix4f dest) {
        dest.set(zeroToOne);
        return setZRow(dest, 2 * dest.m02() - dest.m03(), 2 * dest.m12() - dest.m13(), 2 * dest.m22() - dest.m23(), 2 * dest.m32() - dest.m33());
    }

    /** Sets the clip-z row (element 2 of every column). */
    private static Matrix4f setZRow(Matrix4f m, float c0, float c1, float c2, float c3) {
        return m.m02(c0).m12(c1).m22(c2).m32(c3);
    }
}
