package dev.shaderbridge.render.frame;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.buffers.GpuBuffer;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.api.textures.GpuTexture;
import dev.shaderbridge.model.DepthMode;

/**
 * Reads the depth at the centre of the screen back from the GPU for {@code centerDepthSmooth}.
 * Once per frame, after the opaque geometry (where the headless executor samples it, as Iris does
 * at {@code beginHand}), the centre texel of Minecraft's depth buffer is copied into a small
 * host-readable buffer; the copy completes asynchronously and its value feeds the frame state a
 * frame or two later, where it is smoothed with the pack's {@code centerDepthHalflife} as before.
 * A few buffers are cycled, so a sample is skipped only when the GPU lags several frames behind.
 * Render thread only (the copy callbacks run there).
 */
final class CenterDepthProbe implements AutoCloseable {
    /** Samples in flight at most. */
    static final int SLOTS = 3;
    /** Bytes per sample buffer (enough for any depth texel). */
    private static final int BUFFER_SIZE = 8;

    private final GpuDevice device;
    private final DepthMode depthMode;
    private final GpuBuffer[] buffers = new GpuBuffer[SLOTS];
    private final boolean[] busy = new boolean[SLOTS];
    private int next;
    private float latest = Float.NaN;
    private boolean closed;

    /**
     * @param device    the GPU device
     * @param depthMode the pack's depth convention
     */
    CenterDepthProbe(GpuDevice device, DepthMode depthMode) {
        this.device = device;
        this.depthMode = depthMode;
    }

    /**
     * Copies the centre texel of a depth texture for reading back. Call outside render passes.
     *
     * @param encoder a command encoder
     * @param depth   Minecraft's depth texture
     */
    void sample(CommandEncoder encoder, GpuTexture depth) {
        GpuFormat format = depth.getFormat();
        int slot = next;
        if (closed || !CenterDepth.readable(format) || busy[slot]) {
            return;
        }
        if (buffers[slot] == null) {
            int index = slot;
            buffers[slot] = device.createBuffer(() -> "ShaderBridge centre depth " + index, GpuBuffer.USAGE_MAP_READ | GpuBuffer.USAGE_COPY_DST,
                BUFFER_SIZE);
        }
        busy[slot] = true;
        next = (next + 1) % SLOTS;
        GpuBuffer buffer = buffers[slot];
        encoder.copyTextureToBuffer(depth, buffer, 0, () -> read(slot, buffer, format), 0, depth.getWidth(0) / 2, depth.getHeight(0) / 2, 1, 1);
    }

    private void read(int slot, GpuBuffer buffer, GpuFormat format) {
        busy[slot] = false;
        if (closed || buffer.isClosed()) {
            return;
        }
        try (GpuBufferSlice.MappedView view = buffer.map(true, false)) {
            float depth = CenterDepth.decode(view.data(), format, depthMode);
            if (Float.isFinite(depth)) {
                latest = depth;
            }
        }
    }

    /** @return the latest centre depth read back (GL window depth), or NaN before the first */
    float latest() {
        return latest;
    }

    @Override
    public void close() {
        closed = true;
        for (int i = 0; i < SLOTS; i++) {
            if (buffers[i] != null) {
                buffers[i].close();
                buffers[i] = null;
            }
        }
    }
}
