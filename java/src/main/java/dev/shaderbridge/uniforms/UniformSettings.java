package dev.shaderbridge.uniforms;

import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.PackSettings;
import dev.shaderbridge.model.ShadowSettings;

/**
 * The pack directives that change how builtin uniforms are computed.
 *
 * @param sunPathRotation       {@code sunPathRotation} in degrees
 * @param wetnessHalfLife       {@code wetnessHalflife} (rising wetness), tenths of a second
 * @param drynessHalfLife       {@code drynessHalflife} (falling wetness), tenths of a second
 * @param eyeBrightnessHalfLife {@code eyeBrightnessHalflife}, tenths of a second
 * @param centerDepthHalfLife   {@code centerDepthHalflife}, tenths of a second
 * @param shadowDistance        {@code shadowDistance}: half extent of the orthographic shadow projection
 * @param shadowNearPlane       shadow near plane ({@code -1}: see {@link ShadowMatrices#planes})
 * @param shadowFarPlane        shadow far plane ({@code -1}: see {@link ShadowMatrices#planes})
 * @param shadowIntervalSize    {@code shadowIntervalSize}
 * @param shadowFov             {@code shadowMapFov} for a legacy perspective shadow projection, or null
 * @param endFlashShadows       End flashes cast shadows (the shadow light follows the flash in the End)
 * @param unifiedProjection     vanilla terrain and LODs share one projection ending at the DH far plane
 * @param oldHandLight          the main hand reports the brighter light of both hands
 */
public record UniformSettings(
    float sunPathRotation,
    float wetnessHalfLife,
    float drynessHalfLife,
    float eyeBrightnessHalfLife,
    float centerDepthHalfLife,
    float shadowDistance,
    float shadowNearPlane,
    float shadowFarPlane,
    float shadowIntervalSize,
    Float shadowFov,
    boolean endFlashShadows,
    boolean unifiedProjection,
    boolean oldHandLight
) {
    /** The defaults of {@code sb_core::model::PackSettings} and {@code ShadowSettings}. */
    public static final UniformSettings DEFAULT = new UniformSettings(0, 600, 200, 10, 1, 160, 0.05f, 256, 2, null, false, false, true);

    /**
     * @param pipeline a compiled dimension pipeline
     * @return the settings of that pipeline
     */
    public static UniformSettings of(DimensionPipeline pipeline) {
        PackSettings s = pipeline.settings();
        ShadowSettings shadow = pipeline.targets().shadow();
        return new UniformSettings(s.sunPathRotation(), s.wetnessHalfLife(), s.drynessHalfLife(), s.eyeBrightnessHalfLife(), s.centerDepthHalfLife(),
            shadow.distance(), shadow.nearPlane(), shadow.farPlane(), shadow.intervalSize(), shadow.fov(), s.endFlashShadows(),
            pipeline.distantHorizons().unifiedProjection(), s.oldHandLight());
    }
}
