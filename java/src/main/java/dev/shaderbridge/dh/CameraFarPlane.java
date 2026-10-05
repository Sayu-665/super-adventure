package dev.shaderbridge.dh;

/**
 * The far plane of Minecraft's level projection while a pack renders with
 * {@link DhMode#SYNTHESIZED} LODs: vanilla terrain and LODs then share one projection whose far
 * plane is the Distant Horizons far plane (ARCHITECTURE §9). The frame orchestration
 * {@linkplain #request requests} the far plane for the next frame; the camera hook
 * ({@code CameraMixin}) applies it when Minecraft sets up the camera's projection and records
 * what it applied, so that a frame only draws unified LODs when the projection really reaches
 * them ({@link #extendedTo}). Render thread only.
 */
public final class CameraFarPlane {
    private static float requested = Float.NaN;
    private static float applied = Float.NaN;

    private CameraFarPlane() {
    }

    /**
     * @param far the far plane for the following frames, or {@link Float#NaN} for Minecraft's own
     */
    public static void request(float far) {
        requested = far;
    }

    /**
     * The camera hook: called with the far plane Minecraft is about to set up its projection with.
     *
     * @param vanilla Minecraft's far plane ({@code Camera.depthFar})
     * @return the far plane to use
     */
    public static float apply(float vanilla) {
        applied = requested;
        return Float.isNaN(requested) ? vanilla : requested;
    }

    /**
     * @param far a far plane
     * @return whether the camera's current projection was set up with exactly that far plane
     */
    public static boolean extendedTo(float far) {
        return applied == far;
    }
}
