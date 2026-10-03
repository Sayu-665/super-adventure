package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.pipeline.BlendFunction;
import com.mojang.renderpearl.api.pipeline.ColorTargetState;
import java.util.List;
import java.util.Optional;

/**
 * The color target states of a pack pipeline in one {@link AttachmentLayout}: per slot the
 * attachment format, whether the program writes it, and its blend function.
 *
 * @param slots    one entry per attachment slot
 * @param problems reasons the states cannot be expressed through Mojang's pipeline API on this
 *                 device; the program then needs the raw path
 * @param notes    harmless oddities worth a log line (e.g. an output whose type differs from its
 *                 target's, which is discarded)
 */
public record AttachmentPlan(List<Slot> slots, List<String> problems, List<String> notes) {
    public AttachmentPlan {
        slots = List.copyOf(slots);
        problems = List.copyOf(problems);
        notes = List.copyOf(notes);
    }

    /**
     * One attachment slot.
     *
     * @param target the colortex/shadowcolor index in the slot
     * @param format the attachment format
     * @param write  the program writes the slot (write mask all) or leaves it untouched (mask none)
     * @param blend  the blend function, empty for no blending
     */
    public record Slot(int target, GpuFormat format, boolean write, Optional<BlendFunction> blend) {
        /** @return Mojang's color target state for the slot */
        public ColorTargetState state() {
            return new ColorTargetState(blend, format, write ? ColorTargetState.WRITE_ALL : ColorTargetState.WRITE_NONE);
        }
    }

    /** @return whether the plan can be used as is */
    public boolean expressible() {
        return problems.isEmpty();
    }

    /** @return the color target state of every slot */
    public List<ColorTargetState> colorTargetStates() {
        return slots.stream().map(Slot::state).toList();
    }

    /** @return targets the program writes, in slot order */
    public List<Integer> writtenTargets() {
        return slots.stream().filter(Slot::write).map(Slot::target).toList();
    }
}
