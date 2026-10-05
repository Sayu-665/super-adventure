package dev.shaderbridge.render.raw;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.ComputeInfo;
import dev.shaderbridge.model.IndirectDispatch;
import dev.shaderbridge.model.WorkGroups;
import dev.shaderbridge.render.frame.DispatchSize;
import java.util.List;
import java.util.Optional;
import org.junit.jupiter.api.Test;

class ComputeLimitsTest {
    private static ComputeInfo local(int x, int y, int z) {
        return new ComputeInfo(List.of(x, y, z), new WorkGroups.Absolute(1, 1, 1), null);
    }

    @Test
    void localSizesWithinTheDeviceLimitsRun() {
        ComputeLimits limits = ComputeLimits.MINIMUM;
        assertEquals(List.of(), limits.problems(local(8, 8, 1)));
        assertEquals(List.of(), limits.problems(local(128, 1, 1)));
        assertEquals(List.of(), limits.problems(local(4, 4, 4)));
        assertEquals(2, limits.problems(local(256, 1, 1)).size(), "x above 128, and 256 invocations");
        assertEquals(1, limits.problems(local(1, 1, 65)).size(), "z above 64");
        assertEquals(1, limits.problems(local(16, 16, 1)).size(), "256 invocations above 128");
        assertEquals(1, limits.problems(local(0, 1, 1)).size(), "an empty local size");
        assertEquals(List.of(), new ComputeLimits(List.of(1024, 1024, 64), List.of(1024, 1024, 64), 1024).problems(local(16, 16, 4)));
    }

    @Test
    void dispatchSizesAreClampedToTheDeviceLimitNotTheMinimum() {
        // RethinkingVoxels' shadowcomp3_a dispatches 65536 groups: one more than every device supports.
        ComputeInfo wide = new ComputeInfo(List.of(1, 1, 1), new WorkGroups.Absolute(65536, 1, 1), null);
        assertArrayEquals(new int[] {65535, 1, 1}, DispatchSize.of(wide, 1, 1, ComputeLimits.MINIMUM.maxGroupCountArray()));
        ComputeLimits desktop = new ComputeLimits(List.of(-1, 65535, 65535), List.of(1024, 1024, 64), 1024);
        assertArrayEquals(new int[] {Integer.MAX_VALUE, 65535, 65535}, desktop.maxGroupCountArray(), "0xFFFFFFFF is read unsigned");
        assertArrayEquals(new int[] {65536, 1, 1}, DispatchSize.of(wide, 1, 1, desktop.maxGroupCountArray()));
    }

    @Test
    void indirectDispatchesMustLieInTheirBuffer() {
        IndirectDispatch at16 = new IndirectDispatch(2, 16);
        assertEquals(Optional.empty(), ComputeLimits.indirectProblem(at16, Optional.of(28L)));
        assertTrue(ComputeLimits.indirectProblem(at16, Optional.of(27L)).isPresent(), "12 bytes from offset 16");
        assertTrue(ComputeLimits.indirectProblem(new IndirectDispatch(2, 6), Optional.of(64L)).isPresent(), "unaligned offset");
        assertTrue(ComputeLimits.indirectProblem(at16, Optional.empty()).isPresent(), "no such buffer");
    }
}
