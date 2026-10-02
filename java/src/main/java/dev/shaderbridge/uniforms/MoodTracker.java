package dev.shaderbridge.uniforms;

/**
 * Iris' {@code constantMood}: the cave mood of the vanilla ambient sound handler, accumulated the
 * same way but never reset when a mood sound plays. Ticked once per client tick with the light at
 * a random block near the player, as vanilla samples it.
 */
public final class MoodTracker {
    private float mood;

    /**
     * Folds in one sample.
     *
     * @param skyLight   sky light at the sampled block, 0..15
     * @param blockLight block light at the sampled block, 0..15
     * @param tickDelay  {@code AmbientMoodSettings.tickDelay()} of the biome
     */
    public void tick(int skyLight, int blockLight, int tickDelay) {
        if (skyLight > 0) {
            mood -= skyLight / 15.0f * 0.001f;
        } else {
            mood -= (float) (blockLight - 1) / tickDelay;
        }
        mood = Math.clamp(mood, 0.0f, 1.0f);
    }

    /** Forgets the accumulated mood (on world change). */
    public void reset() {
        mood = 0;
    }

    /** @return the current mood, 0..1 */
    public float value() {
        return mood;
    }
}
