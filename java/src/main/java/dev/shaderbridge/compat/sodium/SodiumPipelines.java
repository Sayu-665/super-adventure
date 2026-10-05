package dev.shaderbridge.compat.sodium;

import dev.shaderbridge.render.mapping.PipelineMapping;
import dev.shaderbridge.render.mapping.VanillaPipelineTable;
import java.util.Optional;
import java.util.Set;
import net.minecraft.resources.Identifier;

/**
 * Which pack programs draw Sodium's terrain. Sodium 0.9 builds one render pipeline per terrain
 * pass, located at {@code sodium:<path of the pipeline ChunkSectionLayer.pipeline(false) returns>}
 * ({@code ShaderChunkRenderer.createShader}): {@code sodium:pipeline/solid_terrain},
 * {@code sodium:pipeline/cutout_terrain} and {@code sodium:pipeline/translucent_terrain}, or, when
 * that method returns another pipeline (ShaderBridge's extended vanilla terrain clones, located at
 * {@code shaderbridge:extended_terrain/minecraft/pipeline/...}), a path ending in the same
 * {@code pipeline/<layer>_terrain}. Each is
 * drawn with the programs of the vanilla terrain pipeline it stands for (so Sodium and vanilla
 * terrain shade alike: {@code gbuffers_terrain_solid}, {@code gbuffers_terrain_cutout} and
 * {@code gbuffers_water}, and {@code shadow_solid}, {@code shadow_cutout} and
 * {@code shadow_water} in the shadow pass), compiled for the {@value #PROFILE} draw profile.
 * Sodium's order-independent-transparency pipelines are located in the {@code minecraft}
 * namespace ({@code pipeline/oit_*_sodium_terrain}) and stay with the vanilla table, which keeps
 * them vanilla (packs force classic transparency).
 */
public final class SodiumPipelines {
    /** Namespace of Sodium's terrain pipeline locations. */
    public static final String NAMESPACE = "sodium";
    /** The draw profile of Sodium's terrain vertices ({@code crates/sb-transform/profiles/sodium_terrain.toml}). */
    public static final String PROFILE = "sodium_terrain";
    /** Location paths of Sodium's terrain pipelines: those of the vanilla pipelines of its passes. */
    public static final Set<String> TERRAIN_PATHS = Set.of("pipeline/solid_terrain", "pipeline/cutout_terrain", "pipeline/translucent_terrain");

    private SodiumPipelines() {
    }

    /**
     * @param location a render pipeline location
     * @return the mapping of a Sodium pipeline, or empty for pipelines of other namespaces
     */
    public static Optional<PipelineMapping> lookup(Identifier location) {
        if (!NAMESPACE.equals(location.getNamespace())) {
            return Optional.empty();
        }
        return Optional.of(lookupPath(location.getPath()));
    }

    /**
     * @param path the location path of a pipeline in the {@value #NAMESPACE} namespace
     * @return its mapping: the vanilla terrain pipeline's programs with the {@value #PROFILE}
     *     profile, or vanilla rendering for other Sodium pipelines
     */
    public static PipelineMapping lookupPath(String path) {
        String terrain = terrainPath(path);
        if (!TERRAIN_PATHS.contains(terrain)) {
            return new PipelineMapping.Vanilla("a Sodium pipeline ShaderBridge does not shade");
        }
        if (VanillaPipelineTable.lookupPath(terrain) instanceof PipelineMapping.Mapped vanilla) {
            return new PipelineMapping.Mapped(vanilla.gbuffers(), vanilla.shadow(), PROFILE);
        }
        return new PipelineMapping.Vanilla("the vanilla pipeline " + terrain + " is not shaded");
    }

    /**
     * @param path a Sodium pipeline's location path
     * @return the vanilla pipeline path it ends with ({@code pipeline/...}), or the path itself
     */
    static String terrainPath(String path) {
        int start = path.lastIndexOf("pipeline/");
        return start > 0 && path.charAt(start - 1) == '/' ? path.substring(start) : path;
    }
}
