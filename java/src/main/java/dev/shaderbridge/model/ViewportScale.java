package dev.shaderbridge.model;

/**
 * {@code scale.<prog>}: viewport scale and offset.
 *
 * @param scale   viewport scale
 * @param offsetX horizontal offset (fraction of the screen)
 * @param offsetY vertical offset (fraction of the screen)
 */
public record ViewportScale(float scale, float offsetX, float offsetY) {
}
