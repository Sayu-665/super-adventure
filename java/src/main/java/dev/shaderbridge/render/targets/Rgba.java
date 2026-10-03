package dev.shaderbridge.render.targets;

import java.util.List;
import org.joml.Vector4f;

/**
 * A clear color.
 *
 * @param r red
 * @param g green
 * @param b blue
 * @param a alpha
 */
public record Rgba(float r, float g, float b, float a) {
    /** Opaque white, the clear color of shadow color targets and {@code colortex1}. */
    public static final Rgba WHITE = new Rgba(1, 1, 1, 1);
    /** Transparent black, the clear color of the other color targets. */
    public static final Rgba TRANSPARENT = new Rgba(0, 0, 0, 0);

    /**
     * @param components four components, as {@code ColorTarget.clearColor} stores them
     * @return the color
     * @throws IllegalArgumentException if there are not four components
     */
    public static Rgba of(List<Float> components) {
        if (components.size() != 4) {
            throw new IllegalArgumentException("a clear color has four components, got " + components.size());
        }
        return new Rgba(components.get(0), components.get(1), components.get(2), components.get(3));
    }

    /** @return the color as Mojang's clear APIs take it */
    public Vector4f toVector() {
        return new Vector4f(r, g, b, a);
    }
}
