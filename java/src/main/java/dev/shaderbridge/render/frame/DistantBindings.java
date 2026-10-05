package dev.shaderbridge.render.frame;

import dev.shaderbridge.dh.DhHostBlocks;
import dev.shaderbridge.render.pipeline.BindingPlan;
import java.util.ArrayList;
import java.util.List;

/**
 * Which Distant Horizons host resources ({@code dh_terrain} profile) a LOD pipeline declares, so
 * that ShaderBridge, standing in for Distant Horizons' renderer, binds exactly those: the
 * {@code uLightMap} and {@code uBlockAtlas} samplers and the {@code vertSharedUniformBlock} and
 * {@code vertUniqueUniformBlock} blocks. A host resource it does not know and that has no pack
 * fallback cannot be bound, so the pipeline cannot draw (Mojang's draw validation requires every
 * declared descriptor).
 *
 * @param lightmap the pipeline declares {@code uLightMap}
 * @param atlas    the pipeline declares {@code uBlockAtlas}
 * @param shared   the pipeline declares {@code vertSharedUniformBlock}
 * @param unique   the pipeline declares {@code vertUniqueUniformBlock}
 * @param unknown  host descriptors nobody can bind
 */
record DistantBindings(boolean lightmap, boolean atlas, boolean shared, boolean unique, List<String> unknown) {
    /** The lightmap sampler of the {@code dh_terrain} profile. */
    static final String LIGHTMAP = "uLightMap";
    /** The LOD block atlas sampler of the {@code dh_terrain} profile. */
    static final String BLOCK_ATLAS = "uBlockAtlas";

    DistantBindings {
        unknown = List.copyOf(unknown);
    }

    /**
     * @param plan a LOD pipeline's binding plan
     * @return what it declares
     */
    static DistantBindings of(BindingPlan plan) {
        boolean lightmap = false;
        boolean atlas = false;
        boolean shared = false;
        boolean unique = false;
        List<String> unknown = new ArrayList<>();
        for (BindingPlan.Binding b : plan.bindings()) {
            if (!(b.source() instanceof BindingPlan.Source.Host host)) {
                continue;
            }
            switch (b.name()) {
                case LIGHTMAP -> lightmap = true;
                case BLOCK_ATLAS -> atlas = true;
                case DhHostBlocks.SHARED_BLOCK -> shared = true;
                case DhHostBlocks.UNIQUE_BLOCK -> unique = true;
                default -> {
                    if (host.fallback().isEmpty()) {
                        unknown.add(b.name());
                    }
                }
            }
        }
        return new DistantBindings(lightmap, atlas, shared, unique, unknown);
    }

    /** @return whether every host descriptor can be bound */
    boolean supported() {
        return unknown.isEmpty();
    }
}
