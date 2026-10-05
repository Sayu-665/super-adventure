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
 * @param blend        blend of the geometry this slot stands for (the vanilla state Iris keeps),
 *                     for programs with {@link Program#inheritBlend()} drawn without a vanilla
 *                     draw to inherit from; null = no blending (or absent in older models)
 * @param alphaTest    alpha test of this slot's draws ({@code alphaTest.<program>}, else Iris'
 *                     default for the geometry; {@code always} = none); null when absent (older
 *                     models: use the program's)
 */
public record GeometrySlot(int program, GeometryProgram resolvedFrom, Map<String, Integer> variants, BlendMode blend, AlphaTest alphaTest) {
    public GeometrySlot {
        variants = Copies.map(variants);
    }

    /**
     * A slot without its own blend and alpha test (as in models written before they existed).
     *
     * @param program      index into {@link DimensionPipeline#programs()}
     * @param resolvedFrom the program actually used (may be a fallback)
     * @param variants     the program's variants by draw profile
     */
    public GeometrySlot(int program, GeometryProgram resolvedFrom, Map<String, Integer> variants) {
        this(program, resolvedFrom, variants, null, null);
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

    /**
     * The blend and alpha test of drawing this slot's geometry with {@code program} (the slot's
     * program or one of its variants), as {@code sb_core::model::GeometrySlot::{blend_for,
     * alpha_test_ref}} compute them: the program's blend, or this slot's when the program inherits
     * it (it keeps {@code inheritBlend}, so a host replacing a vanilla draw can use that draw's
     * blend instead); and an alpha test whose reference is the slot's, or one the compiled
     * comparison always passes when the slot does not test alpha.
     *
     * @param program a program drawing this slot
     * @return the program to draw with
     */
    public Program drawn(Program program) {
        BlendMode blend = program.inheritBlend() ? this.blend : program.blend();
        return program.withDrawState(blend, program.inheritBlend(), alphaTestFor(program.alphaTest()));
    }

    private AlphaTest alphaTestFor(AlphaTest compiled) {
        AlphaTest effective = alphaTest != null ? alphaTest : compiled;
        if (effective == null || compiled == null) {
            return effective;
        }
        float reference = effective.func() == AlphaFunc.ALWAYS ? compiled.func().alwaysPassingReference().orElse(compiled.reference())
            : effective.reference();
        return new AlphaTest(compiled.func(), reference);
    }
}
