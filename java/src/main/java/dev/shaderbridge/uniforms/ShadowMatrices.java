package dev.shaderbridge.uniforms;

import org.joml.Matrix4f;

/**
 * The shadow camera, computed as Iris and OptiFine do: a rotation that looks along the sun or
 * moon direction (tilted by {@code sunPathRotation}), snapped to a grid of {@code shadowIntervalSize}
 * blocks so that shadows do not shimmer, and an orthographic projection over {@code shadowDistance}.
 * All projections are GL style (NDC z in [-1,1]).
 */
public final class ShadowMatrices {
    /**
     * Iris' default {@code shadowNearPlane} (and the near plane of the legacy perspective
     * projection). Negative because the shadow model-view keeps the light at the camera instead
     * of moving it 100 blocks towards the sun, as older Iris versions and OptiFine did.
     */
    public static final float DEFAULT_NEAR = -100.05f;
    /** Iris' default {@code shadowFarPlane} (and the far plane of the legacy perspective projection). */
    public static final float DEFAULT_FAR = 156.0f;
    /** The OptiFine-documented defaults that {@code sb_core::model::ShadowSettings} carries. */
    private static final float MODEL_DEFAULT_NEAR = 0.05f;
    private static final float MODEL_DEFAULT_FAR = 256.0f;
    /** Plane value meaning "the Distant Horizons render distance" ({@code -1}). */
    private static final float DH_DISTANCE = -1.0f;

    private ShadowMatrices() {
    }

    /**
     * The near and far planes of the orthographic shadow projection.
     *
     * @param near near plane
     * @param far  far plane
     */
    public record Planes(float near, float far) {
    }

    /**
     * Resolves the pipeline's shadow planes as Iris 26.3 (and {@code sb-runtime}) do:
     * <ul>
     *   <li>the model's OptiFine defaults {@code 0.05}/{@code 256}, which assumed a light placed 100
     *       blocks away, become Iris' {@link #DEFAULT_NEAR}/{@link #DEFAULT_FAR} for the model-view of
     *       {@link #celestialModelView}, which covers the same depth range;</li>
     *   <li>{@code -1} means minus / plus {@code minusOneDistance} (see {@link #minusOneDistance});</li>
     *   <li>non-finite or coinciding planes fall back to the defaults instead of producing a
     *       singular matrix.</li>
     * </ul>
     *
     * @param near             {@code shadowNearPlane} of the pipeline
     * @param far              {@code shadowFarPlane} of the pipeline
     * @param minusOneDistance the distance a {@code -1} plane stands for
     * @return the planes to build the projection with
     */
    public static Planes planes(float near, float far, float minusOneDistance) {
        if (near == MODEL_DEFAULT_NEAR && far == MODEL_DEFAULT_FAR) {
            return new Planes(DEFAULT_NEAR, DEFAULT_FAR);
        }
        float n = near == DH_DISTANCE ? -minusOneDistance : near;
        float f = far == DH_DISTANCE ? minusOneDistance : far;
        if (!Float.isFinite(n) || !Float.isFinite(f) || n == f) {
            return new Planes(DEFAULT_NEAR, DEFAULT_FAR);
        }
        return new Planes(n, f);
    }

    /**
     * The distance a {@code -1} shadow plane stands for, as Iris 26.3 computes it
     * ({@code DHCompat.getRenderDistance() * 16}): the vanilla render distance in blocks without
     * Distant Horizons, but 16 times the DH distance in blocks while DH renders, because Iris'
     * DH render distance is already in blocks. Packs tuned on Iris see that range, and
     * {@code sb-runtime} reproduces it too.
     *
     * @param dhRendering            Distant Horizons renders
     * @param dhRenderDistanceBlocks the DH render distance in blocks
     * @param renderDistanceBlocks   the vanilla render distance in blocks
     * @return the distance in blocks
     */
    public static float minusOneDistance(boolean dhRendering, float dhRenderDistanceBlocks, float renderDistanceBlocks) {
        return dhRendering ? dhRenderDistanceBlocks * 16 : renderDistanceBlocks;
    }

    /**
     * @param halfPlaneLength {@code shadowDistance}: half the width and height of the covered area
     *                        (a non-positive or non-finite value covers one block)
     * @param planes          near and far plane, see {@link #planes}
     * @param dest            receives the matrix
     * @return {@code dest}
     */
    public static Matrix4f orthographic(float halfPlaneLength, Planes planes, Matrix4f dest) {
        float half = Float.isFinite(halfPlaneLength) && halfPlaneLength > 0 ? halfPlaneLength : 1.0f;
        return dest.setOrthoSymmetric(half * 2, half * 2, planes.near(), planes.far(), false);
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
        float near = DEFAULT_NEAR;
        float far = DEFAULT_FAR;
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
