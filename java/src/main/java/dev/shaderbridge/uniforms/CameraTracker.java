package dev.shaderbridge.uniforms;

import org.joml.Vector3d;
import org.joml.Vector3dc;

/**
 * Tracks {@code cameraPosition} and {@code previousCameraPosition} with Iris' precision shift:
 * when the camera leaves a +/-30000 block window on X or Z (or teleports more than 1000 blocks),
 * both positions are shifted by a multiple of 30000 so that float precision stays usable, while
 * their difference is preserved.
 */
public final class CameraTracker {
    /** Half-width of the window the shifted position stays in. */
    static final double WALK_RANGE = 30000;
    /** A jump larger than this re-centres immediately. */
    static final double TELEPORT_RANGE = 1000;

    private final Vector3d shift = new Vector3d();
    private final Vector3d current = new Vector3d();
    private final Vector3d previous = new Vector3d();
    private final Vector3d currentUnshifted = new Vector3d();
    private final Vector3d previousUnshifted = new Vector3d();

    /**
     * Advances one frame.
     *
     * @param camera the camera's world position this frame
     */
    public void update(Vector3dc camera) {
        previous.set(current);
        previousUnshifted.set(currentUnshifted);
        currentUnshifted.set(camera);
        current.set(camera).add(shift);
        double dx = shiftFor(current.x, previous.x);
        double dz = shiftFor(current.z, previous.z);
        if (dx != 0.0 || dz != 0.0) {
            shift.add(dx, 0, dz);
            current.add(dx, 0, dz);
            previous.add(dx, 0, dz);
        }
    }

    private static double shiftFor(double value, double previousValue) {
        if (Math.abs(value) > WALK_RANGE || Math.abs(value - previousValue) > TELEPORT_RANGE) {
            return -(value - value % WALK_RANGE);
        }
        return 0.0;
    }

    /** @return the shifted position ({@code cameraPosition}) */
    public Vector3dc current() {
        return current;
    }

    /** @return the shifted previous position ({@code previousCameraPosition}) */
    public Vector3dc previous() {
        return previous;
    }

    /** @return the real camera position */
    public Vector3dc currentUnshifted() {
        return currentUnshifted;
    }

    /** @return the real camera position of the previous frame */
    public Vector3dc previousUnshifted() {
        return previousUnshifted;
    }
}
