package dev.shaderbridge.render.raw;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.BindingTable;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ResourceKind;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.model.ShaderStage;
import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.render.pipeline.SpirvReflection;
import dev.shaderbridge.render.pipeline.SpirvReflection.Descriptor;
import dev.shaderbridge.render.pipeline.SpirvReflection.DescriptorType;
import dev.shaderbridge.render.pipeline.SpirvReflection.ImageDim;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.util.List;
import java.util.Map;
import java.util.function.Function;
import java.util.stream.Collectors;
import org.junit.jupiter.api.Test;
import org.lwjgl.vulkan.VK10;

/** Descriptor layout planning from glimmer's real SPIR-V and model, and the cases the raw path refuses. */
class DescriptorPlannerTest {
    private static final RenderFixture GLIMMER = RenderFixture.load(RenderFixture.GLIMMER);

    private static DescriptorPlan plan(Program program) {
        return DescriptorPlanner.plan(GLIMMER.dim(), program, Models.reflect(GLIMMER, program));
    }

    private static Map<Integer, DescriptorPlan.Binding> set(DescriptorPlan plan, int set) {
        return plan.sets().stream().filter(s -> s.set() == set).findFirst().orElseThrow(() -> new AssertionError("no set " + set))
            .bindings().stream().collect(Collectors.toMap(DescriptorPlan.Binding::binding, Function.identity()));
    }

    private static ResourceRef resource(DescriptorPlan.Binding b) {
        return assertInstanceOf(DescriptorPlan.Source.Pack.class, b.source()).entry().resource();
    }

    @Test
    void computeProgramBindsTheFrameBlockStorageBuffersAndImages() {
        DescriptorPlan plan = plan(GLIMMER.program("world0/setup.csh", null));
        assertTrue(plan.bindable(), plan.problems().toString());
        assertEquals(List.of(0, 2), plan.sets().stream().map(DescriptorPlan.SetLayout::set).toList(), "sb_Frame in set 0, buffers and images in set 2");
        DescriptorPlan.Binding frame = set(plan, 0).get(0);
        assertInstanceOf(DescriptorPlan.Source.Frame.class, frame.source());
        assertEquals(VK10.VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, frame.type());
        assertEquals("sb_Frame", frame.name());
        Map<Integer, DescriptorPlan.Binding> storage = set(plan, 2);
        assertEquals(VK10.VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, storage.get(0).type());
        assertEquals(new ResourceRef.Ssbo(0), resource(storage.get(0)));
        assertEquals(new ResourceRef.Ssbo(1), resource(storage.get(1)));
        assertEquals(VK10.VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, storage.get(19).type());
        assertEquals(new ResourceRef.Image("sunTransmittanceLUT"), resource(storage.get(19)));
        plan.sets().forEach(s -> s.bindings().forEach(b -> {
            assertEquals(VK10.VK_SHADER_STAGE_COMPUTE_BIT, b.stages());
            assertEquals(1, b.count());
        }));
    }

    @Test
    void sampledResourcesComeFromTheBindingTable() {
        DescriptorPlan plan = plan(GLIMMER.program("world0/composite4.csh", null));
        assertTrue(plan.bindable(), plan.problems().toString());
        Map<Integer, DescriptorPlan.Binding> samplers = set(plan, 1);
        assertEquals(VK10.VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, samplers.get(23).type());
        assertEquals(new ResourceRef.DepthTex(1), resource(samplers.get(23)));
        assertEquals(new ResourceRef.DhDepthTex(1), resource(samplers.get(33)));
        DescriptorPlan.Source.Pack shadow = assertInstanceOf(DescriptorPlan.Source.Pack.class, samplers.get(28).source());
        assertEquals(new ResourceRef.ShadowTexHw(1), shadow.entry().resource());
        assertTrue(assertInstanceOf(ResourceKind.Sampler.class, shadow.entry().kind()).shadow(), "shadowtex1HW is a comparison sampler");
        assertEquals(ImageDim.D2, samplers.get(28).descriptor().dim());
    }

    @Test
    void stagesOfOneBindingAreMerged() {
        DescriptorPlan plan = plan(GLIMMER.program("world0/composite", "fullscreen"));
        assertTrue(plan.bindable(), plan.problems().toString());
        DescriptorPlan.Binding environment = set(plan, 2).get(0);
        assertEquals(VK10.VK_SHADER_STAGE_VERTEX_BIT | VK10.VK_SHADER_STAGE_FRAGMENT_BIT, environment.stages(), "used by both stages");
        assertEquals(VK10.VK_SHADER_STAGE_FRAGMENT_BIT, set(plan, 1).get(22).stages(), "depthtex0 only in the fragment stage");
        assertEquals(Map.of(VK10.VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, 2, VK10.VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, 2,
            VK10.VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, 1), plan.descriptorCounts());
    }

