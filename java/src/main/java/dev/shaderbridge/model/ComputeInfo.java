package dev.shaderbridge.model;

import java.util.List;

/**
 * Dispatch information of a compute program.
 *
 * @param localSize  local work group size (x, y, z)
 * @param workGroups dispatch size
 * @param indirect   indirect dispatch source, or null
 */
public record ComputeInfo(List<Integer> localSize, WorkGroups workGroups, IndirectDispatch indirect) {
    public ComputeInfo {
        localSize = Copies.list(localSize);
        Copies.required(workGroups, "work_groups");
    }
}
