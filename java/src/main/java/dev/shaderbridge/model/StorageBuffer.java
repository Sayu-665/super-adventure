package dev.shaderbridge.model;

import java.util.List;

/**
 * A shader storage buffer ({@code bufferObject.<index>}).
 *
 * @param index    buffer index
 * @param size     size in bytes, or bytes per pixel when {@code relative} is set
 * @param relative screen-relative size factors, or null
 * @param file     initial content file (relative to {@code shaders/}), or null
 */
public record StorageBuffer(int index, long size, List<Float> relative, String file) {
    public StorageBuffer {
        relative = relative == null ? null : Copies.list(relative);
    }
}
