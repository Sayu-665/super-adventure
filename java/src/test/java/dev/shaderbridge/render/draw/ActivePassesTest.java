package dev.shaderbridge.render.draw;

import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;

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
            assertSame(replacement, ActivePasses.substitute(ours, requested).pipeline());
            assertNull(ActivePasses.substitute(other, requested));
            ActivePasses.close(other);
            assertSame(replacement, ActivePasses.substitute(ours, requested).pipeline(), "closing another pass changes nothing");
        } finally {
            ActivePasses.close(ours);
        }
        assertNull(ActivePasses.substitute(ours, requested));
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
