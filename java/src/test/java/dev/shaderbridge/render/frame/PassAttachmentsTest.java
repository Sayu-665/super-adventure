package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertEquals;

import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.render.RenderFixture;
import java.util.List;
import java.util.Set;
import org.junit.jupiter.api.Test;

/** {@link PassAttachments}: shared geometry attachments and composite outputs. */
class PassAttachmentsTest {
    private static final Program COMPOSITE = RenderFixture.load(RenderFixture.TUTORIAL4).dim().programs().stream()
        .filter(p -> p.name().endsWith("composite")).findFirst().orElseThrow();

    static Program withOutputs(Program p, List<Integer> drawBuffers, List<Integer> outputSlots) {
        return new Program(p.name(), p.kind(), p.drawProfile(), p.requiresRawVulkan(), p.stages(), drawBuffers, outputSlots, p.outputTypes(), p.blend(),
            p.blendPerBuffer(), p.alphaTest(), p.viewport(), p.mipmapTargets(), p.bindingsUsed(), p.vertexInputs(), p.pushConstantSize(), p.compute(),
            p.cull(), p.synthesizedFrom());
    }

    @Test
    void geometryUsesTheCurrentTexturesAndKeepsSlotZeroBacked() {
        FlipState flips = new FlipState();
        flips.flip(List.of(3));
        flips.flipShadow(List.of(1));
        assertEquals(List.of(new AttachmentSlot.Sink(), new AttachmentSlot.Target(3, true), new AttachmentSlot.Unused(), new AttachmentSlot.Target(4, false)),
            PassAttachments.geometry(List.of(0, 3, 5, 4), false, flips, Set.of(3, 4)::contains));
        assertEquals(List.of(new AttachmentSlot.Target(0, false), new AttachmentSlot.Target(1, true)),
            PassAttachments.geometry(List.of(0, 1), true, flips, t -> true));
    }

    @Test
    void compositeOutputsGoToTheOtherTexture() {
        FlipState flips = new FlipState();
        flips.flip(List.of(2));
        Program p = withOutputs(COMPOSITE, List.of(2, 7), List.of(0, 1));
        assertEquals(List.of(new AttachmentSlot.Target(2, false), new AttachmentSlot.Target(7, true)),
            PassAttachments.fullscreen(p, PassGroup.COMPOSITE, flips, t -> true, 8));
        assertEquals(List.of(new AttachmentSlot.Target(2, true), new AttachmentSlot.Target(7, true)),
            PassAttachments.fullscreen(p, PassGroup.SHADOW_COMP, flips, t -> true, 8), "shadowcomp follows the shadowcolor flips");
        assertEquals(List.of(2, 7), targets(PassAttachments.fullscreen(p, PassGroup.COMPOSITE, flips, t -> true, 8)));
    }

    @Test
    void missingAndRepeatedTargetsGetNoTexture() {
        FlipState flips = new FlipState();
        Program p = withOutputs(COMPOSITE, List.of(9, 4, 4, 6), List.of());
        assertEquals(List.of(new AttachmentSlot.Sink(), new AttachmentSlot.Target(4, true), new AttachmentSlot.Unused(), new AttachmentSlot.Unused()),
            PassAttachments.fullscreen(p, PassGroup.DEFERRED, flips, Set.of(4)::contains, 8));
        assertEquals(List.of(new AttachmentSlot.Sink(), new AttachmentSlot.Target(4, true)),
            PassAttachments.fullscreen(p, PassGroup.DEFERRED, flips, Set.of(4)::contains, 2), "outputs past the attachment limit are dropped");
    }

    @Test
    void outputSlotsPlaceOutputsAndHolesAreFilled() {
        Program p = withOutputs(COMPOSITE, List.of(5), List.of(2));
        assertEquals(List.of(new AttachmentSlot.Sink(), new AttachmentSlot.Unused(), new AttachmentSlot.Target(5, true)),
            PassAttachments.fullscreen(p, PassGroup.COMPOSITE, new FlipState(), t -> true, 8));
    }

    @Test
    void finalDrawsIntoTheMainTargetAndProgramsWithoutOutputsDrawNothing() {
        assertEquals(List.of(new AttachmentSlot.MainColor()), PassAttachments.fullscreen(COMPOSITE, PassGroup.FINAL, new FlipState(), t -> false, 8));
        assertEquals(List.of(), PassAttachments.fullscreen(withOutputs(COMPOSITE, List.of(), List.of()), PassGroup.COMPOSITE, new FlipState(), t -> true, 8));
    }

    static List<Integer> targets(List<AttachmentSlot> slots) {
        return slots.stream().filter(s -> s instanceof AttachmentSlot.Target).map(s -> ((AttachmentSlot.Target) s).target()).toList();
    }
}
