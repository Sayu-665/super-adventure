package dev.shaderbridge.uniforms;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBuffer;

/**
 * One uniform buffer per frame in flight, created on first use, so that the CPU never overwrites
 * a buffer the GPU may still read.
 */
final class GpuBufferRing implements AutoCloseable {
    /** Frames the GPU may lag behind, as Minecraft's own ring buffers assume. */
    static final int FRAMES_IN_FLIGHT = 3;

    private final String label;
    private final long size;
    private final GpuBuffer[] buffers = new GpuBuffer[FRAMES_IN_FLIGHT];
    private int index = FRAMES_IN_FLIGHT - 1;

    GpuBufferRing(String label, long size) {
        this.label = label;
        this.size = size;
    }

    /** @return the buffer of the next frame */
    GpuBuffer next() {
        index = (index + 1) % FRAMES_IN_FLIGHT;
        if (buffers[index] == null || buffers[index].isClosed()) {
            String name = label + " #" + index;
            buffers[index] = RenderSystem.getDevice().createBuffer(() -> name, GpuBuffer.USAGE_UNIFORM | GpuBuffer.USAGE_COPY_DST, size);
        }
        return buffers[index];
    }

    @Override
    public void close() {
        for (int i = 0; i < buffers.length; i++) {
            if (buffers[i] != null) {
                buffers[i].close();
                buffers[i] = null;
            }
        }
    }
}
