package dev.shaderbridge.render.frame;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import dev.shaderbridge.uniforms.DrawState;
import dev.shaderbridge.uniforms.FrameState;
import java.util.HashMap;
import java.util.Map;

/**
 * The {@code sb_Draw} block of every kind of draw ({@link DrawKey}) in one frame. Which kinds of
 * draws occur is only known inside render passes, when Minecraft binds its pipelines; the block of
 * a kind is written the first time the kind is drawn in a frame, through a writer that may write
 * inside a pass ({@code DrawUniforms} writes host-visible pages), and reused by the frame's later
 * draws of the kind. Render thread only.
 */
public final class DrawSlots {
    /** Writes one block and returns the slice it occupies; callable inside render passes. */
    @FunctionalInterface
    public interface BlockWriter {
        /**
         * @param frame the frame
         * @param draw  the draw's values
         * @return the slice to bind as {@code sb_Draw}
         */
        GpuBufferSlice push(FrameState frame, DrawState draw);
    }

    /** At most this many kinds get blocks of their own per frame (a safety net against unbounded keys). */
    static final int MAX_KEYS = 4096;

    private final Map<DrawKey, GpuBufferSlice> slices = new HashMap<>();
    private final DrawState scratch = new DrawState();
    private FrameState frame;
    private BlockWriter writer;
    private GpuBufferSlice cameraDefault;
    private GpuBufferSlice shadowDefault;

    /**
     * Starts a frame: forgets the previous frame's blocks and writes the two default blocks
     * (camera and shadow matrices, no alpha test, no blending), used for kinds past
     * {@link #MAX_KEYS}.
     *
     * @param frame  the frame, {@linkplain FrameState#update() updated}
     * @param writer the block writer of the frame
     */
    public void prepare(FrameState frame, BlockWriter writer) {
        this.frame = frame;
        this.writer = writer;
        slices.clear();
        cameraDefault = write(new DrawKey("", RenderStages.NONE, false, 0, DrawKey.blendFunc(null), AlbedoSize.NONE));
        shadowDefault = write(new DrawKey("", RenderStages.NONE, true, 0, DrawKey.blendFunc(null), AlbedoSize.NONE));
    }

    /**
     * @param key a kind of draw
     * @return its block in this frame, written now if the kind was not drawn before in this frame
     * @throws IllegalStateException before the first {@link #prepare}
     */
    public GpuBufferSlice slice(DrawKey key) {
        if (writer == null) {
            throw new IllegalStateException("prepare() was not called");
        }
        GpuBufferSlice slice = slices.get(key);
        if (slice != null) {
            return slice;
        }
        if (slices.size() >= MAX_KEYS) {
            return key.shadow() ? shadowDefault : cameraDefault;
        }
        slice = write(key);
        slices.put(key, slice);
        return slice;
    }

    private GpuBufferSlice write(DrawKey key) {
        key.apply(frame, scratch);
        return writer.push(frame, scratch);
    }
}
