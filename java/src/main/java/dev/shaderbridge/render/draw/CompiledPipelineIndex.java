package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import java.util.IdentityHashMap;
import java.util.Map;

/**
 * Which {@link RenderPipeline} a compiled pipeline was compiled from. A render pass only sees
 * compiled pipelines, but routing a draw to a pack program needs the vanilla pipeline's location,
 * vertex layout and bind groups. Every pipeline vanilla code binds comes from
 * {@code RenderSystem.getCompiledPipeline}, whose results are recorded here (by identity).
 * Pipeline caches are replaced on resource reloads, so the index forgets everything once it grows
 * past {@link #MAX_ENTRIES}; live pipelines are recorded again on their next lookup.
 */
public final class CompiledPipelineIndex {
    /** Entries kept before the index is reset (vanilla has a few hundred pipelines). */
    static final int MAX_ENTRIES = 8192;

    private static final Map<CompiledRenderPipeline, RenderPipeline> INDEX = new IdentityHashMap<>();

    private CompiledPipelineIndex() {
    }

    /**
     * @param pipeline a pipeline
     * @param compiled what it compiled to (ignored when null)
     */
    public static synchronized void record(RenderPipeline pipeline, CompiledRenderPipeline compiled) {
        if (compiled == null) {
            return;
        }
        if (INDEX.size() >= MAX_ENTRIES && !INDEX.containsKey(compiled)) {
            INDEX.clear();
        }
        INDEX.put(compiled, pipeline);
    }

    /**
     * @param compiled a compiled pipeline
     * @return the pipeline it was compiled from, or null if it did not come from
     *     {@code RenderSystem.getCompiledPipeline}
     */
    public static synchronized RenderPipeline lookup(CompiledRenderPipeline compiled) {
        return INDEX.get(compiled);
    }
}
