package dev.shaderbridge.uniforms;

/**
 * {@code frameCounter}, {@code frameTime} and {@code frameTimeCounter} with Iris semantics: frame
 * times have millisecond resolution, the counter wraps after an hour and the frame counter after
 * 720720 frames.
 */
public final class FrameTimer {
    /** {@code frameCounter} wraps to 0 at this value (divisible by 1..16). */
    public static final int FRAME_COUNTER_PERIOD = 720720;
    /** {@code frameTimeCounter} wraps to 0 at this value, in seconds. */
    public static final float TIME_COUNTER_PERIOD = 3600.0f;

    private int frameCounter;
    private float frameTime;
    private float frameTimeCounter;
    private long lastStartNanos;
    private boolean started;

    /**
     * Starts a frame.
     *
     * @param startNanos monotonic time of the frame start ({@link System#nanoTime()})
     */
    public void beginFrame(long startNanos) {
        long elapsedMillis = started ? (startNanos - lastStartNanos) / 1_000_000L : 0L;
        frameTime = elapsedMillis / 1000.0f;
        frameTimeCounter += frameTime;
        if (frameTimeCounter >= TIME_COUNTER_PERIOD) {
            frameTimeCounter = 0.0f;
        }
        frameCounter = (frameCounter + 1) % FRAME_COUNTER_PERIOD;
        lastStartNanos = startNanos;
        started = true;
    }

    /** @return the {@code frameCounter} uniform */
    public int frameCounter() {
        return frameCounter;
    }

    /** @return the {@code frameTime} uniform: duration of the previous frame in seconds */
    public float frameTime() {
        return frameTime;
    }

    /** @return the {@code frameTimeCounter} uniform: seconds since start, wrapping hourly */
    public float frameTimeCounter() {
        return frameTimeCounter;
    }
}
