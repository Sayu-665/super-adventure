package dev.shaderbridge.dh;

import dev.shaderbridge.model.DhPipeline;
import dev.shaderbridge.model.DhStrategy;
import dev.shaderbridge.model.ShadowSettings;

/**
 * How ShaderBridge draws Distant Horizons' LODs for a dimension pipeline (ARCHITECTURE §9), the
 * same conventions the headless executor ({@code crates/sb-runtime}) follows:
 *
 * <ul>
 *   <li>{@link #NATIVE}: the pack's own {@code dh_*} programs. LODs are drawn everywhere Distant
 *   Horizons keeps them, the vanilla area included (as Distant Horizons draws them for an Iris
 *   pack; the programs hide them near the camera themselves), into a depth buffer of their own
 *   that becomes {@code dhDepthTex0}/{@code dhDepthTex1}, with {@code dhProjection} spanning
 *   Distant Horizons' near and far planes.</li>
 *   <li>{@link #SYNTHESIZED}: {@code dh_*} programs generated from {@code gbuffers_*}. They know
 *   nothing of LODs, so LODs share Minecraft's depth buffer and one projection that reaches the
 *   Distant Horizons far plane, and LOD sections lying entirely inside the vanilla area are not
 *   drawn (they would poke through the vanilla terrain; sections crossing its edge are drawn, so
 *   LODs and vanilla terrain overlap there rather than leave a gap).</li>
 *   <li>{@link #OFF}: no LODs are drawn.</li>
 * </ul>
 */
public enum DhMode {
    /** No LODs: Distant Horizons is absent or does not render, or the pack was compiled without it. */
    OFF,
    /** The pack's own programs: separate DH depth, LODs everywhere, DH near and far planes. */
    NATIVE,
    /** Synthesized programs: shared depth, one unified projection, LODs beyond the vanilla area only. */
    SYNTHESIZED;

    /**
     * Chooses the mode of a dimension pipeline. A synthesized pipeline without the unified
     * projection (the compiler always sets it) is drawn like a native one.
     *
     * @param pipeline    the dimension's Distant Horizons configuration
     * @param dhRendering Distant Horizons is installed, hooked and has rendering enabled
     * @return the mode
     */
    public static DhMode select(DhPipeline pipeline, boolean dhRendering) {
        if (!dhRendering || pipeline.strategy() == DhStrategy.DISABLED) {
            return OFF;
        }
        return pipeline.strategy() == DhStrategy.SYNTHESIZED && pipeline.unifiedProjection() ? SYNTHESIZED : NATIVE;
    }

    /**
     * Whether LODs cast shadows: the dimension draws {@code dh_shadow} and the pack's shadow
     * pass is on with {@code shadowDhEnabled} (Iris' {@code dhShadow.enabled}).
     *
     * @param pipeline the dimension's Distant Horizons configuration
     * @param shadow   the pack's shadow settings
     * @return whether the shadow pass draws LODs (when Distant Horizons renders)
     */
    public static boolean castsShadows(DhPipeline pipeline, ShadowSettings shadow) {
        return pipeline.strategy() != DhStrategy.DISABLED && pipeline.shadowEnabled() && shadow.enabled() && shadow.dhShadowEnabled();
    }

    /** @return whether LODs are drawn */
    public boolean drawsLods() {
        return this != OFF;
    }

    /** @return whether LODs have a depth buffer of their own ({@code dhDepthTex0}) */
    public boolean separateDepth() {
        return this == NATIVE;
    }

    /**
     * @return whether vanilla terrain and LODs share one projection ending at the Distant
     *     Horizons far plane (and LODs inside the vanilla area are skipped)
     */
    public boolean unifiedProjection() {
        return this == SYNTHESIZED;
    }
}