    @Test
    void minecraftsHostBlocksCannotBeBound() {
        DescriptorPlan plan = plan(GLIMMER.program("world0/shadow", "vanilla_terrain"));
        assertTrue(plan.problems().stream().anyMatch(p -> p.contains("Minecraft's Globals block")), plan.problems().toString());
    }

    @Test
    void descriptorsOutsideTheBindingTableAreProblems() {
        Program program = GLIMMER.program("world0/setup.csh", null);
        DescriptorPlan plan = DescriptorPlanner.plan(Models.withBindings(GLIMMER.dim(), new BindingTable(List.of())), program,
            Models.reflect(GLIMMER, program));
        assertEquals(3, plan.problems().size(), plan.problems().toString());
        assertTrue(plan.problems().getFirst().contains("is not in the pack's binding table"));
        assertEquals(List.of(0), plan.sets().stream().map(DescriptorPlan.SetLayout::set).toList(), "the frame block needs no table entry");
    }

    @Test
    void unsupportedDescriptorsAreProblems() {
        Program program = GLIMMER.program("world0/setup.csh", null);
        List<Descriptor> descriptors = List.of(
            new Descriptor("undecorated", DescriptorType.UNIFORM_BUFFER, ImageDim.NONE, false, false, 1, ScalarClass.OTHER, -1, -1),
            new Descriptor("runtime", DescriptorType.STORAGE_BUFFER, ImageDim.NONE, false, false, 0, ScalarClass.OTHER, 2, 0),
            new Descriptor("layers", DescriptorType.SAMPLED_IMAGE, ImageDim.D2, true, false, 1, ScalarClass.FLOAT, 1, 5),
            new Descriptor("cube", DescriptorType.SAMPLED_IMAGE, ImageDim.CUBE, false, false, 1, ScalarClass.FLOAT, 1, 6),
            new Descriptor("separate", DescriptorType.SEPARATE_SAMPLER, ImageDim.NONE, false, false, 1, ScalarClass.OTHER, 1, 7),
            new Descriptor("far", DescriptorType.UNIFORM_BUFFER, ImageDim.NONE, false, false, 1, ScalarClass.OTHER, 4, 0));
        SpirvReflection reflection = new SpirvReflection(SpirvReflection.Stage.COMPUTE, descriptors, List.of(), List.of(), 0);
        DescriptorPlan plan = DescriptorPlanner.plan(GLIMMER.dim(), program, Map.of(ShaderStage.COMPUTE, reflection));
        assertEquals(6, plan.problems().size(), plan.problems().toString());
        assertTrue(plan.sets().isEmpty());
    }

    @Test
    void conflictingStagesAreProblems() {
        Program program = GLIMMER.program("world0/composite", "fullscreen");
        Descriptor frame = new Descriptor("sb_Frame", DescriptorType.UNIFORM_BUFFER, ImageDim.NONE, false, false, 1, ScalarClass.OTHER, 0, 0);
        Descriptor clash = new Descriptor("sb_Frame", DescriptorType.UNIFORM_BUFFER, ImageDim.NONE, false, false, 2, ScalarClass.OTHER, 0, 0);
        Map<ShaderStage, SpirvReflection> stages = Map.of(
            ShaderStage.VERTEX, new SpirvReflection(SpirvReflection.Stage.VERTEX, List.of(frame), List.of(), List.of(), 0),
            ShaderStage.FRAGMENT, new SpirvReflection(SpirvReflection.Stage.FRAGMENT, List.of(clash), List.of(), List.of(), 0));
        DescriptorPlan plan = DescriptorPlanner.plan(GLIMMER.dim(), program, stages);
        assertEquals(1, plan.problems().size(), plan.problems().toString());
        assertTrue(plan.problems().getFirst().contains("set 0, binding 0"));
    }

    @Test
    void budgetFitsEveryGlimmerProgramTheRawPathCanBind() {
        for (Program program : GLIMMER.dim().programs()) {
            DescriptorPlan plan = plan(program);
            if (plan.bindable()) {
                assertEquals(List.of(), DescriptorBudget.problems(plan), program.name());
            }
        }
        DescriptorPlan huge = new DescriptorPlan(List.of(new DescriptorPlan.SetLayout(1, List.of(new DescriptorPlan.Binding(1, 0, "many",
            VK10.VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, 2000, VK10.VK_SHADER_STAGE_COMPUTE_BIT, null, new DescriptorPlan.Source.Draw())))), List.of());
        assertEquals(1, DescriptorBudget.problems(huge).size());
    }
}
