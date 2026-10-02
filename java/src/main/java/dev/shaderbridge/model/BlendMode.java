package dev.shaderbridge.model;

/**
 * Separate color and alpha blend factors with ADD equations.
 *
 * @param srcColor source color factor
 * @param dstColor destination color factor
 * @param srcAlpha source alpha factor
 * @param dstAlpha destination alpha factor
 */
public record BlendMode(BlendFactor srcColor, BlendFactor dstColor, BlendFactor srcAlpha, BlendFactor dstAlpha) {
}
