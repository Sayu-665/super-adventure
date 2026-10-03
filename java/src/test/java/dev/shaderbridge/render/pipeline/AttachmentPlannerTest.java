package dev.shaderbridge.render.pipeline;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.pipeline.BlendFunction;
import com.mojang.renderpearl.api.pipeline.ColorTargetState;
import dev.shaderbridge.model.BlendFactor;
import dev.shaderbridge.model.BlendMode;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.render.pipeline.AttachmentLayout.Attachment;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.stream.IntStream;
import org.junit.jupiter.api.Test;

class AttachmentPlannerTest {
    private static final PipelineCapabilities BASELINE = PipelineCapabilities.baseline();
    private static final PipelineCapabilities INDEPENDENT = new PipelineCapabilities(true, PipelineCapabilities.DEFAULT_MAX_DESCRIPTORS);
    private static final BlendMode TRANSLUCENT = new BlendMode(BlendFactor.SRC_ALPHA, BlendFactor.ONE_MINUS_SRC_ALPHA, BlendFactor.ONE,
        BlendFactor.ONE_MINUS_SRC_ALPHA);
    private static final BlendMode ADD = new BlendMode(BlendFactor.ONE, BlendFactor.ONE, BlendFactor.ONE, BlendFactor.ONE);
    private static final RenderFixture TUTORIAL = RenderFixture.load(RenderFixture.TUTORIAL4);

    private static Map<Integer, ScalarClass> floats(int count) {
        Map<Integer, ScalarClass> out = new HashMap<>();
        IntStream.range(0, count).forEach(i -> out.put(i, ScalarClass.FLOAT));
        return out;
    }

    @Test
    void sharedGbufferAttachmentsTakeTheirFormatsFromTheTargets() {
        Program terrain = TUTORIAL.program("gbuffers_terrain", "vanilla_terrain");
        AttachmentLayout layout = AttachmentLayout.geometry(TUTORIAL.dim(), terrain, false);
        assertEquals("gbuffers", layout.id());
        assertTrue(layout.shared());
        // colortex0 RGBA16F, colortex1/2 RGB16 (widened to four components).
        assertEquals(List.of(new Attachment(0, GpuFormat.RGBA16_FLOAT), new Attachment(1, GpuFormat.RGBA16_UNORM), new Attachment(2, GpuFormat.RGBA16_UNORM)),
            layout.attachments());
        AttachmentLayout shadow = AttachmentLayout.geometry(TUTORIAL.dim(), TUTORIAL.program("shadow", "vanilla_terrain"), true);
        assertEquals(List.of(new Attachment(0, GpuFormat.RGBA8_UNORM), new Attachment(1, GpuFormat.RGBA8_UNORM)), shadow.attachments());
    }

    @Test
    void programWritingEverySharedSlotIsExpressibleWithoutIndependentBlend() {
        Program terrain = TUTORIAL.program("gbuffers_terrain", "vanilla_terrain");
        AttachmentPlan plan = AttachmentPlanner.plan(terrain, AttachmentLayout.geometry(TUTORIAL.dim(), terrain, false), floats(3), BASELINE);
        assertTrue(plan.expressible(), plan.problems().toString());
        assertEquals(List.of(0, 1, 2), plan.writtenTargets());
        BlendFunction blend = BlendFunctions.of(terrain.blend());
        for (ColorTargetState state : plan.colorTargetStates()) {
            assertEquals(ColorTargetState.WRITE_ALL, state.writeMask());
            assertEquals(Optional.of(blend), state.blendFunction());
        }
    }

    @Test
    void unwrittenSharedSlotsAreMaskedAndNeedIndependentBlend() {
        Program entities = TUTORIAL.program("gbuffers_entities", "vanilla_entity");
        AttachmentLayout layout = AttachmentLayout.geometry(TUTORIAL.dim(), entities, false);
        AttachmentPlan baseline = AttachmentPlanner.plan(entities, layout, floats(1), BASELINE);
        assertFalse(baseline.expressible());
        assertTrue(baseline.problems().getFirst().contains("independentBlend"), baseline.problems().toString());
        AttachmentPlan plan = AttachmentPlanner.plan(entities, layout, floats(1), INDEPENDENT);
        assertTrue(plan.expressible(), plan.problems().toString());
        List<ColorTargetState> states = plan.colorTargetStates();
        assertEquals(ColorTargetState.WRITE_ALL, states.get(0).writeMask());
        assertEquals(ColorTargetState.WRITE_NONE, states.get(1).writeMask());
        assertEquals(ColorTargetState.WRITE_NONE, states.get(2).writeMask());
        assertEquals(Optional.empty(), states.get(1).blendFunction());
        assertEquals(List.of(GpuFormat.RGBA16_FLOAT, GpuFormat.RGBA16_UNORM, GpuFormat.RGBA16_UNORM), states.stream().map(ColorTargetState::format).toList());
    }

