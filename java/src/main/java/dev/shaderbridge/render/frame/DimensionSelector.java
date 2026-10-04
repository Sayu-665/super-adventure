package dev.shaderbridge.render.frame;

import dev.shaderbridge.model.CompiledPack;
import dev.shaderbridge.model.DimensionPipeline;
import java.util.Optional;

/**
 * Picks the dimension pipeline of a pack for a Minecraft dimension, as Iris does: the world folder
 * assigned to the dimension's id ({@code dimension.properties}, or the default {@code world0},
 * {@code world-1}, {@code world1} folders), else the folder holding the wildcard ({@code *} or
 * {@code *:*}: {@code world0} without {@code dimension.properties}, or the pack root). A dimension
 * matching neither renders without shaders.
 */
public final class DimensionSelector {
    private DimensionSelector() {
    }

    /**
     * @param pack        the compiled pack
     * @param dimensionId the dimension's id, e.g. {@code minecraft:the_nether}
     * @return its pipeline, if the pack has one for it
     */
    public static Optional<DimensionPipeline> select(CompiledPack pack, String dimensionId) {
        return pack.dimensions().stream().filter(d -> d.dimensionIds().contains(dimensionId)).findFirst()
            .or(() -> pack.dimensions().stream().filter(d -> d.dimensionIds().contains("*") || d.dimensionIds().contains("*:*")).findFirst());
    }
}
