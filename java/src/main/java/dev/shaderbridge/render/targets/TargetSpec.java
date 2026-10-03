package dev.shaderbridge.render.targets;

import com.mojang.renderpearl.api.GpuFormat;
import java.util.Optional;

/**
 * A color render target to create, in a main/alt pair.
 *
 * @param index      colortex or shadowcolor index
 * @param shadow     a shadow color target
 * @param format     texture format (the renderable form of the pack's format)
 * @param width      width in pixels
 * @param height     height in pixels
 * @param mipLevels  mip levels (1 unless a program requests mipmaps of the target)
 * @param clear      cleared at the start of every frame
 * @param clearColor the pack's clear color, empty for {@link ClearColors#defaultClear the default}
 */
public record TargetSpec(int index, boolean shadow, GpuFormat format, int width, int height, int mipLevels, boolean clear, Optional<Rgba> clearColor) {
    /** @return {@code colortexN} or {@code shadowcolorN} */
    public String name() {
        return (shadow ? "shadowcolor" : "colortex") + index;
    }
}
