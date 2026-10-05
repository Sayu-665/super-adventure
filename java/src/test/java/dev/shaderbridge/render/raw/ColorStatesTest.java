package dev.shaderbridge.render.raw;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.BlendFactor;
import dev.shaderbridge.model.BlendMode;
import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import org.junit.jupiter.api.Test;
import org.lwjgl.vulkan.VK10;

/** Color attachment states of raw fullscreen pipelines (the headless executor's and AttachmentPlanner's rules). */
class ColorStatesTest {
    private static final RenderFixture GLIMMER = RenderFixture.load(RenderFixture.GLIMMER);
    private static final BlendMode ADD = new BlendMode(BlendFactor.ONE, BlendFactor.ONE, BlendFactor.ONE, BlendFactor.ONE);
    private static final BlendMode ALPHA = new BlendMode(BlendFactor.SRC_ALPHA, BlendFactor.ONE_MINUS_SRC_ALPHA, BlendFactor.ONE, BlendFactor.ZERO);

    /** A composite program writing colortex 3, 5 and 7, blending with ADD except colortex 5 (off) and 7 (ALPHA). */
    private static Program program() {
        Program p = GLIMMER.program("world0/composite", "fullscreen");
        Map<Integer, BlendMode> perBuffer = new HashMap<>();
        perBuffer.put(5, null);
        perBuffer.put(7, ALPHA);
        return new Program(p.name(), new ProgramKind.Composite(PassGroup.COMPOSITE, 0), p.drawProfile(), true, p.stages(), List.of(3, 5, 7),
            List.of(0, 1, 2), List.of("float", "float", "uint"), ADD, perBuffer, null, p.viewport(), List.of(), p.bindingsUsed(), List.of(), 0,
            null, null, null);
    }

    private static final Map<Integer, ScalarClass> OUTPUTS = Map.of(0, ScalarClass.FLOAT, 1, ScalarClass.FLOAT, 2, ScalarClass.FLOAT);

    @Test
    void outputsWriteTheirSlotWithTheBlendOfTheirTarget() {
        List<ColorStates.Slot> slots = ColorStates.of(program(),
            List.of(Optional.of(GpuFormat.RGBA16_FLOAT), Optional.of(GpuFormat.RGBA8_UNORM), Optional.of(GpuFormat.RGBA8_UNORM)), OUTPUTS);
        assertEquals(new ColorStates.Slot(Optional.of(GpuFormat.RGBA16_FLOAT), true, Optional.of(ADD)), slots.get(0), "the program's blend");
        assertEquals(new ColorStates.Slot(Optional.of(GpuFormat.RGBA8_UNORM), true, Optional.empty()), slots.get(1), "turned off for colortex5");
        assertEquals(new ColorStates.Slot(Optional.of(GpuFormat.RGBA8_UNORM), true, Optional.of(ALPHA)), slots.get(2), "colortex7's override");
    }

    @Test
    void slotsWithoutAttachmentOrMatchingOutputAreNotWritten() {
        List<ColorStates.Slot> slots = ColorStates.of(program(),
            List.of(Optional.of(GpuFormat.RGBA16_FLOAT), Optional.empty(), Optional.of(GpuFormat.R32_UINT)), OUTPUTS);
        assertEquals(new ColorStates.Slot(Optional.empty(), false, Optional.empty()), slots.get(1));
        assertEquals(new ColorStates.Slot(Optional.of(GpuFormat.R32_UINT), false, Optional.empty()), slots.get(2),
            "a float output into an integer target is discarded, and integer targets never blend");
        assertEquals(new ColorStates.Slot(Optional.of(GpuFormat.R32_UINT), true, Optional.empty()),
            ColorStates.of(program(), List.of(Optional.empty(), Optional.empty(), Optional.of(GpuFormat.R32_UINT)), Map.of(2, ScalarClass.UINT)).get(2));
    }

    @Test
    void differingStatesNeedIndependentBlend() {
        List<ColorStates.Slot> mixed = ColorStates.of(program(),
            List.of(Optional.of(GpuFormat.RGBA16_FLOAT), Optional.of(GpuFormat.RGBA8_UNORM), Optional.empty()), OUTPUTS);
        assertTrue(ColorStates.problem(mixed, false).isPresent());
        assertEquals(Optional.empty(), ColorStates.problem(mixed, true));
        List<ColorStates.Slot> same = ColorStates.of(program(), List.of(Optional.of(GpuFormat.RGBA16_FLOAT), Optional.empty(), Optional.empty()),
            OUTPUTS);
        assertEquals(Optional.empty(), ColorStates.problem(same, false), "slots without attachment take the common state");
    }

    @Test
    void blendFactorsMapToVulkan() {
        assertEquals(VK10.VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA, ColorStates.vkFactor(BlendFactor.ONE_MINUS_SRC_ALPHA));
        assertEquals(VK10.VK_BLEND_FACTOR_SRC_ALPHA_SATURATE, ColorStates.vkFactor(BlendFactor.SRC_ALPHA_SATURATE));
        for (BlendFactor f : BlendFactor.values()) {
            assertTrue(ColorStates.vkFactor(f) >= 0, f.toString());
        }
    }
}
