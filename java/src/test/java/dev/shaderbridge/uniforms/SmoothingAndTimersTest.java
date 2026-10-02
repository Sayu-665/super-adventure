package dev.shaderbridge.uniforms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.joml.Vector3d;
import org.junit.jupiter.api.Test;

class SmoothingAndTimersTest {
    private static final long MS = 1_000_000L;

    @Test
    void smoothingHalvesTheGapPerHalfLife() {
        SmoothedFloat s = new SmoothedFloat(10, 10); // 10 tenths = 1 second
        assertEquals(0, s.update(0, 0));
        assertEquals(0.5f, s.update(1, 1.0f), 1e-6f);
        assertEquals(0.75f, s.update(1, 1.0f), 1e-6f);
        assertEquals(0.875f, s.update(1, 1.0f), 1e-6f);
    }

    @Test
    void risingAndFallingUseTheirOwnHalfLives() {
        SmoothedFloat wetness = new SmoothedFloat(600, 200); // 60 s up, 20 s down
        wetness.update(0, 0);
        assertEquals(0.5f, wetness.update(1, 60), 1e-5f);
        assertEquals(0.25f, wetness.update(0, 20), 1e-5f);
        SmoothedFloat instant = new SmoothedFloat(0, 0);
        instant.update(0, 0);
        assertEquals(1, instant.update(1, 0.016f));
    }

    @Test
    void frameStateSmoothsWetnessAndEyeBrightness() {
        FrameState state = new FrameState();
        state.configure(new UniformSettings(0, 10, 10, 10, 10, 160, 0.05f, 256, 2, null, false, false, true));
        state.frameStartNanos = 0;
        state.eyeSkyLight = 0;
        state.update();
        assertEquals(0, state.wetness());
        state.frameStartNanos = 1000 * MS;
        state.rainStrength = 1;
        state.eyeSkyLight = 15;
        state.update();
        assertEquals(0.5f, state.wetness(), 1e-5f);
        assertEquals(120, state.eyeBrightnessSmoothY(), 1e-3f);
        state.frameStartNanos = 2000 * MS;
        state.update();
        assertEquals(0.75f, state.wetness(), 1e-5f);
        assertEquals(180, state.eyeBrightnessSmoothY(), 1e-3f);
    }

    @Test
    void frameTimerHasMillisecondResolutionAndWraps() {
        FrameTimer timer = new FrameTimer();
        timer.beginFrame(5 * MS);
        assertEquals(0, timer.frameTime());
        assertEquals(1, timer.frameCounter());
        timer.beginFrame(5 * MS + 16_700_000L);
        assertEquals(0.016f, timer.frameTime(), 1e-7f);
        assertEquals(0.016f, timer.frameTimeCounter(), 1e-7f);
        timer.beginFrame(5 * MS + 16_700_000L + 3_600_000 * MS);
        assertEquals(0, timer.frameTimeCounter(), "wraps after an hour");
        FrameTimer counter = new FrameTimer();
        for (int i = 0; i < FrameTimer.FRAME_COUNTER_PERIOD; i++) {
            counter.beginFrame(i);
        }
        assertEquals(0, counter.frameCounter());
    }

    @Test
    void cameraTrackerShiftsFarPositions() {
        CameraTracker tracker = new CameraTracker();
        tracker.update(new Vector3d(10, 64, 20));
        tracker.update(new Vector3d(11, 64, 21));
        assertEquals(new Vector3d(11, 64, 21), tracker.current());
        assertEquals(new Vector3d(10, 64, 20), tracker.previous());

        tracker.update(new Vector3d(30000.5, 64, 21));
        assertTrue(Math.abs(tracker.current().x()) < CameraTracker.WALK_RANGE, "shifted back into the window");
        assertEquals(0.5, tracker.current().x(), 1e-9);
        assertEquals(30000.5 - 11, tracker.current().x() - tracker.previous().x(), 1e-9, "the step survives the shift");
        assertEquals(new Vector3d(30000.5, 64, 21), tracker.currentUnshifted());
        assertEquals(new Vector3d(11, 64, 21), tracker.previousUnshifted());

        tracker.update(new Vector3d(30001.5, 64, 21));
        assertEquals(1.5, tracker.current().x(), 1e-9, "the shift persists");
        assertEquals(0.5, tracker.previous().x(), 1e-9);
    }

    @Test
    void moodAccumulatesInTheDarkWithoutResetting() {
        MoodTracker mood = new MoodTracker();
        for (int i = 0; i < 7000; i++) {
            mood.tick(0, 0, 6000);
        }
        assertEquals(1.0f, mood.value(), 1e-6f, "clamped at 1 instead of resetting");
        mood.tick(15, 0, 6000);
        assertEquals(0.999f, mood.value(), 1e-6f);
        mood.reset();
        mood.tick(0, 15, 6000);
        assertEquals(0, mood.value(), "bright block light never goes below 0");
    }
}
