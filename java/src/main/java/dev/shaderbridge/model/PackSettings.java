package dev.shaderbridge.model;

import java.util.Map;

/**
 * Functional {@code shaders.properties} keys and global const directives
 * ({@code sb_core::model::PackSettings}). Boolean and string fields mirror the Iris directive of the
 * same name; {@link #raw()} keeps every functional key for values not modelled here.
 *
 * @param clouds                   cloud mode ({@code default}, {@code fast}, {@code fancy}, {@code off})
 * @param dhClouds                 Distant Horizons cloud mode
 * @param oldHandLight             the main hand also uses the off-hand light when brighter
 * @param dynamicHandLight         held light sources light the surroundings
 * @param oldLighting              vanilla directional shading is applied
 * @param separateAo               ambient occlusion goes to the vertex alpha instead of the color
 * @param underwaterOverlay        draw the vanilla underwater overlay
 * @param vignette                 draw the vanilla vignette
 * @param sun                      draw the sun
 * @param moon                     draw the moon
 * @param stars                    draw the stars
 * @param sky                      draw the sky
 * @param weather                  draw rain and snow
 * @param weatherParticles         spawn weather particles
 * @param rainDepth                weather writes depth
 * @param beaconBeamDepth          beacon beams write depth
 * @param frustumCulling           frustum culling is enabled
 * @param occlusionCulling         occlusion culling is enabled
 * @param separateEntityDraws      entities are drawn one by one
 * @param allowConcurrentCompute   compute passes may overlap
 * @param particlesOrdering        when particles are drawn relative to deferred passes
 * @param supportsColorCorrection  the pack handles color correction itself
 * @param skipAllRendering         vanilla world rendering is skipped
 * @param voxelizeLightBlocks      light blocks are rendered into the shadow pass for voxelization
 * @param endFlashShadows          End flashes cast shadows
 * @param fallbackTex              colortex index unshaded geometry writes to
 * @param backFace                 back-face rendering overrides per render layer
 * @param sunPathRotation          {@code sunPathRotation} in degrees
 * @param ambientOcclusionLevel    {@code ambientOcclusionLevel}
 * @param wetnessHalfLife          {@code wetnessHalflife}, in tenths of a second
 * @param drynessHalfLife          {@code drynessHalflife}, in tenths of a second
 * @param eyeBrightnessHalfLife    {@code eyeBrightnessHalflife}, in tenths of a second
 * @param centerDepthHalfLife      {@code centerDepthHalflife}, in tenths of a second
 * @param raw                      every raw functional key and value
 */
public record PackSettings(
    String clouds,
    String dhClouds,
    boolean oldHandLight,
    boolean dynamicHandLight,
    boolean oldLighting,
    boolean separateAo,
    boolean underwaterOverlay,
    boolean vignette,
    boolean sun,
    boolean moon,
    boolean stars,
    boolean sky,
    boolean weather,
    boolean weatherParticles,
    boolean rainDepth,
    boolean beaconBeamDepth,
    boolean frustumCulling,
    boolean occlusionCulling,
    boolean separateEntityDraws,
    boolean allowConcurrentCompute,
    String particlesOrdering,
    boolean supportsColorCorrection,
    boolean skipAllRendering,
    boolean voxelizeLightBlocks,
    boolean endFlashShadows,
    int fallbackTex,
    Map<String, Boolean> backFace,
    float sunPathRotation,
    float ambientOcclusionLevel,
    float wetnessHalfLife,
    float drynessHalfLife,
    float eyeBrightnessHalfLife,
    float centerDepthHalfLife,
    Map<String, String> raw
) {
    public PackSettings {
        backFace = Copies.map(backFace);
        raw = Copies.map(raw);
    }
}
