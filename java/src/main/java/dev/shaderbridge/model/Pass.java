package dev.shaderbridge.model;

import java.util.List;

/**
 * One step of the frame.
 *
 * @param group      pass group
 * @param index      index within the group (geometry passes use 0)
 * @param computes   compute programs dispatched first, in order
 * @param program    fullscreen program, or null
 * @param flipsAfter colortex buffers whose main/alt roles swap after this pass
 * @param flipState  flip state of every colortex when the pass starts (true = alt is read)
 */
public record Pass(PassGroup group, int index, List<Integer> computes, Integer program, List<Integer> flipsAfter, List<Boolean> flipState) {
    public Pass {
        Copies.required(group, "group");
        computes = Copies.list(computes);
        flipsAfter = Copies.list(flipsAfter);
        flipState = Copies.list(flipState);
    }
}
