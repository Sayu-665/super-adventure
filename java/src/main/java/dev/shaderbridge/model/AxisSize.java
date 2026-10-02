package dev.shaderbridge.model;

/** One axis of a {@link TargetSize.PerAxis} size ({@code #[serde(tag = "type", content = "value")]}). */
public sealed interface AxisSize {
    /**
     * Fraction of the screen extent.
     *
     * @param factor the fraction
     */
    record Relative(float factor) implements AxisSize {
    }

    /**
     * Pixels.
     *
     * @param pixels the extent in pixels
     */
    record Absolute(int pixels) implements AxisSize {
    }
}
