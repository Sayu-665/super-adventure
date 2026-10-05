package dev.shaderbridge.model;

import java.util.Map;

/**
 * Resolved program of a geometry type.
 *
 * @param program      index into {@link DimensionPipeline#programs()} of the program drawing this
 *                     geometry with the slot's default draw profile
 * @param resolvedFrom the program actually used (may be a fallback)
 * @param variants     the same program (same name and kind) translated for other draw profiles:
 *                     draw profile id to index into {@link DimensionPipeline#programs()}, each with
 *                     the {@code use_alt} of this slot's pass. Lists the variants other slots needed
 *                     and the host's {@code profile_overrides}, not variants compiled later on demand
 *                     ({@code compileVariant}). serde {@code #[serde(default)]}: empty when absent.
 */
public record GeometrySlot(int program, GeometryProgram resolvedFrom, Map<String, Integer> variants) {
    public GeometrySlot {
        variants = Copies.map(variants);
    }

    /**
     * A slot without variants.
     *
     * @param program      index into {@link DimensionPipeline#programs()}
     * @param resolvedFrom the program actually used (may be a fallback)
     */
    public GeometrySlot(int program, GeometryProgram resolvedFrom) {
        this(program, resolvedFrom, Map.of());
    }
}
