package dev.shaderbridge.render.frame;

import dev.shaderbridge.render.targets.TargetPlanner;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.function.IntPredicate;

/**
 * Main/alt ("flip") state of the {@code colortex} and {@code shadowcolor} ping-pong buffers over
 * one frame, as Iris' buffer flipper and the headless executor ({@code sb-runtime/src/flips.rs})
 * track it:
 *
 * <ul>
 *   <li>every frame starts with every buffer in its main texture;</li>
 *   <li>geometry reads and writes the current texture ({@link #read});</li>
 *   <li>composite-style programs read the current texture and write the other one
 *   ({@link #write}); the pass's {@code flips_after} then swap the roles;</li>
 *   <li>the model's per-pass {@code flip_state} is authoritative ({@link #adopt});</li>
 *   <li>at the end of the frame buffers left in their alt texture are copied back to main.</li>
 * </ul>
 *
 * <p>{@code shadowcolor} buffers ping-pong the same way in {@code shadowcomp} passes; the model has
 * no schedule for them, so the state follows the targets each shadowcomp program writes. Indices
 * outside {@code colortex0..31} / {@code shadowcolor0..7} read as main and ignore flips.
 */
public final class FlipState {
    private final boolean[] color = new boolean[TargetPlanner.MAX_COLOR_TEX];
    private final boolean[] shadow = new boolean[TargetPlanner.MAX_SHADOW_COLOR];

    /** Start of a frame: every buffer in main. */
    public void reset() {
        Arrays.fill(color, false);
        Arrays.fill(shadow, false);
    }

    /**
     * @param index a colortex index
     * @return whether colortex {@code index}'s current contents are in its alt texture (what
     *     programs read and geometry writes)
     */
    public boolean read(int index) {
        return index >= 0 && index < color.length && color[index];
    }

    /**
     * @param index a colortex index
     * @return whether a composite-style program writes colortex {@code index}'s alt texture
     */
    public boolean write(int index) {
        return !read(index);
    }

    /**
     * @param index a shadowcolor index
     * @return whether shadowcolor {@code index}'s current contents are in its alt texture
     */
    public boolean shadowRead(int index) {
        return index >= 0 && index < shadow.length && shadow[index];
    }

    /**
     * @param index a shadowcolor index
     * @return whether a shadowcomp program writes shadowcolor {@code index}'s alt texture
     */
    public boolean shadowWrite(int index) {
        return !shadowRead(index);
    }

    /**
     * Adopts the model's flip state at the start of a pass.
     *
     * @param passState the pass's {@code flip_state} (index = colortex)
     * @param known     which colortex targets exist (mismatches of other indices are not reported)
     * @return the existing targets whose tracked state disagreed with the model
     */
    public List<Integer> adopt(List<Boolean> passState, IntPredicate known) {
        List<Integer> mismatches = new ArrayList<>();
        for (int i = 0; i < passState.size() && i < color.length; i++) {
            boolean s = passState.get(i);
            if (color[i] != s && known.test(i)) {
                mismatches.add(i);
            }
            color[i] = s;
        }
        return mismatches;
    }

    /**
     * Swaps main and alt of colortex buffers (a pass's {@code flips_after}).
     *
     * @param indices colortex indices
     */
    public void flip(List<Integer> indices) {
        for (int i : indices) {
            if (i >= 0 && i < color.length) {
                color[i] = !color[i];
            }
        }
    }

    /**
     * Swaps main and alt of the shadowcolor buffers a shadowcomp program wrote.
     *
     * @param indices shadowcolor indices
     */
    public void flipShadow(List<Integer> indices) {
        for (int i : indices) {
            if (i >= 0 && i < shadow.length) {
                shadow[i] = !shadow[i];
            }
        }
    }

    /** @return per colortex index (0..31): its current contents are in its alt texture */
    public List<Boolean> colorState() {
        return states(color);
    }

    /** @return per shadowcolor index (0..7): its current contents are in its alt texture */
    public List<Boolean> shadowState() {
        return states(shadow);
    }

    private static List<Boolean> states(boolean[] flags) {
        List<Boolean> out = new ArrayList<>(flags.length);
        for (boolean f : flags) {
            out.add(f);
        }
        return out;
    }

    /**
     * @param copies the model's {@code end_of_frame_copies}
     * @return those currently in their alt texture: the alt to main copies due now
     */
    public List<Integer> endOfFrameCopies(List<Integer> copies) {
        return copies.stream().filter(this::read).toList();
    }

    /**
     * @param candidates shadowcolor buffers that are not cleared every frame
     * @return those that end the frame in their alt texture and must be copied back
     */
    public List<Integer> shadowCopies(List<Integer> candidates) {
        return candidates.stream().filter(this::shadowRead).toList();
    }
}
