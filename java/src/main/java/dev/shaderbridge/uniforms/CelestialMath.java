package dev.shaderbridge.uniforms;

import org.joml.Matrix4f;
import org.joml.Matrix4fc;
import org.joml.Vector3f;
import org.joml.Vector4f;

/**
 * Sun, moon and shadow-light geometry from Minecraft's celestial angle attributes, as OptiFine and
 * Iris define the uniforms: positions are in view space with length 100.
 */
public final class CelestialMath {
    private CelestialMath() {
    }

    /**
     * Iris' "celestial angle": the attribute angle shifted by 90 degrees into [0, 360].
     *
     * @param attributeDegrees {@code SUN_ANGLE} or {@code MOON_ANGLE} attribute value
     * @return degrees, 0 = sunrise, 90 = noon, 180 = sunset
     */
    public static float celestialAngle(float attributeDegrees) {
        float c = attributeDegrees + 90.0f;
        if (c < 0) {
            c += 360;
        } else if (c > 360) {
            c -= 360;
        }
        return c;
    }

    /**
     * @param sunAttributeDegrees {@code SUN_ANGLE} attribute
     * @return true while the sun is above the horizon
     */
    public static boolean isDay(float sunAttributeDegrees) {
        return celestialAngle(sunAttributeDegrees) < 180;
    }

    /**
     * @param sunAttributeDegrees {@code SUN_ANGLE} attribute
     * @return the {@code sunAngle} uniform, 0..1 (0.25 = noon)
     */
    public static float sunAngle(float sunAttributeDegrees) {
        return celestialAngle(sunAttributeDegrees) / 360.0f;
    }

    /**
     * @param sunAttributeDegrees  {@code SUN_ANGLE} attribute
     * @param moonAttributeDegrees {@code MOON_ANGLE} attribute
     * @return the {@code shadowAngle} uniform: the angle of whichever body casts shadows
     */
    public static float shadowAngle(float sunAttributeDegrees, float moonAttributeDegrees) {
        return celestialAngle(isDay(sunAttributeDegrees) ? sunAttributeDegrees : moonAttributeDegrees) / 360.0f;
    }

    /**
     * Position of the sun or moon in view space: the sky rotation of the vanilla sky renderer
     * (90 degrees about -Y, {@code sunPathRotation} about Z, the body's angle about X) applied to
     * {@code (0, 100, 0)}.
     *
     * @param modelView        {@code gbufferModelView}
     * @param sunPathRotation  path tilt in degrees
     * @param attributeDegrees the body's {@code SUN_ANGLE}/{@code MOON_ANGLE} attribute
     * @param dest             receives the position
     * @return {@code dest}
     */
    public static Vector3f celestialPosition(Matrix4fc modelView, float sunPathRotation, float attributeDegrees, Vector3f dest) {
        Matrix4f m = new Matrix4f(modelView)
            .rotateY((float) Math.toRadians(-90.0))
            .rotateZ((float) Math.toRadians(sunPathRotation))
            .rotateX((float) Math.toRadians(attributeDegrees));
        return xyz(m.transform(new Vector4f(0, 100, 0, 1)), dest);
    }

    /**
     * @param modelView {@code gbufferModelView}
     * @param dest      receives the {@code upPosition} uniform
     * @return {@code dest}
     */
    public static Vector3f upPosition(Matrix4fc modelView, Vector3f dest) {
        Matrix4f m = new Matrix4f(modelView).rotateY((float) Math.toRadians(-90.0));
        return xyz(m.transform(new Vector4f(0, 100, 0, 0)), dest);
    }

    /**
     * @param modelView {@code gbufferModelView}
     * @param xAngle    End flash X angle in degrees
     * @param yAngle    End flash Y angle in degrees
     * @param dest      receives the {@code endFlashPosition} uniform
     * @return {@code dest}
     */
    public static Vector3f endFlashPosition(Matrix4fc modelView, float xAngle, float yAngle, Vector3f dest) {
        Matrix4f m = new Matrix4f(modelView)
            .rotateY((float) Math.toRadians(180.0f - yAngle))
            .rotateX((float) Math.toRadians(-90.0f - xAngle));
        return xyz(m.transform(new Vector4f(0, 100, 0, 0)), dest);
    }

    private static Vector3f xyz(Vector4f v, Vector3f dest) {
        return dest.set(v.x, v.y, v.z);
    }
}
