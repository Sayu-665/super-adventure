package dev.shaderbridge.dh;

/**
 * Distant Horizons settings ShaderBridge reads through its API every frame.
 *
 * @param lodChunks      LOD render distance in chunks ({@code chunkRenderDistance})
 * @param overdraw       overdraw prevention fraction, negative for automatic ({@code overdrawPreventionRadius})
 * @param lodOnly        the LOD-only debug mode ({@code lodOnlyMode})
 * @param earthCurvature curvature ratio ({@code earthCurvatureRatio}), 0 for a flat world
 */
public record DhSettings(int lodChunks, float overdraw, boolean lodOnly, int earthCurvature) {
}
