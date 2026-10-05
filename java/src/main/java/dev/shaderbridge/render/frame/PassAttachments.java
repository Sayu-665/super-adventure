package dev.shaderbridge.render.frame;

import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.Program;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.function.IntPredicate;

/**
 * The color attachments of ShaderBridge's render passes, with the headless executor's rules:
 *
 * <ul>
 *   <li>world geometry draws into the dimension's shared attachment list, each target in its
 *   current texture ({@link FlipState#read});</li>
 *   <li>a composite-style program draws output {@code i} into slot {@code output_slots[i]} (the
 *   identity for fullscreen programs), each target in its other texture ({@link FlipState#write});
 *   a target listed twice or missing gets no texture; {@code final} draws into Minecraft's main
 *   color target.</li>
 * </ul>
 *
 * A slot without a usable texture is an {@link AttachmentSlot.Sink}.
 */
public final class PassAttachments {
    private PassAttachments() {
    }

    /**
     * @param shared the dimension's {@code gbuffer_attachments} or {@code shadow_attachments}
     * @param shadow the shadow pass (shadowcolor targets and their flip state)
     * @param flips  the flip state
     * @param usable which targets have a texture of the pass size
     * @return one slot per shared attachment
     */
    public static List<AttachmentSlot> geometry(List<Integer> shared, boolean shadow, FlipState flips, IntPredicate usable) {
        List<AttachmentSlot> slots = new ArrayList<>(shared.size());
        for (int t : shared) {
            if (usable.test(t)) {
                slots.add(new AttachmentSlot.Target(t, shadow ? flips.shadowRead(t) : flips.read(t)));
            } else {
                slots.add(new AttachmentSlot.Sink());
            }
        }
        return slots;
    }

    /**
     * @param program        a composite-style program
     * @param group          its pass group
     * @param flips          the flip state
     * @param usable         which targets of the pass's kind (shadowcolor for {@code shadowcomp},
     *                       colortex otherwise) have a texture of the pass size
     * @param maxAttachments the device's color attachment limit
     * @return one slot per output location (empty if the program has no outputs)
     */
    public static List<AttachmentSlot> fullscreen(Program program, PassGroup group, FlipState flips, IntPredicate usable, int maxAttachments) {
        if (group == PassGroup.FINAL) {
            return List.of(new AttachmentSlot.MainColor());
        }
        boolean shadow = group == PassGroup.SHADOW_COMP;
        List<AttachmentSlot> slots = new ArrayList<>();
        Set<Integer> assigned = new HashSet<>();
        for (int i = 0; i < program.drawBuffers().size(); i++) {
            int t = program.drawBuffers().get(i);
            int location = i < program.outputSlots().size() ? program.outputSlots().get(i) : i;
            if (location < 0 || location >= maxAttachments) {
                continue;
            }
            while (slots.size() <= location) {
                slots.add(null);
            }
            slots.set(location, usable.test(t) && assigned.add(t)
                ? new AttachmentSlot.Target(t, shadow ? flips.shadowWrite(t) : flips.write(t)) : new AttachmentSlot.Sink());
        }
        for (int s = 0; s < slots.size(); s++) {
            if (slots.get(s) == null) {
                slots.set(s, new AttachmentSlot.Sink());
            }
        }
        return slots;
    }
}
