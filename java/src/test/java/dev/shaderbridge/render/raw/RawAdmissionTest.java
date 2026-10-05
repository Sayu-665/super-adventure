package dev.shaderbridge.render.raw;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.ComputeInfo;
import dev.shaderbridge.model.IndirectDispatch;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.ShaderStage;
import dev.shaderbridge.model.StageModule;
import dev.shaderbridge.model.WorkGroups;
import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.render.pipeline.ProgramVariant;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.util.ArrayList;
import java.util.EnumSet;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/** Which programs the raw path takes, on glimmer (a pack whose computes, SSBOs and images need it). */
class RawAdmissionTest {
    private static final RenderFixture GLIMMER = RenderFixture.load(RenderFixture.GLIMMER);

    private static RawAdmission.Result admit(Program program) {
        return admit(program, EnabledFeatures.none());
    }

    private static RawAdmission.Result admit(Program program, EnabledFeatures features) {
        return RawAdmission.check(GLIMMER.dim(), new ProgramVariant(GLIMMER.dim().folder(), program, GLIMMER.blobs()), features,
            ComputeLimits.MINIMUM);
    }

    @Test
    void everyGlimmerComputeProgramIsAdmitted() {
        int computes = 0;
        for (Program program : GLIMMER.dim().programs()) {
            if (program.kind() instanceof ProgramKind.Compute) {
                RawAdmission.Result.Compute admitted = assertInstanceOf(RawAdmission.Result.Compute.class, admit(program), program.name());
                assertEquals(ShaderStage.COMPUTE, admitted.module().stage());
                assertTrue(admitted.plan().bindable());
                computes++;
            }
        }
        assertEquals(6, computes);
    }

    @Test
    void geometryProgramsAreDeclinedSoTheirSlotFallsBack() {
        RawAdmission.Result result = admit(GLIMMER.program("world0/gbuffers_skybasic", "vanilla_position"));
        String reason = assertInstanceOf(RawAdmission.Result.Rejected.class, result).reason();
        assertTrue(reason.contains("inside Minecraft's render passes") && reason.contains("gbuffers_skybasic"), reason);
    }

    @Test
    void compositeStyleProgramsAreDrawnWhenTheDeviceCanStoreFromTheirStages() {
        Program composite = GLIMMER.program("world0/composite", "fullscreen");
        String reason = assertInstanceOf(RawAdmission.Result.Rejected.class, admit(composite)).reason();
        assertTrue(reason.contains("fragmentStoresAndAtomics") && reason.contains("vertexPipelineStoresAndAtomics"), reason);
        EnabledFeatures stores = new EnabledFeatures(EnumSet.of(RawFeature.FRAGMENT_STORES_AND_ATOMICS, RawFeature.VERTEX_PIPELINE_STORES_AND_ATOMICS),
            0, 0);
        int fullscreen = 0;
        for (Program program : GLIMMER.dim().programs()) {
            if (program.kind() instanceof ProgramKind.Composite) {
                RawAdmission.Result.Fullscreen admitted = assertInstanceOf(RawAdmission.Result.Fullscreen.class, admit(program, stores),
                    program.name());
                assertEquals(List.of(ShaderStage.VERTEX, ShaderStage.FRAGMENT), admitted.modules().stream().map(m -> m.stage()).toList());
                fullscreen++;
            }
        }
        assertEquals(23, fullscreen);
        RawAdmission.Result.Fullscreen last = assertInstanceOf(RawAdmission.Result.Fullscreen.class,
            admit(GLIMMER.program("world0/final", "fullscreen"), stores));
        assertEquals(Map.of(0, ScalarClass.FLOAT), last.fragmentOutputs());
        RawAdmission.Result.Fullscreen storeOnly = assertInstanceOf(RawAdmission.Result.Fullscreen.class,
            admit(GLIMMER.program("world0/prepare2", "fullscreen"), stores));
        assertEquals(Map.of(), storeOnly.fragmentOutputs(), "prepare2 only writes storage buffers");
    }

