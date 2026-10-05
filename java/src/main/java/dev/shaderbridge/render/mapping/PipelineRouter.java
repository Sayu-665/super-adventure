package dev.shaderbridge.render.mapping;

import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import dev.shaderbridge.render.pipeline.ProfileVertexFormats;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Routes vanilla pipelines to pack programs: the {@link VanillaPipelineTable} entry of the
 * pipeline's location, kept only if the pipeline's vertex buffers actually carry the attributes
 * of the entry's draw profile (several pipelines share a location, e.g. the {@code GLINT_SPECIAL}
 * item pipelines, and mods may register pipelines under vanilla locations). A terrain pipeline
 * cloned with the extended chunk vertex format whose buffers lack an extension attribute falls
 * back to the basic terrain profile of its draw path, which reads the {@code BLOCK} attributes
 * only. Results are cached per pipeline instance. Thread-safe.
 */
public final class PipelineRouter {
    private final ProfileVertexFormats formats;
    private final Map<RenderPipeline, PipelineMapping> cache = new ConcurrentHashMap<>();

    /** @param formats vertex layouts of the draw profiles */
    public PipelineRouter(ProfileVertexFormats formats) {
        this.formats = formats;
    }

    /**
     * @param vanilla a vanilla render pipeline about to be drawn
     * @return the pack program to draw it with, or why it stays vanilla
     */
    public PipelineMapping route(RenderPipeline vanilla) {
        return cache.computeIfAbsent(vanilla, this::decide);
    }

    private PipelineMapping decide(RenderPipeline vanilla) {
        PipelineMapping mapping = VanillaPipelineTable.lookup(vanilla.getLocation());
        if (!(mapping instanceof PipelineMapping.Mapped mapped)) {
            return mapping;
        }
        List<String> profiles = new ArrayList<>();
        profiles.add(mapped.profile());
        VanillaPipelineTable.basicProfile(mapped.profile()).ifPresent(profiles::add);
        List<String> reasons = new ArrayList<>();
        for (String name : profiles) {
            Optional<List<VertexFormat>> profile = formats.bindings(name);
            if (profile.isEmpty()) {
                reasons.add("draw profile " + name + " has no known vertex layout");
                continue;
            }
            List<String> problems = ProfileVertexFormats.compatibility(profile.get(), vanilla.getVertexFormatBindings());
            if (problems.isEmpty()) {
                return name.equals(mapped.profile()) ? mapped : new PipelineMapping.Mapped(mapped.gbuffers(), mapped.shadow(), name);
            }
            reasons.add("its vertex format does not match draw profile " + name + ": " + String.join("; ", problems));
        }
        return new PipelineMapping.Vanilla(String.join("; ", reasons));
    }
}
