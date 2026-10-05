package dev.shaderbridge.render.raw;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ShaderStage;
import dev.shaderbridge.model.StageModule;
import dev.shaderbridge.render.RenderFixture;
import java.nio.ByteBuffer;
import java.util.EnumSet;
import java.util.List;
import java.util.Set;
import org.junit.jupiter.api.Test;

class SpirvCapabilitiesTest {
    private static final RenderFixture GLIMMER = RenderFixture.load(RenderFixture.GLIMMER);
    private static final int SUBGROUP_COMPUTE = 0x20;

    private static ByteBuffer module(int... capabilities) {
        SpirvAssembler m = new SpirvAssembler();
        for (int c : capabilities) {
            m.op(SpirvAssembler.OP_CAPABILITY, c);
        }
        return m.build();
    }

    @Test
    void readsTheDeclaredCapabilities() {
        assertEquals(Set.of(1, 2, 56), SpirvCapabilities.read(module(56, 1, 2)));
        for (Program program : GLIMMER.dim().programs()) {
            for (StageModule stage : program.stages()) {
                Set<Integer> caps = SpirvCapabilities.read(GLIMMER.blobs().spirv(stage.spirv()));
                assertTrue(caps.contains(1), program.name() + " declares Shader");
            }
        }
    }

    @Test
    void coreCapabilitiesNeedNothing() {
        assertEquals(List.of(), SpirvCapabilities.problems(Set.of(0, 1, 50, 51, 52, 43, 44, 4427), ShaderStage.COMPUTE, EnabledFeatures.none()));
    }

    @Test
    void featureCapabilitiesNeedTheirFeature() {
        Set<Integer> geometryFloat64 = Set.of(1, 2, 10);
        List<String> problems = SpirvCapabilities.problems(geometryFloat64, ShaderStage.GEOMETRY, EnabledFeatures.none());
        assertEquals(2, problems.size(), problems.toString());
        assertTrue(problems.get(0).contains("geometryShader"), problems.get(0));
        assertTrue(problems.get(1).contains("shaderFloat64"), problems.get(1));
        EnabledFeatures enabled = new EnabledFeatures(EnumSet.of(RawFeature.GEOMETRY_SHADER, RawFeature.FLOAT64), 0, 0);
        assertEquals(List.of(), SpirvCapabilities.problems(geometryFloat64, ShaderStage.GEOMETRY, enabled));
        assertEquals(1, SpirvCapabilities.problems(Set.of(56), ShaderStage.COMPUTE, enabled).size(), "StorageImageWriteWithoutFormat");
    }

    @Test
    void subgroupCapabilitiesNeedStageAndOperationSupport() {
        Set<Integer> ballot = Set.of(61, 64);
        EnabledFeatures basicOnly = new EnabledFeatures(Set.of(), SUBGROUP_COMPUTE, 0x1);
        assertEquals(1, SpirvCapabilities.problems(ballot, ShaderStage.COMPUTE, basicOnly).size(), "no ballot support");
        EnabledFeatures ballotInFragment = new EnabledFeatures(Set.of(), 0x10, 0x9);
        assertEquals(2, SpirvCapabilities.problems(ballot, ShaderStage.COMPUTE, ballotInFragment).size(), "no subgroups in compute");
        EnabledFeatures full = new EnabledFeatures(Set.of(), SUBGROUP_COMPUTE, 0xFF);
        assertEquals(List.of(), SpirvCapabilities.problems(ballot, ShaderStage.COMPUTE, full));
    }

    @Test
    void unknownCapabilitiesAreRefused() {
        List<String> problems = SpirvCapabilities.problems(Set.of(5345), ShaderStage.COMPUTE, new EnabledFeatures(EnumSet.allOf(RawFeature.class), 0xFF, 0xFF));
        assertEquals(1, problems.size());
        assertTrue(problems.getFirst().contains("5345"), problems.getFirst());
    }
}
