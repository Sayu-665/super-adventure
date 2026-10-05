package dev.shaderbridge.render.raw;

import dev.shaderbridge.model.ComputeInfo;
import dev.shaderbridge.model.IndirectDispatch;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;

/**
 * The device's compute limits ({@code VkPhysicalDeviceLimits.maxComputeWorkGroup*}) and the checks
 * a compute program and its dispatches must pass: a local size the device can run (a pipeline
 * with a larger one is invalid), group counts within the device limit, and an indirect dispatch
 * whose arguments lie inside its buffer.
 *
 * @param maxGroupCount  maximum work group count per dimension
 * @param maxGroupSize   maximum local size per dimension
 * @param maxInvocations maximum invocations (product of the local size) per work group
 */
public record ComputeLimits(List<Integer> maxGroupCount, List<Integer> maxGroupSize, int maxInvocations) {
    /** The limits every Vulkan device guarantees. */
    public static final ComputeLimits MINIMUM = new ComputeLimits(List.of(65535, 65535, 65535), List.of(128, 128, 64), 128);

    /** Bytes of the {@code VkDispatchIndirectCommand} an indirect dispatch reads. */
    public static final int INDIRECT_COMMAND_SIZE = 12;

    public ComputeLimits {
        maxGroupCount = List.copyOf(maxGroupCount);
        maxGroupSize = List.copyOf(maxGroupSize);
        if (maxGroupCount.size() != 3 || maxGroupSize.size() != 3) {
            throw new IllegalArgumentException("compute limits have three dimensions");
        }
    }

    /**
     * @param compute a compute program's compute information
     * @return why its local size cannot run on the device, empty if it can
     */
    public List<String> problems(ComputeInfo compute) {
        List<String> problems = new ArrayList<>();
        long invocations = 1;
        for (int i = 0; i < 3; i++) {
            long size = i < compute.localSize().size() ? Integer.toUnsignedLong(compute.localSize().get(i)) : 1;
            if (size < 1 || size > Integer.toUnsignedLong(maxGroupSize.get(i))) {
                problems.add("local size " + compute.localSize() + " exceeds the device's " + maxGroupSize);
            }
            invocations *= Math.max(1, size);
        }
        if (invocations > Integer.toUnsignedLong(maxInvocations)) {
            problems.add(invocations + " invocations per work group exceed the device's " + Integer.toUnsignedLong(maxInvocations));
        }
        return problems.stream().distinct().toList();
    }

    /** @return the maximum work group counts {@code [x, y, z]}, for {@code DispatchSize} */
    public int[] maxGroupCountArray() {
        return maxGroupCount.stream().mapToInt(c -> (int) Math.min(Integer.toUnsignedLong(c), Integer.MAX_VALUE)).toArray();
    }

    /**
     * @param indirect   an indirect dispatch ({@code indirect.<program>})
     * @param bufferSize the size of the storage buffer it reads, empty if the pack has no such buffer
     * @return why the dispatch cannot be made, empty if it can
     */
    public static Optional<String> indirectProblem(IndirectDispatch indirect, Optional<Long> bufferSize) {
        if (bufferSize.isEmpty()) {
            return Optional.of("its indirect dispatch reads storage buffer " + indirect.buffer() + ", which the pack does not declare");
        }
        long offset = Integer.toUnsignedLong(indirect.offset());
        if (offset % 4 != 0) {
            return Optional.of("its indirect dispatch offset " + offset + " is not a multiple of 4");
        }
        if (offset + INDIRECT_COMMAND_SIZE > bufferSize.get()) {
            return Optional.of("its indirect dispatch at offset " + offset + " lies outside storage buffer " + indirect.buffer() + " ("
                + bufferSize.get() + " bytes)");
        }
        return Optional.empty();
    }
}
