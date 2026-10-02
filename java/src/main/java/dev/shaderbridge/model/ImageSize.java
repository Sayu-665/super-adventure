package dev.shaderbridge.model;

/** Size of a custom image ({@code #[serde(tag = "type")]}). */
public sealed interface ImageSize {
    /**
     * Relative to the screen size.
     *
     * @param x width factor
     * @param y height factor
     */
    record Relative(float x, float y) implements ImageSize {
    }

    /**
     * A 1D image (wire tag {@code absolute1_d}, serde's snake_case of {@code Absolute1D}).
     *
     * @param width width in texels
     */
    record Absolute1D(int width) implements ImageSize {
    }

    /**
     * A 2D image.
     *
     * @param width  width in texels
     * @param height height in texels
     */
    record Absolute2D(int width, int height) implements ImageSize {
    }

    /**
     * A 3D image.
     *
     * @param width  width in texels
     * @param height height in texels
     * @param depth  depth in texels
     */
    record Absolute3D(int width, int height, int depth) implements ImageSize {
    }
}