    @Test
    void outputSlotsPlaceLogicalOutputsInTheSharedList() {
        Program base = TUTORIAL.program("gbuffers_entities", "vanilla_entity");
        // RENDERTARGETS: 2,0 -> output 0 is colortex2 (slot 2), output 1 is colortex0 (slot 0).
        Program p = Programs.outputs(base, List.of(2, 0), List.of(2, 0), TRANSLUCENT, Map.of());
        AttachmentPlan plan = AttachmentPlanner.plan(p, AttachmentLayout.geometry(TUTORIAL.dim(), p, false), Map.of(0, ScalarClass.FLOAT, 2,
            ScalarClass.FLOAT), INDEPENDENT);
        assertEquals(List.of(0, 2), plan.writtenTargets());
        Program wrong = Programs.outputs(base, List.of(2), List.of(1), TRANSLUCENT, Map.of());
        AttachmentPlan bad = AttachmentPlanner.plan(wrong, AttachmentLayout.geometry(TUTORIAL.dim(), wrong, false), floats(3), INDEPENDENT);
        assertTrue(bad.problems().getFirst().contains("maps to slot 1"), bad.problems().toString());
        Program outside = Programs.outputs(base, List.of(7), List.of(5), TRANSLUCENT, Map.of());
        assertTrue(AttachmentPlanner.plan(outside, AttachmentLayout.geometry(TUTORIAL.dim(), outside, false), floats(3), INDEPENDENT)
            .problems().getFirst().contains("has no attachment"));
    }

    @Test
    void perBufferBlendOverridesTheProgramBlend() {
        Program base = TUTORIAL.program("gbuffers_terrain", "vanilla_terrain");
        Map<Integer, BlendMode> perBuffer = new HashMap<>();
        perBuffer.put(1, null);
        perBuffer.put(2, TRANSLUCENT);
        Program p = Programs.outputs(base, List.of(0, 1, 2), List.of(0, 1, 2), TRANSLUCENT, perBuffer);
        AttachmentPlan plan = AttachmentPlanner.plan(p, AttachmentLayout.geometry(TUTORIAL.dim(), p, false), floats(3), INDEPENDENT);
        assertTrue(plan.expressible(), plan.problems().toString());
        List<Optional<BlendFunction>> blends = plan.slots().stream().map(AttachmentPlan.Slot::blend).toList();
        assertEquals(List.of(Optional.of(BlendFunctions.of(TRANSLUCENT)), Optional.empty(), Optional.of(BlendFunctions.of(TRANSLUCENT))), blends);
        // Mojang's builder takes one blend function per pipeline.
        perBuffer.put(2, ADD);
        Program twoFunctions = Programs.outputs(base, List.of(0, 1, 2), List.of(0, 1, 2), TRANSLUCENT, perBuffer);
        AttachmentPlan rejected = AttachmentPlanner.plan(twoFunctions, AttachmentLayout.geometry(TUTORIAL.dim(), twoFunctions, false), floats(3),
            INDEPENDENT);
        assertTrue(rejected.problems().getFirst().contains("different blend functions"), rejected.problems().toString());
    }

    @Test
    void integerTargetsAreNeverBlendedAndMismatchedOutputsAreDiscarded() {
        Program base = TUTORIAL.program("gbuffers_terrain", "vanilla_terrain");
        AttachmentLayout layout = new AttachmentLayout("t", List.of(new Attachment(0, GpuFormat.RGBA16_FLOAT), new Attachment(1, GpuFormat.R32_UINT)), false);
        Program p = Programs.outputs(base, List.of(0, 1), List.of(0, 1), TRANSLUCENT, Map.of());
        AttachmentPlan plan = AttachmentPlanner.plan(p, layout, Map.of(0, ScalarClass.FLOAT, 1, ScalarClass.UINT), INDEPENDENT);
        assertEquals(Optional.empty(), plan.slots().get(1).blend());
        assertTrue(plan.slots().get(1).write());
        AttachmentPlan mismatch = AttachmentPlanner.plan(p, layout, Map.of(0, ScalarClass.FLOAT, 1, ScalarClass.FLOAT), INDEPENDENT);
        assertFalse(mismatch.slots().get(1).write());
        assertEquals(1, mismatch.notes().size(), mismatch.notes().toString());
    }

    @Test
    void moreThanEightAttachmentsCannotBeExpressed() {
        List<Attachment> nine = IntStream.range(0, 9).mapToObj(i -> new Attachment(i, GpuFormat.RGBA8_UNORM)).toList();
        Program p = TUTORIAL.program("composite", "fullscreen");
        AttachmentPlan plan = AttachmentPlanner.plan(p, new AttachmentLayout("nine", nine, false), floats(1), INDEPENDENT);
        assertTrue(plan.problems().getFirst().contains("exceed"), plan.problems().toString());
    }

    @Test
    void fullscreenLayoutsUseTheProgramsDrawBuffers() {
        Program composite = TUTORIAL.program("composite", "fullscreen");
        AttachmentLayout layout = AttachmentLayout.fullscreen(TUTORIAL.dim(), composite);
        assertFalse(layout.shared());
        assertEquals(List.of(new Attachment(0, GpuFormat.RGBA16_FLOAT)), layout.attachments());
        AttachmentLayout mainTarget = AttachmentLayout.single("final", 0, GpuFormat.RGBA8_UNORM);
        AttachmentPlan plan = AttachmentPlanner.plan(TUTORIAL.program("final", "fullscreen"), mainTarget, floats(1), BASELINE);
        assertTrue(plan.expressible(), plan.problems().toString());
        assertEquals(1, mainTarget.slotOf(0) + 1);
        assertEquals(-1, mainTarget.slotOf(3));
    }
}
