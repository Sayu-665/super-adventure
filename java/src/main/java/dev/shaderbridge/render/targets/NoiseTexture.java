package dev.shaderbridge.render.targets;

import java.util.Random;

/**
 * The generated {@code noisetex}: white noise with opaque alpha, the same pixels Iris generates
 * (a {@link Random} seeded with 0 drawing one ARGB color per pixel, column by column), so packs
 * see the noise they were tuned with. Used unless the pack sets {@code texture.noise}.
 */
public final class NoiseTexture {
    /** Largest generated noise texture. */
    public static final int MAX_RESOLUTION = 4096;

    private NoiseTexture() {
    }

    /**
     * @param resolution {@code noiseTextureResolution} (clamped to [1, {@value #MAX_RESOLUTION}])
     * @return the ARGB pixels, row-major ({@code [y * size + x]})
     */
    public static int[] argb(int resolution) {
        int size = Math.clamp(resolution, 1, MAX_RESOLUTION);
        int[] pixels = new int[size * size];
        Random random = new Random(0);
        for (int x = 0; x < size; x++) {
            for (int y = 0; y < size; y++) {
                pixels[y * size + x] = random.nextInt() | 0xFF000000;
            }
        }
        return pixels;
    }
}
