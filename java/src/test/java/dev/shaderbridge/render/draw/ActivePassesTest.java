package dev.shaderbridge.render.draw;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import java.lang.reflect.Proxy;
import net.minecraft.client.renderer.RenderPipelines;
import org.junit.jupiter.api.Test;

/** {@link ActivePasses} and {@link CompiledPipelineIndex}: which pass is ShaderBridge's and where pipelines came from. */
class ActivePassesTest {
    private static RenderPass pass() {
        return (RenderPass) Proxy.newProxyInstance(RenderPass.class.getClassLoader(), new Class<?>[] {RenderPass.class}, (p, m, a) -> {
            throw new UnsupportedOperationException(m.getName());
        });
    }

    @Test
    void onlyTheOpenShaderBridgePassIsSubstituted() {
        RenderPass ours = pass();
        RenderPass other = pass();
        CompiledRenderPipeline requested = VanillaClonesTest.compiled();
        CompiledRenderPipeline replacement = VanillaClonesTest.compiled();
        ActivePasses.open(ours, r -> ActivePasses.Substitution.unchanged(replacement));
        try {
            assertTrue(ActivePasses.owns(ours));
            assertFalse(ActivePasses.owns(other));
            assertFalse(ActivePasses.owns(null));
            assertSame(replacement, ActivePasses.substitute(ours, requested).pipeline());
            assertTrue(ActivePasses.substitute(ours, requested).binding().isEmpty(), "vanilla pipelines bind nothing of the pack's");
            assertNull(ActivePasses.substitute(other, requested));
            ActivePasses.close(other);
            assertSame(replacement, ActivePasses.substitute(ours, requested).pipeline(), "closing another pass changes nothing");
        } finally {
            ActivePasses.close(ours);
        }
        assertNull(ActivePasses.substitute(ours, requested));
        assertFalse(ActivePasses.owns(ours), "a closed pass is no longer ShaderBridge's");
    }

    @Test
    void undrawablePipelinesAreSkippedAndUniformsReplacedOnlyInTheirPass() {
        RenderPass ours = pass();
        RenderPass other = pass();
        GpuBufferSlice camera = new GpuBufferSlice(null, 0, 160);
        GpuBufferSlice shadow = new GpuBufferSlice(null, 256, 160);
        ActivePasses.open(ours, new ActivePasses.PassDraws() {
            @Override
            public ActivePasses.Substitution substitute(CompiledRenderPipeline requested) {
                return ActivePasses.Substitution.skip();
            }

            @Override
            public GpuBufferSlice uniform(String name, GpuBufferSlice value) {
                return "DynamicTransforms".equals(name) && value.equals(camera) ? shadow : value;
            }
        });
        try {
            ActivePasses.Substitution skip = ActivePasses.substitute(ours, VanillaClonesTest.compiled());
            assertTrue(skip.skipped());
            assertNull(skip.pipeline());
            assertFalse(ActivePasses.Substitution.unchanged(VanillaClonesTest.compiled()).skipped());
            assertSame(shadow, ActivePasses.uniform(ours, "DynamicTransforms", camera));
            assertSame(camera, ActivePasses.uniform(ours, "Projection", camera), "other uniforms are kept");
            assertSame(camera, ActivePasses.uniform(other, "DynamicTransforms", camera), "other passes are untouched");
            assertNull(ActivePasses.uniform(ours, "DynamicTransforms", null));
        } finally {
            ActivePasses.close(ours);
        }
        assertSame(camera, ActivePasses.uniform(ours, "DynamicTransforms", camera), "a closed pass replaces nothing");
    }

    @Test
    void compiledPipelinesAreTracedBackByIdentity() {
        RenderPipeline vanilla = RenderPipelines.SOLID_TERRAIN_MULTIDRAW;
        CompiledRenderPipeline compiled = VanillaClonesTest.compiled();
        CompiledPipelineIndex.record(vanilla, compiled);
        CompiledPipelineIndex.record(vanilla, null);
        assertSame(vanilla, CompiledPipelineIndex.lookup(compiled));
        assertNull(CompiledPipelineIndex.lookup(VanillaClonesTest.compiled()));
        for (int i = 0; i < CompiledPipelineIndex.MAX_ENTRIES; i++) {
            CompiledPipelineIndex.record(vanilla, VanillaClonesTest.compiled());
        }
        assertNull(CompiledPipelineIndex.lookup(compiled), "a full index starts over");
    }
}
