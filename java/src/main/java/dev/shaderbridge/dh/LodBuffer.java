package dev.shaderbridge.dh;

import com.mojang.renderpearl.api.buffers.GpuBuffer;

/**
 * One Distant Horizons LOD vertex buffer to draw: 16-byte vertices ({@code dh_terrain} profile)
 * positioned relative to the minimum corner of their render section, drawn as indexed triangles
 * with 32-bit indices. The buffers belong to Distant Horizons, which may close them at any time
 * on the render thread; check {@link GpuBuffer#isClosed()} before drawing.
 *
 * @param minX       minimum corner X of the section (block)
 * @param minY       minimum corner Y of the section (block)
 * @param minZ       minimum corner Z of the section (block)
 * @param width      width of the section in blocks (X and Z)
 * @param vertices   the vertex buffer
 * @param indices    the index buffer (32-bit indices)
 * @param indexCount number of indices to draw
 */
public record LodBuffer(int minX, int minY, int minZ, int width, GpuBuffer vertices, GpuBuffer indices, int indexCount) {
    /** @return whether Distant Horizons has not freed either buffer */
    public boolean drawable() {
        return !vertices.isClosed() && !indices.isClosed();
    }
}
