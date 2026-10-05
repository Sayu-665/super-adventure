package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.ViewportScale;

/**
 * The viewport of a composite-style draw, in pixels of its attachments: {@code scale.<program>}
 * maps the fullscreen quad into the rectangle at {@code (offsetX, offsetY) * size} of
 * {@code scale * size} (bloom tiles, half-resolution passes), as Iris sets the viewport and as the
 * headless executor does ({@code set_viewport}). Row 0 is GL's bottom row (no Y flip, ARCHITECTURE
 * §4), so the offsets apply from the first row as in GL. The scissor stays the whole attachment.
 *
 * @param x      left edge
 * @param y      first row
 * @param width  width (at least 1)
 * @param height height (at least 1)
 */
public record ViewportRect(float x, float y, float width, float height) {
    /**
     * @param scale  the program's viewport scale and offset
     * @param width  attachment width
     * @param height attachment height
     * @return the viewport; the whole attachment when the scale is the default or not usable
     *     (not finite, or not positive)
     */
    public static ViewportRect of(ViewportScale scale, int width, int height) {
        ViewportRect full = new ViewportRect(0, 0, width, height);
        if (scale == null || !Float.isFinite(scale.scale()) || !Float.isFinite(scale.offsetX()) || !Float.isFinite(scale.offsetY())
            || scale.scale() <= 0) {
            return full;
        }
        return new ViewportRect(scale.offsetX() * width, scale.offsetY() * height, Math.max(1.0f, scale.scale() * width),
            Math.max(1.0f, scale.scale() * height));
    }

    /**
     * @param width  attachment width
     * @param height attachment height
     * @return whether this viewport covers exactly the whole attachment
     */
    public boolean covers(int width, int height) {
        return x == 0 && y == 0 && this.width == width && this.height == height;
    }
}
