package dev.shaderbridge.uniforms;

/**
 * Exponential smoothing with separate half-lives for rising and falling values, as OptiFine and
 * Iris smooth {@code wetness}, {@code eyeBrightnessSmooth} and {@code centerDepthSmooth}.
 * Half-lives are given in tenths of a second (the unit of {@code wetnessHalflife} and friends).
 */
public final class SmoothedFloat {
    private static final double LN_2 = Math.log(2.0);

    private final float decayUp;
    private final float decayDown;
    private float value;
    private boolean initialized;

    /**
     * @param halfLifeUp   half-life for rising values, in tenths of a second
     * @param halfLifeDown half-life for falling values, in tenths of a second
     */
    public SmoothedFloat(float halfLifeUp, float halfLifeDown) {
        this.decayUp = decay(halfLifeUp);
        this.decayDown = decay(halfLifeDown);
    }

    private static float decay(float halfLifeTenths) {
        float seconds = halfLifeTenths * 0.1f;
        return seconds <= 0 ? Float.POSITIVE_INFINITY : (float) (LN_2 / seconds);
    }

    /**
     * Folds a new raw value in. The first value is taken as is.
     *
     * @param raw          the unsmoothed value
     * @param deltaSeconds time since the previous update
     * @return the smoothed value
     */
    public float update(float raw, float deltaSeconds) {
        if (!initialized) {
            value = raw;
            initialized = true;
            return value;
        }
        float k = raw > value ? decayUp : decayDown;
        float alpha = Float.isInfinite(k) ? 1.0f : 1.0f - (float) Math.exp(-k * deltaSeconds);
        value = (1 - alpha) * value + alpha * raw;
        return value;
    }

    /** @return the current smoothed value (0 before the first update) */
    public float get() {
        return value;
    }
}
