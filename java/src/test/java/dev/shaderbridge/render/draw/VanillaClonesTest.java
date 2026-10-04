package dev.shaderbridge.render.draw;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.pipeline.ColorTargetState;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.render.pipeline.AttachmentLayout;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import net.minecraft.client.renderer.RenderPipelines;
import org.junit.jupiter.api.Test;

/** {@link VanillaClones}: vanilla pipelines adapted to ShaderBridge pass layouts. */
class VanillaClonesTest {
    private static final AttachmentLayout GBUFFERS = AttachmentLayout.shared(RenderFixture.load(RenderFixture.TUTORIAL4).dim(), false).orElseThrow();
    private static final RenderPipeline TRANSLUCENT = RenderPipelines.TRANSLUCENT_TERRAIN_MULTIDRAW;

    static CompiledRenderPipeline compiled() {
        return new CompiledRenderPipeline() {
            @Override
            public boolean isClosed() {
                return false;
            }

            @Override
            public void close() {
            }
        };
    }

    private static void assertSameDraw(RenderPipeline vanilla, RenderPipeline clone) {
        assertEquals(vanilla.getShaders(), clone.getShaders());
        assertSame(vanilla.getShaderDefines(), clone.getShaderDefines());
        assertEquals(vanilla.getBindGroupLayouts(), clone.getBindGroupLayouts());
        assertEquals(vanilla.getVertexFormatBindings(), clone.getVertexFormatBindings());
        assertEquals(vanilla.getPrimitiveTopology(), clone.getPrimitiveTopology());
        assertEquals(vanilla.isCull(), clone.isCull());
        assertEquals(vanilla.getPolygonMode(), clone.getPolygonMode());
        assertEquals(vanilla.pushConstantSize(), clone.pushConstantSize());
        assertEquals("shaderbridge", clone.getLocation().getNamespace());
    }

    private static List<GpuFormat> formats(RenderPipeline p) {
        return p.getColorTargetStates().stream().map(ColorTargetState::format).toList();
    }

    @Test
    void fallbackWritesTheFallbackTargetInSlotZeroWhenTheDeviceAllowsMasks() {
        RenderPipeline clone = VanillaClones.fallbackPipeline(TRANSLUCENT, GBUFFERS, GBUFFERS.attachments().getFirst().target(), true);
        assertSameDraw(TRANSLUCENT, clone);
        ColorTargetState vanilla = TRANSLUCENT.getColorTargetStates().getFirst();
        List<ColorTargetState> states = clone.getColorTargetStates();
        assertEquals(GBUFFERS.attachments().stream().map(AttachmentLayout.Attachment::format).toList(), formats(clone));
        assertEquals(vanilla.blendFunction(), states.getFirst().blendFunction());
        assertEquals(vanilla.writeMask(), states.getFirst().writeMask());
        for (ColorTargetState s : states.subList(1, states.size())) {
            assertEquals(ColorTargetState.WRITE_NONE, s.writeMask());
            assertTrue(s.blendFunction().isEmpty());
        }
        assertEquals(TRANSLUCENT.getDepthStencilState(), clone.getDepthStencilState());
    }

    @Test
    void withoutIndependentBlendOrAFallbackInSlotZeroOnlyDepthIsWritten() {
        int slot0 = GBUFFERS.attachments().getFirst().target();
        assertTrue(GBUFFERS.attachments().size() > 1);
        for (RenderPipeline clone : List.of(VanillaClones.fallbackPipeline(TRANSLUCENT, GBUFFERS, slot0, false),
            VanillaClones.fallbackPipeline(TRANSLUCENT, GBUFFERS, 31, true))) {
            assertTrue(clone.getColorTargetStates().stream().allMatch(s -> s.writeMask() == ColorTargetState.WRITE_NONE));
            assertEquals(TRANSLUCENT.getDepthStencilState(), clone.getDepthStencilState());
        }
        AttachmentLayout single = AttachmentLayout.single("one", 0, GpuFormat.RGBA16_FLOAT);
        RenderPipeline one = VanillaClones.fallbackPipeline(TRANSLUCENT, single, 0, false);
        assertEquals(List.of(GpuFormat.RGBA16_FLOAT), formats(one));
        assertEquals(TRANSLUCENT.getColorTargetStates().getFirst().writeMask(), one.getColorTargetStates().getFirst().writeMask(),
            "a single attachment needs no independent blend");
    }

    @Test
    void discardClonesWriteNothing() {
        RenderPipeline clone = VanillaClones.discardPipeline(RenderPipelines.SOLID_TERRAIN_MULTIDRAW, GBUFFERS);
        assertSameDraw(RenderPipelines.SOLID_TERRAIN_MULTIDRAW, clone);
        assertNull(clone.getDepthStencilState());
        assertTrue(clone.getColorTargetStates().stream().allMatch(s -> s.writeMask() == ColorTargetState.WRITE_NONE));
        RenderPipeline depthOnly = VanillaClones.discardPipeline(RenderPipelines.SOLID_TERRAIN_MULTIDRAW, new AttachmentLayout("shadow", List.of(), true));
        assertTrue(depthOnly.getColorTargetStates().isEmpty());
    }

    @Test
    void clonesAreCompiledOncePerPipelineLayoutAndMode() {
        List<RenderPipeline> compiles = new ArrayList<>();
        CompiledRenderPipeline result = compiled();
        VanillaClones clones = new VanillaClones(p -> {
            compiles.add(p);
            return p.getLocation().getPath().contains("solid") ? null : result;
        }, true);
        Optional<CompiledRenderPipeline> a = clones.fallback(TRANSLUCENT, GBUFFERS, 0);
        Optional<CompiledRenderPipeline> b = clones.fallback(TRANSLUCENT, GBUFFERS, 0);
        assertSame(result, a.orElseThrow());
        assertSame(result, b.orElseThrow());
        assertSame(compiles.get(0), compiles.get(1), "the same clone object is handed to the pipeline cache");
        clones.discard(TRANSLUCENT, GBUFFERS);
        assertEquals(3, compiles.size());
        assertFalse(compiles.get(2) == compiles.get(0));
        assertTrue(clones.discard(RenderPipelines.SOLID_TERRAIN_MULTIDRAW, GBUFFERS).isEmpty(), "a failed compile is reported as empty");
    }
}
