package dev.shaderbridge.model;

/** Size of a render target ({@code #[serde(tag = "type")]}). */
public sealed interface TargetSize {
    /**
     * Resolves the size to pixels, at least 1x1.
     *
     * @param width  screen width in pixels
     * @param height screen height in pixels
     * @return {@code [width, height]}
     */
    int[] resolve(int width, int height);

    /**
     * Truncates a relative extent (at least 1), as the Rust side and Iris do
     * ({@code (int) (extent * factor)}).
     */
    private static int relative(float factor, int extent) {
        return Math.max(1, (int) (extent * factor));
    }

    /**
     * Relative to the screen size.
     *
     * @param x width factor
     * @param y height factor
     */
    record Relative(float x, float y) implements TargetSize {
        @Override
        public int[] resolve(int width, int height) {
            return new int[] {relative(x, width), relative(y, height)};
        }
    }

    /**
     * Absolute pixels.
     *
     * @param width  width in pixels
     * @param height height in pixels
     */
    record Absolute(int width, int height) implements TargetSize {
        @Override
        public int[] resolve(int screenWidth, int screenHeight) {
            return new int[] {Math.max(1, width), Math.max(1, height)};
        }
    }

    /**
     * Mixed per-axis sizes.
     *
     * @param x horizontal size
     * @param y vertical size
     */
    record PerAxis(AxisSize x, AxisSize y) implements TargetSize {
        @Override
        public int[] resolve(int width, int height) {
            return new int[] {axis(x, width), axis(y, height)};
        }

        private static int axis(AxisSize size, int extent) {
            return switch (size) {
                case AxisSize.Relative r -> relative(r.factor(), extent);
                case AxisSize.Absolute a -> Math.max(1, a.pixels());
            };
        }
    }
}
