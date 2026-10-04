package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.ComputeInfo;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.WorkGroups;
import java.util.List;
import org.junit.jupiter.api.Test;

/** {@link DispatchSize} (the cases of {@code dispatch_size} in sb-runtime) and the extent {@link ComputeDispatcher} covers. */
class DispatchSizeTest {
    private static final int[] MAX = {65535, 65535, 65535};

    private static ComputeInfo compute(List<Integer> local, WorkGroups groups) {
        return new ComputeInfo(local, groups, null);
    }

    @Test
    void absoluteWorkGroupsAreUsedAsGiven() {
        assertArrayEquals(new int[] {4, 2, 1}, DispatchSize.of(compute(List.of(8, 8, 1), new WorkGroups.Absolute(4, 2, 1)), 1920, 1080, MAX));
        assertArrayEquals(new int[] {65535, 1, 1}, DispatchSize.of(compute(List.of(1, 1, 1), new WorkGroups.Absolute(-1, 1, 1)), 1, 1, MAX),
            "u32 counts are clamped to the device limit");
    }

    @Test
    void relativeWorkGroupsCoverTheScaledExtent() {
        assertArrayEquals(new int[] {120, 68, 1}, DispatchSize.of(compute(List.of(16, 16, 1), new WorkGroups.Relative(1, 1)), 1920, 1080, MAX));
        assertArrayEquals(new int[] {60, 34, 1}, DispatchSize.of(compute(List.of(16, 16, 1), new WorkGroups.Relative(0.5f, 0.5f)), 1920, 1080, MAX));
        assertArrayEquals(new int[] {0, 0, 1}, DispatchSize.of(compute(List.of(16, 16, 1), new WorkGroups.Relative(Float.NaN, -2)), 1920, 1080, MAX));
    }

    @Test
    void withoutComputeInformationTheExtentIsCoveredByUnitGroups() {
        assertArrayEquals(new int[] {1920, 1080, 1}, DispatchSize.of(null, 1920, 1080, MAX));
        assertArrayEquals(new int[] {100, 50, 1}, DispatchSize.of(compute(List.of(0, 0, 0), new WorkGroups.Relative(1, 1)), 100, 50, MAX),
            "a zero local size counts as one");
        assertArrayEquals(new int[] {8, 8, 1}, DispatchSize.of(null, 1920, 1080, new int[] {8, 8, 8}));
    }

    @Test
    void shadowComputesCoverTheShadowMap() {
        assertTrue(ComputeDispatcher.coversShadowMap(kind(new ProgramKind.Compute(PassGroup.SHADOW_COMP, 1, 'a'))));
        assertTrue(ComputeDispatcher.coversShadowMap(kind(new ProgramKind.GeometryCompute(GeometryProgram.SHADOW, null))));
        assertFalse(ComputeDispatcher.coversShadowMap(kind(new ProgramKind.Compute(PassGroup.COMPOSITE, 1, null))));
        assertFalse(ComputeDispatcher.coversShadowMap(kind(new ProgramKind.GeometryCompute(GeometryProgram.TERRAIN, null))));
        assertFalse(ComputeDispatcher.coversShadowMap(kind(new ProgramKind.Composite(PassGroup.SHADOW_COMP, 1))));
    }

    private static Program kind(ProgramKind kind) {
        Program p = FramePlanTest.BASE.programs().getFirst();
        return new Program(p.name(), kind, p.drawProfile(), p.requiresRawVulkan(), p.stages(), p.drawBuffers(), p.outputSlots(), p.outputTypes(), p.blend(),
            p.blendPerBuffer(), p.alphaTest(), p.viewport(), p.mipmapTargets(), p.bindingsUsed(), p.vertexInputs(), p.pushConstantSize(), p.compute(),
            p.cull(), p.synthesizedFrom());
    }
}
