package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.Collections;
import java.util.List;
import org.junit.jupiter.api.Test;

/** The cases of {@code sb-runtime/src/flips.rs}, plus the snapshots handed to the raw path. */
class FlipStateTest {
    @Test
    void compositePingPongLikeIris() {
        FlipState f = new FlipState();
        assertFalse(f.read(0));
        assertTrue(f.write(0));
        f.flip(List.of(0));
        assertTrue(f.read(0));
        assertFalse(f.write(0));
        f.flip(List.of(0, 1));
        assertFalse(f.read(0));
        assertTrue(f.read(1));
        // composite2 writes the alt of 2 without flipping it: later passes keep reading main.
        assertTrue(f.write(2));
        assertFalse(f.read(2));
        assertEquals(List.of(1), f.endOfFrameCopies(List.of(0, 1, 2)));
        f.reset();
        assertFalse(f.read(1));
    }

    @Test
    void modelFlipStateIsAuthoritative() {
        FlipState f = new FlipState();
        assertEquals(List.of(3), f.adopt(List.of(false, false, false, true, true), i -> i != 4));
        assertTrue(f.read(3));
        assertTrue(f.read(4));
        assertEquals(List.of(), f.adopt(List.of(false, false, false, true), i -> true));
        f.flip(List.of(999, -1));
        assertFalse(f.read(999));
        assertFalse(f.read(-1));
        assertTrue(f.adopt(Collections.nCopies(100, true), i -> true).size() <= 32);
    }

    @Test
    void shadowcolorPingPong() {
        FlipState f = new FlipState();
        assertFalse(f.shadowRead(0));
        assertTrue(f.shadowWrite(0));
        f.flipShadow(List.of(0, 1));
        f.flipShadow(List.of(1));
        assertTrue(f.shadowRead(0));
        assertFalse(f.shadowRead(1));
        assertEquals(List.of(0), f.shadowCopies(List.of(0, 1)));
        f.flipShadow(List.of(8));
        assertFalse(f.shadowRead(8));
    }

    @Test
    void snapshotsCoverEveryTarget() {
        FlipState f = new FlipState();
        f.flip(List.of(5));
        f.flipShadow(List.of(2));
        List<Boolean> color = f.colorState();
        assertEquals(32, color.size());
        assertTrue(color.get(5));
        assertEquals(1, color.stream().filter(b -> b).count());
        assertEquals(8, f.shadowState().size());
        assertTrue(f.shadowState().get(2));
    }
}
