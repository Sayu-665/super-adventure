package dev.shaderbridge.uniforms;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBuffer;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.commands.GpuFence;
import dev.shaderbridge.model.BlockLayout;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;
import java.util.List;

/**
 * The {@code sb_Draw} block, one slice per draw. Each draw's block is written into the next slot
 * of a page (aligned to the device's uniform offset alignment) through a mapping of the page:
 * pages are host-visible ({@code USAGE_MAP_WRITE}), so a block can be written at any time, inside
 * a render pass too, and is bound right away. (A copy command, {@code writeToBuffer}, is not
 * allowed inside a pass; that is why the blocks of draw kinds first seen inside a pass used to
 * take effect a frame late.) Every frame in flight owns its own pages, which grow on demand; the
 * pages of a frame are reused only once the GPU has finished that frame (a fence per frame).
 */
public final class DrawUniforms implements AutoCloseable {
    private static final int SLOTS_PER_PAGE = 256;
    /** Pages are uniform buffers the CPU writes through a mapping. */
    static final int PAGE_USAGE = GpuBuffer.USAGE_UNIFORM | GpuBuffer.USAGE_MAP_WRITE;

    private final List<FrameUniforms.Binding> builtins;
    /** The block with every constant initializer applied; each draw starts from a copy. */
    private final ByteBuffer template;
    private final ByteBuffer scratch;
    private final int blockSize;
    private final int stride;
    private final UniformWriter out = new UniformWriter();
    private final List<List<GpuBuffer>> frames = new ArrayList<>();
    private final GpuFence[] fences = new GpuFence[GpuBufferRing.FRAMES_IN_FLIGHT];
    private int frame = -1;
    private int page;
    private int slot;

    /**
     * @param layout    the pipeline's {@code sb_Draw} layout
     * @param alignment the device's minimum uniform buffer offset alignment
     */
    public DrawUniforms(BlockLayout layout, int alignment) {
        this.blockSize = Math.max(16, layout.size());
        this.stride = alignUp(blockSize, Math.max(16, alignment));
        this.template = ByteBuffer.allocate(blockSize).order(ByteOrder.LITTLE_ENDIAN);
        this.scratch = ByteBuffer.allocate(blockSize).order(ByteOrder.LITTLE_ENDIAN);
        this.builtins = FrameUniforms.bind(layout, new Std140Writer(template));
        for (int i = 0; i < GpuBufferRing.FRAMES_IN_FLIGHT; i++) {
            frames.add(new ArrayList<>());
        }
    }

    private static int alignUp(int value, int alignment) {
        return (value + alignment - 1) / alignment * alignment;
    }

    /**
     * Starts a frame: the slots of the oldest frame in flight are reused, once the GPU has
     * finished reading them. Call outside render passes.
     *
     * @param encoder the command encoder
     */
    public void beginFrame(CommandEncoder encoder) {
        if (frame >= 0 && fences[frame] == null) {
            // The previous frame was abandoned before endFrame: fence what was recorded so far.
            fences[frame] = encoder.createFence();
        }
        frame = (frame + 1) % GpuBufferRing.FRAMES_IN_FLIGHT;
        page = 0;
        slot = 0;
        GpuFence fence = fences[frame];
        fences[frame] = null;
        if (fence != null) {
            fence.awaitCompletion(GpuFence.NO_TIMEOUT);
            fence.close();
        }
    }

    /**
     * Ends the frame's use of its slots: they are reused once the GPU passes this point. Call
     * outside render passes, after the frame's last draw.
     *
     * @param encoder the command encoder
     */
    public void endFrame(CommandEncoder encoder) {
        if (frame >= 0 && fences[frame] == null) {
            fences[frame] = encoder.createFence();
        }
    }

    /**
     * Writes the block of one draw into the next slot; usable inside render passes.
     *
     * @param frameState the frame
     * @param draw       the draw's values ({@link DrawState#reset} first for defaults)
     * @return the slice to bind as {@code sb_Draw}, valid at once
     */
    public GpuBufferSlice push(FrameState frameState, DrawState draw) {
        if (frame < 0) {
            throw new IllegalStateException("beginFrame() was not called");
        }
        if (slot == SLOTS_PER_PAGE) {
            page++;
            slot = 0;
        }
        List<GpuBuffer> pages = frames.get(frame);
        if (page == pages.size()) {
            String label = "ShaderBridge sb_Draw " + frame + "/" + page;
            pages.add(RenderSystem.getDevice().createBuffer(() -> label, PAGE_USAGE, (long) stride * SLOTS_PER_PAGE));
        }
        GpuBuffer target = pages.get(page);
        int offset = slot * stride;
        scratch.clear();
        fill(frameState, draw, scratch);
        try (GpuBufferSlice.MappedView view = target.map(offset, blockSize, false, true)) {
            view.data().put(0, scratch, 0, blockSize);
        }
        slot++;
        return target.slice(offset, blockSize);
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
        writer.buffer().put(0, template, 0, blockSize);
        draw.update();
        for (FrameUniforms.Binding binding : builtins) {
            binding.builtin().provider().write(frameState, draw, out.bind(writer, binding.offset(), binding.member().ty()));
        }
    }

    @Override
    public void close() {
        for (int i = 0; i < fences.length; i++) {
            if (fences[i] != null) {
                fences[i].close();
                fences[i] = null;
            }
        }
        for (List<GpuBuffer> pages : frames) {
            pages.forEach(GpuBuffer::close);
            pages.clear();
        }
    }
}
