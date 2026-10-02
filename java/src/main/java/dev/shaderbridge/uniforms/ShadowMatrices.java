package dev.shaderbridge.uniforms;

import org.joml.Matrix4f;

/**
 * The shadow camera, computed as Iris and OptiFine do: a rotation that looks along the sun or
 * moon direction (tilted by {@code sunPathRotation}), snapped to a grid of {@code shadowIntervalSize}
 * blocks so that shadows do not shimmer, and an orthographic projection over {@code shadowDistance}.
 * All projections are GL style (NDC z in [-1,1]).
 */
public final class ShadowMatrices {
    /** Near plane of the legacy perspective shadow projection ({@code shadowMapFov}). */
    private static final float PERSPECTIVE_NEAR = -100.05f;
    /** Far plane of the legacy perspective shadow projection. */
    private static final float PERSPECTIVE_FAR = 156.0f;

    private ShadowMatrices() {
    }

    /**
     * @param halfPlaneLength {@code shadowDistance}: half the width and height of the covered area
     * @param near            near plane
     * @param far             far plane
     * @param dest            receives the matrix
     * @return {@code dest}
     */
    public static Matrix4f orthographic(float halfPlaneLength, float near, float far, Matrix4f dest) {
        return dest.setOrthoSymmetric(halfPlaneLength * 2, halfPlaneLength * 2, near, far, false);
    }

    /**
     * The legacy perspective shadow projection of packs that set {@code shadowMapFov}, with the
     * fixed planes OptiFine used.
     *
     * @param fovDegrees vertical field of view
     * @param dest       receives the matrix
     * @return {@code dest}
     */
    public static Matrix4f perspective(float fovDegrees, Matrix4f dest) {
        float yScale = (float) (1.0 / Math.tan(Math.toRadians(fovDegrees) * 0.5));
        float near = PERSPECTIVE_NEAR;
        float far = PERSPECTIVE_FAR;
        return dest.set(
            yScale, 0, 0, 0,
            0, yScale, 0, 0,
            0, 0, (far + near) / (near - far), -1,
            0, 0, 2 * far * near / (near - far), 1);
    }

    /**
     * The shadow model-view of the overworld sky.
     *
     * @param shadowAngle     {@code shadowAngle} uniform value (0..0.5 for the sun, 0.5..1 for the moon)
     * @param sunPathRotation tilt of the celestial path in degrees
     * @param intervalSize    grid size the camera snaps to, 0 disables snapping
     * @param cameraX         camera position (unshifted world coordinates)
     * @param cameraY         camera position
     * @param cameraZ         camera position
     * @param dest            receives the matrix
     * @return {@code dest}
     */
    public static Matrix4f celestialModelView(float shadowAngle, float sunPathRotation, float intervalSize,
                                              double cameraX, double cameraY, double cameraZ, Matrix4f dest) {
        float skyAngle = shadowAngle < 0.25f ? shadowAngle + 0.75f : shadowAngle - 0.25f;
        dest.identity()
            .rotateX((float) Math.toRadians(90.0))
            .rotateZ((float) Math.toRadians(skyAngle * -360.0f))
            .rotateX((float) Math.toRadians(sunPathRotation));
        return snapToGrid(dest, intervalSize, cameraX, cameraY, cameraZ);
    }

    /**
     * The shadow model-view of an End flash, for packs that cast shadows from it.
     *
     * @param xAngle       the flash's X angle in degrees
     * @param yAngle       the flash's Y angle in degrees
     * @param intervalSize grid size the camera snaps to, 0 disables snapping
     * @param cameraX      camera position (unshifted world coordinates)
     * @param cameraY      camera position
     * @param cameraZ      camera position
     * @param dest         receives the matrix
     * @return {@code dest}
     */
    public static Matrix4f endFlashModelView(float xAngle, float yAngle, float intervalSize,
                                             double cameraX, double cameraY, double cameraZ, Matrix4f dest) {
        dest.identity()
            .rotateX((float) Math.toRadians(-xAngle))
            .rotateY((float) Math.toRadians(yAngle));
        return snapToGrid(dest, intervalSize, cameraX, cameraY, cameraZ);
    }

    /**
     * Translates by the camera's offset within its grid cell, shifted by half a cell. Java's float
     * remainder keeps the sign of the dividend, so negative coordinates land in
     * {@code (-1.5 * interval, -0.5 * interval]}, exactly as in Iris.
     */
    private static Matrix4f snapToGrid(Matrix4f dest, float intervalSize, double cameraX, double cameraY, double cameraZ) {
        if (Math.abs(intervalSize) == 0.0f) {
            return dest;
        }
        float half = intervalSize / 2.0f;
        float offsetX = (float) cameraX % intervalSize - half;
        float offsetY = (float) cameraY % intervalSize - half;
        float offsetZ = (float) cameraZ % intervalSize - half;
        return dest.translate(offsetX, offsetY, offsetZ);
    }
}
