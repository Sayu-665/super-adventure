package dev.shaderbridge.render.targets;

/**
 * Iris' default clear colors, used when a pack sets no {@code clear color} for a target:
 * {@code colortex0} clears to the fog color with alpha 1, {@code colortex1} to opaque white,
 * the other color targets to transparent black and shadow color targets to opaque white.
 */
public final class ClearColors {
    private ClearColors() {
    }

    /**
     * @param index  colortex or shadowcolor index
     * @param shadow the target is a shadow color target
     * @param fog    the current fog color (alpha ignored)
     * @return the default clear color
     */
    public static Rgba defaultClear(int index, boolean shadow, Rgba fog) {
        if (shadow) {
            return Rgba.WHITE;
        }
        return switch (index) {
            case 0 -> new Rgba(fog.r(), fog.g(), fog.b(), 1);
            case 1 -> Rgba.WHITE;
            default -> Rgba.TRANSPARENT;
        };
    }

    /**
     * @param spec a target
     * @param fog  the current fog color
     * @return the color the target is cleared to
     */
    public static Rgba of(TargetSpec spec, Rgba fog) {
        return spec.clearColor().orElseGet(() -> defaultClear(spec.index(), spec.shadow(), fog));
    }
}
