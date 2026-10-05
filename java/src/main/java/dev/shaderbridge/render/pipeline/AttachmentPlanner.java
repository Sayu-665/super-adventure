package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.pipeline.BlendFunction;
import com.mojang.renderpearl.api.pipeline.ColorTargetState;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.render.pipeline.AttachmentLayout.Attachment;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;

/**
 * Builds a program's {@link AttachmentPlan}, with the rules of the headless executor
 * ({@code sb-runtime}): logical output {@code i} goes to slot {@code output_slots[i]} of a shared
 * layout (slot {@code i} otherwise); a slot is written when the program maps an output to it and
 * the fragment shader declares that location with the target's numeric class; every other slot is
 * masked so the attachment keeps its contents. Blending uses the per-buffer override, else the
 * program's blend (or, for a program that inherits it, the blend of the draw it replaces), and is
 * off for integer targets.
 */
public final class AttachmentPlanner {
    private AttachmentPlanner() {
    }

    /**
     * @param program          the program
     * @param layout           the attachments of the pass it draws in
     * @param fragmentOutputs  fragment output locations with their numeric class (SPIR-V reflection)
     * @param capabilities     device capabilities
     * @return the plan
     */
    public static AttachmentPlan plan(Program program, AttachmentLayout layout, Map<Integer, ScalarClass> fragmentOutputs,
                                      PipelineCapabilities capabilities) {
        return plan(program, null, layout, fragmentOutputs, capabilities);
    }

    /**
     * @param program          the program
     * @param hostBlend        the blend of the draw the program replaces ({@link PipelineShape#hostBlend()}),
     *                         used instead of the program's when the program inherits it
     *                         ({@code Program.inheritBlend}, as Iris keeps the vanilla pipeline's
     *                         blend); null when there is no such draw
     * @param layout           the attachments of the pass it draws in
     * @param fragmentOutputs  fragment output locations with their numeric class (SPIR-V reflection)
     * @param capabilities     device capabilities
     * @return the plan
     */
    public static AttachmentPlan plan(Program program, Optional<BlendFunction> hostBlend, AttachmentLayout layout,
                                      Map<Integer, ScalarClass> fragmentOutputs, PipelineCapabilities capabilities) {
        Optional<BlendFunction> base = program.inheritBlend() && hostBlend != null ? hostBlend
            : Optional.ofNullable(program.blend()).map(BlendFunctions::of);
        List<Attachment> attachments = layout.attachments();
        List<String> problems = new ArrayList<>();
        List<String> notes = new ArrayList<>();
        if (attachments.size() > ColorTargetState.MAX_COLOR_TARGETS) {
            problems.add(attachments.size() + " color attachments exceed Mojang's limit of " + ColorTargetState.MAX_COLOR_TARGETS);
        }
        boolean[] mapped = new boolean[attachments.size()];
        List<Optional<BlendFunction>> blends = new ArrayList<>();
        attachments.forEach(a -> blends.add(Optional.empty()));
        List<Integer> drawBuffers = program.drawBuffers();
        for (int i = 0; i < drawBuffers.size(); i++) {
            int target = drawBuffers.get(i);
            int slot = slotOf(program, layout, i, target);
            if (slot < 0 || slot >= attachments.size()) {
                problems.add("output " + i + " (target " + target + ") has no attachment in the " + layout.id() + " pass");
                continue;
            }
            if (attachments.get(slot).target() != target) {
                problems.add("output " + i + " (target " + target + ") maps to slot " + slot + ", which holds target " + attachments.get(slot).target());
                continue;
            }
            mapped[slot] = true;
            blends.set(slot, blendOf(program, target, base));
        }
        List<AttachmentPlan.Slot> slots = new ArrayList<>();
        for (int slot = 0; slot < attachments.size(); slot++) {
            Attachment a = attachments.get(slot);
            ScalarClass output = fragmentOutputs.get(slot);
            ScalarClass expected = TextureFormats.numericClass(a.format());
            boolean write = mapped[slot] && output == expected;
            if (mapped[slot] && output != null && output != expected) {
                notes.add("output location " + slot + " is " + output + " but target " + a.target() + " is " + a.format() + "; it is discarded");
            }
            Optional<BlendFunction> blend = write && TextureFormats.blendable(a.format()) ? blends.get(slot) : Optional.empty();
            slots.add(new AttachmentPlan.Slot(a.target(), a.format(), write, blend));
        }
        checkBlends(slots, capabilities, problems);
        return new AttachmentPlan(slots, problems, notes);
    }

    /** Mojang's builder accepts one blend function per pipeline; Vulkan without independentBlend needs identical slots. */
    private static void checkBlends(List<AttachmentPlan.Slot> slots, PipelineCapabilities capabilities, List<String> problems) {
        Set<BlendFunction> functions = new HashSet<>();
        slots.forEach(s -> s.blend().ifPresent(functions::add));
        if (functions.size() > 1) {
            problems.add("targets use different blend functions, which Mojang's pipeline API cannot express");
        }
        if (!capabilities.independentBlend()) {
            Set<String> states = new HashSet<>();
            slots.forEach(s -> states.add(s.write() + "/" + s.blend()));
            if (states.size() > 1) {
                problems.add("attachments need different write masks or blend states, which requires the independentBlend device feature");
            }
        }
    }

    private static int slotOf(Program program, AttachmentLayout layout, int output, int target) {
        if (!layout.shared()) {
            return output;
        }
        return output < program.outputSlots().size() ? program.outputSlots().get(output) : layout.slotOf(target);
    }

    /** The per-buffer override (a null value turns blending off), else the program-wide blend. */
    private static Optional<BlendFunction> blendOf(Program program, int target, Optional<BlendFunction> base) {
        if (program.blendPerBuffer().containsKey(target)) {
            return Optional.ofNullable(program.blendPerBuffer().get(target)).map(BlendFunctions::of);
        }
        return base;
    }
}