    @Test
    void fullscreenDrawsHaveNoVertexInputs() {
        Program entities = GLIMMER.program("world0/gbuffers_spidereyes", "vanilla_entity");
        Program asComposite = Models.with(entities, new ProgramKind.Composite(dev.shaderbridge.model.PassGroup.COMPOSITE, 1), null);
        String reason = assertInstanceOf(RawAdmission.Result.Rejected.class, admit(asComposite, new EnabledFeatures(EnumSet.allOf(RawFeature.class),
            0, 0))).reason();
        assertTrue(reason.contains("vertex attributes"), reason);
    }

    /**
     * The raw path builds no tessellation pipeline, so no pipeline of it would need
     * {@code VkPipelineTessellationDomainOriginStateCreateInfo{LOWER_LEFT}} (ARCHITECTURE.md §4):
     * tessellated composite-style programs are rejected (a fullscreen draw of triangles cannot feed
     * patches) and geometry programs never run there.
     */
    @Test
    void tessellatedProgramsNeverReachTheRawPath() {
        EnabledFeatures all = new EnabledFeatures(EnumSet.allOf(RawFeature.class), 0, 0);
        Program composite = GLIMMER.program("world0/final", "fullscreen");
        StageModule vertex = composite.stage(ShaderStage.VERTEX).orElseThrow();
        List<StageModule> stages = new ArrayList<>(composite.stages());
        stages.add(1, new StageModule(ShaderStage.TESS_CONTROL, vertex.entryPoint(), vertex.spirv(), null, null, "final.tcs"));
        stages.add(2, new StageModule(ShaderStage.TESS_EVAL, vertex.entryPoint(), vertex.spirv(), null, null, "final.tes"));
        Program tessellated = new Program(composite.name(), composite.kind(), composite.drawProfile(), true, stages, composite.drawBuffers(),
            composite.outputSlots(), composite.outputTypes(), composite.blend(), composite.blendPerBuffer(), composite.alphaTest(), composite.viewport(),
            composite.mipmapTargets(), composite.bindingsUsed(), composite.vertexInputs(), composite.pushConstantSize(), composite.compute(),
            composite.cull(), composite.synthesizedFrom());
        String reason = assertInstanceOf(RawAdmission.Result.Rejected.class, admit(tessellated, all)).reason();
        assertTrue(reason.contains("tessellation"), reason);
        Program terrain = GLIMMER.program("world0/gbuffers_skybasic", "vanilla_position");
        assertInstanceOf(RawAdmission.Result.Rejected.class, admit(Models.with(terrain, terrain.kind(), null), all));
    }

    @Test
    void localSizesBeyondTheDeviceAreRejected() {
        Program setup = GLIMMER.program("world0/setup.csh", null);
        Program wide = Models.with(setup, setup.kind(), new ComputeInfo(List.of(256, 1, 1), setup.compute().workGroups(), null));
        String reason = assertInstanceOf(RawAdmission.Result.Rejected.class, admit(wide)).reason();
        assertTrue(reason.contains("local size"), reason);
    }

    @Test
    void indirectDispatchesNeedADeclaredBuffer() {
        Program setup = GLIMMER.program("world0/setup.csh", null);
        WorkGroups groups = setup.compute().workGroups();
        Program known = Models.with(setup, setup.kind(), new ComputeInfo(setup.compute().localSize(), groups, new IndirectDispatch(2, 0)));
        assertInstanceOf(RawAdmission.Result.Compute.class, admit(known));
        Program unknown = Models.with(setup, setup.kind(), new ComputeInfo(setup.compute().localSize(), groups, new IndirectDispatch(9, 0)));
        String reason = assertInstanceOf(RawAdmission.Result.Rejected.class, admit(unknown)).reason();
        assertTrue(reason.contains("storage buffer 9"), reason);
    }

    @Test
    void programsWithoutComputeModulesAreRejected() {
        Program setup = GLIMMER.program("world0/setup.csh", null);
        Program noInfo = Models.with(setup, setup.kind(), null);
        assertInstanceOf(RawAdmission.Result.Rejected.class, admit(noInfo));
        Program geometryCompute = Models.with(setup, new ProgramKind.GeometryCompute(dev.shaderbridge.model.GeometryProgram.SHADOW, 'a'),
            setup.compute());
        assertInstanceOf(RawAdmission.Result.Compute.class, admit(geometryCompute), "shadow.csh-style computes run like composite ones");
    }
}
