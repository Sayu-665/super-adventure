package dev.shaderbridge.model;

import java.util.List;

/**
 * One color render target.
 *
 * @param index           colortex or shadowcolor index
 * @param format          texture format
 * @param clear           cleared every frame
 * @param clearColor      explicit clear color (RGBA), or null for the default
 * @param mipmapPrograms  programs that request mipmaps for this target before running
 * @param size            size relative to the screen or absolute
 * @param used            a program reads or writes the target
 */
public record ColorTarget(
    int index,
    TextureFormat format,
    boolean clear,
    List<Float> clearColor,
    List<Integer> mipmapPrograms,
    TargetSize size,
    boolean used
) {
    public ColorTarget {
        Copies.required(format, "format");
        clearColor = clearColor == null ? null : Copies.list(clearColor);
        mipmapPrograms = Copies.list(mipmapPrograms);
        Copies.required(size, "size");
    }
}
