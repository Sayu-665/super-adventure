package dev.shaderbridge.render.frame;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import dev.shaderbridge.uniforms.DrawState;
import dev.shaderbridge.uniforms.FrameState;
import java.util.HashMap;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Set;

/**
 * The {@code sb_Draw} block of every kind of draw ({@link DrawKey}) in one frame. Buffers may only
 * be written outside render passes, but which kinds of draws occur is only known inside them, when
 * Minecraft binds its pipelines. So every kind seen so far gets its block written before the
 * frame's first pass ({@link #prepare}), and a kind seen for the first time draws with the frame's
 * default block (camera or shadow matrices, no alpha test, no blending) until the next frame.
 * Render thread only.
 */
public final class DrawSlots {
    /** Writes one block and returns the slice it will occupy once uploaded. */
    @FunctionalInterface
    public interface BlockWriter {
        /**
         * @param frame the frame
         * @param draw  the draw's values
         * @return the slice to bind as {@code sb_Draw}
         */
        GpuBufferSlice push(FrameState frame, DrawState draw);
    }

    /** At most this many kinds are remembered (a safety net against unbounded keys). */
    static final int MAX_KEYS = 4096;

    private final Set<DrawKey> known = new LinkedHashSet<>();
    private final Map<DrawKey, GpuBufferSlice> slices = new HashMap<>();
    private final DrawState scratch = new DrawState();
    private GpuBufferSlice cameraDefault;
    private GpuBufferSlice shadowDefault;

    /**
     * Writes the blocks of every known kind and the two defaults. Call before the frame's first
     * render pass, then upload the written blocks.
     *
     * @param frame  the frame, {@linkplain FrameState#update() updated}
     * @param writer the block writer of the frame
     */
    public void prepare(FrameState frame, BlockWriter writer) {
        slices.clear();
        cameraDefault = write(frame, writer, new DrawKey("", RenderStages.NONE, false, 0, DrawKey.blendFunc(null)));
        shadowDefault = write(frame, writer, new DrawKey("", RenderStages.NONE, true, 0, DrawKey.blendFunc(null)));
        for (DrawKey key : known) {
            slices.put(key, write(frame, writer, key));
        }
    }

    /**
     * @param key a kind of draw
     * @return its block, or the frame's default block when the kind is new (it gets its own block
     *     from the next frame on)
     * @throws IllegalStateException before the first {@link #prepare}
     */
    public GpuBufferSlice slice(DrawKey key) {
        if (cameraDefault == null) {
            throw new IllegalStateException("prepare() was not called");
        }
        GpuBufferSlice slice = slices.get(key);
        if (slice != null) {
            return slice;
        }
        if (known.size() < MAX_KEYS) {
            known.add(key);
        }
        return key.shadow() ? shadowDefault : cameraDefault;
    }

    private GpuBufferSlice write(FrameState frame, BlockWriter writer, DrawKey key) {
        key.apply(frame, scratch);
        return writer.push(frame, scratch);
    }
}
