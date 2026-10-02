package dev.shaderbridge.uniforms;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBuffer;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import dev.shaderbridge.model.BlockLayout;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;
import java.util.List;

/**
 * The {@code sb_Draw} block, one slice per draw. Each draw's block is written into a CPU staging
 * page at the next slot (aligned to the device's uniform offset alignment); {@link #flush}
 * uploads the written slots before the render pass that uses them, because
 * {@code CommandEncoder.writeToBuffer} is not allowed inside a pass. Every frame in flight owns
 * its own pages, which grow on demand.
 */
public final class DrawUniforms implements AutoCloseable {
    private static final int SLOTS_PER_PAGE = 256;

    private final List<FrameUniforms.Binding> builtins;
    private final int blockSize;
    private final int stride;
    private final UniformWriter out = new UniformWriter();
    private final List<List<Page>> frames = new ArrayList<>();
    private int frame = -1;
    private int page;
    private int slot;

    /** A GPU buffer of {@link #SLOTS_PER_PAGE} slots and its staging copy. */
    private static final class Page {
        final GpuBuffer buffer;
        final ByteBuffer staging;
        int flushed;

        Page(GpuBuffer buffer, ByteBuffer staging) {
            this.buffer = buffer;
            this.staging = staging;
        }
    }

    /**
     * @param layout    the pipeline's {@code sb_Draw} layout
     * @param alignment the device's minimum uniform buffer offset alignment
     */
    public DrawUniforms(BlockLayout layout, int alignment) {
        this.blockSize = Math.max(16, layout.size());
        this.stride = alignUp(blockSize, Math.max(16, alignment));
        this.builtins = FrameUniforms.bind(layout, new Std140Writer(ByteBuffer.allocate(blockSize)));
        for (int i = 0; i < GpuBufferRing.FRAMES_IN_FLIGHT; i++) {
            frames.add(new ArrayList<>());
        }
    }

    private static int alignUp(int value, int alignment) {
        return (value + alignment - 1) / alignment * alignment;
    }

    /** Starts a frame: the slots of the oldest frame in flight are reused. */
    public void beginFrame() {
        frame = (frame + 1) % GpuBufferRing.FRAMES_IN_FLIGHT;
        page = 0;
        slot = 0;
        for (Page p : frames.get(frame)) {
            p.flushed = 0;
        }
    }

    /**
     * Writes the block of one draw into the next slot.
     *
     * @param frameState the frame
     * @param draw       the draw's values ({@link DrawState#reset} first for defaults)
     * @return the slice to bind as {@code sb_Draw}; valid after the next {@link #flush}
     */
    public GpuBufferSlice push(FrameState frameState, DrawState draw) {
        if (frame < 0) {
            throw new IllegalStateException("beginFrame() was not called");
        }
        if (slot == SLOTS_PER_PAGE) {
            page++;
            slot = 0;
        }
        List<Page> pages = frames.get(frame);
        if (page == pages.size()) {
            String label = "ShaderBridge sb_Draw " + frame + "/" + page;
            GpuBuffer buffer = RenderSystem.getDevice().createBuffer(() -> label, GpuBuffer.USAGE_UNIFORM | GpuBuffer.USAGE_COPY_DST, (long) stride * SLOTS_PER_PAGE);
            pages.add(new Page(buffer, ByteBuffer.allocateDirect(stride * SLOTS_PER_PAGE).order(ByteOrder.LITTLE_ENDIAN)));
        }
        Page target = pages.get(page);
        int offset = slot * stride;
        fill(frameState, draw, target.staging.slice(offset, blockSize).order(ByteOrder.LITTLE_ENDIAN));
        slot++;
        return target.buffer.slice(offset, blockSize);
    }

    /**
     * Writes one draw block.
     *
     * @param frameState the frame
     * @param draw       the draw
     * @param dst        a buffer of at least the block size
     */
    void fill(FrameState frameState, DrawState draw, ByteBuffer dst) {
        Std140Writer writer = new Std140Writer(dst);
        writer.clear(0, blockSize);
        draw.update();
        for (FrameUniforms.Binding binding : builtins) {
            binding.builtin().provider().write(frameState, draw, out.bind(writer, binding.offset(), binding.member().ty()));
        }
    }

    /**
     * Uploads every slot written since the last flush. Call outside render passes.
     *
     * @param encoder the frame's command encoder
     */
    public void flush(CommandEncoder encoder) {
        if (frame < 0) {
            return;
        }
        List<Page> pages = frames.get(frame);
        for (int i = 0; i <= Math.min(page, pages.size() - 1); i++) {
            Page p = pages.get(i);
            int written = (i < page ? SLOTS_PER_PAGE : slot) * stride;
            if (written > p.flushed) {
                encoder.writeToBuffer(p.buffer.slice(p.flushed, written - p.flushed), p.staging.slice(p.flushed, written - p.flushed));
                p.flushed = written;
            }
        }
    }

    @Override
    public void close() {
        for (List<Page> pages : frames) {
            pages.forEach(p -> p.buffer.close());
            pages.clear();
        }
    }
}
