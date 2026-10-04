package dev.shaderbridge.render.frame;

import dev.shaderbridge.model.ComputeInfo;
import dev.shaderbridge.model.WorkGroups;

/**
 * Work group counts of a compute dispatch, as Iris and the headless executor compute them:
 * {@code workGroups} as given, {@code workGroupsRender} (or no directive: 1, 1) as the rendered
 * extent scaled and divided by the local size, rounded up; every count clamped to the device
 * limit.
 */
public final class DispatchSize {
    private DispatchSize() {
    }

    /**
     * @param compute the program's compute information (null: cover the extent)
     * @param width   width of the extent the dispatch covers (screen, or shadow map for shadow computes)
     * @param height  height of that extent
     * @param max     the device's maximum work group counts {@code [x, y, z]}
     * @return the work group counts {@code [x, y, z]}; a zero count means nothing to dispatch
     */
    public static int[] of(ComputeInfo compute, int width, int height, int[] max) {
        int[] local = {1, 1, 1};
        if (compute != null) {
            for (int i = 0; i < Math.min(3, compute.localSize().size()); i++) {
                local[i] = compute.localSize().get(i);
            }
        }
        long[] size = switch (compute == null ? null : compute.workGroups()) {
            case WorkGroups.Absolute a -> new long[] {Integer.toUnsignedLong(a.x()), Integer.toUnsignedLong(a.y()), Integer.toUnsignedLong(a.z())};
            case WorkGroups.Relative r -> new long[] {relative(r.x(), width, local[0]), relative(r.y(), height, local[1]), 1};
            case null -> new long[] {ceilDiv(width, local[0]), ceilDiv(height, local[1]), 1};
        };
        return new int[] {clamp(size[0], max[0]), clamp(size[1], max[1]), clamp(size[2], max[2])};
    }

    private static long relative(float scale, int extent, int local) {
        double s = Float.isFinite(scale) ? Math.max(0, scale) : 0;
        double pixels = Math.ceil(extent * s);
        return (long) Math.min(Math.ceil(pixels / Math.max(1, local)), 0xFFFF_FFFFL);
    }

    private static long ceilDiv(int extent, int local) {
        return (Math.max(0, extent) + Math.max(1, local) - 1L) / Math.max(1, local);
    }

    private static int clamp(long value, int max) {
        return (int) Math.max(0, Math.min(value, max));
    }
}
