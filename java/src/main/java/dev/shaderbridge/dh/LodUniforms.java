package dev.shaderbridge.dh;

import com.mojang.renderpearl.api.buffers.GpuBuffer;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.device.GpuDevice;
import dev.shaderbridge.uniforms.Std140Writer;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;
import java.util.List;
import org.joml.Vector3dc;

/**
 * Uniform buffer space for the Distant Horizons host blocks of one frame: per LOD pass, one
 * {@code vertSharedUniformBlock} followed by one {@code vertUniqueUniformBlock} per buffer, each
 * in its own slot aligned to the device's uniform offset alignment. Slots live in pages that every
 * frame in flight owns separately (the GPU may still read an older frame's) and that are reused
 * frame after frame; a frame's passes write consecutive slots, never overwriting a slot an earlier
 * pass of the frame binds. Blocks are written before their pass opens (buffer writes are not
 * allowed inside a render pass). Render thread only.
 */
public final class LodUniforms implements AutoCloseable {
    /** Frames the GPU may lag behind, as Minecraft's own ring buffers assume. */
    static final int FRAMES_IN_FLIGHT = 3;
    /** Slots per page. */
    static final int SLOTS_PER_PAGE = 1024;

    private final GpuDevice device;
    private final int stride;
    private final ByteBuffer staging;
    private final Std140Writer writer;
    private final List<List<GpuBuffer>> pages = new ArrayList<>();
    private int frame = -1;
    private int used;

    /**
     * The slots of one pass.
     *
     * @param shared the {@code vertSharedUniformBlock} slice
     * @param unique the {@code vertUniqueUniformBlock} slice of each buffer, in order
     */
    public record PassSlots(GpuBufferSlice shared, List<GpuBufferSlice> unique) {
    }

    /**
     * @param device    the GPU device
     * @param alignment the device's minimum uniform buffer offset alignment
     */
    public LodUniforms(GpuDevice device, int alignment) {
        this.device = device;
        this.stride = slotStride(alignment);
        this.staging = ByteBuffer.allocateDirect(SLOTS_PER_PAGE * stride).order(ByteOrder.LITTLE_ENDIAN);
        this.writer = new Std140Writer(staging);
        for (int i = 0; i < FRAMES_IN_FLIGHT; i++) {
            pages.add(new ArrayList<>());
        }
    }

    /**
     * @param alignment the device's minimum uniform buffer offset alignment
     * @return the distance between slots: the larger block rounded up to the alignment
     */
    static int slotStride(int alignment) {
        int a = Math.max(16, alignment);
        int size = Math.max(DhHostBlocks.SHARED_SIZE, DhHostBlocks.UNIQUE_SIZE);
        return (size + a - 1) / a * a;
    }

    /** Starts a frame: the slots of the oldest frame in flight are reused. */
    public void beginFrame() {
        frame = (frame + 1) % FRAMES_IN_FLIGHT;
        used = 0;
    }

    /**
     * Writes and uploads the blocks of one pass.
     *
     * @param encoder a command encoder (outside any render pass)
     * @param shared  the pass's shared values
     * @param buffers the buffers the pass draws
     * @param camera  the camera position ({@code uModelOffset} is relative to it)
     * @return the slices to bind
     */
    public PassSlots write(CommandEncoder encoder, DhHostBlocks.Shared shared, List<LodBuffer> buffers, Vector3dc camera) {
        List<GpuBufferSlice> unique = new ArrayList<>(buffers.size());
        int pageStart = used;
        GpuBufferSlice sharedSlice = null;
        for (int i = -1; i < buffers.size(); i++) {
            int slot = used % SLOTS_PER_PAGE;
            if (slot == 0 && used > pageStart) {
                upload(encoder, pageStart, used);
                pageStart = used;
            }
            int base = slot * stride;
            GpuBufferSlice slice;
            if (i < 0) {
                DhHostBlocks.writeShared(writer, base, shared);
                slice = page(used / SLOTS_PER_PAGE).slice((long) base, DhHostBlocks.SHARED_SIZE);
                sharedSlice = slice;
            } else {
                LodBuffer b = buffers.get(i);
                DhHostBlocks.writeUnique(writer, base, DhHostBlocks.modelOffset(b.minX(), b.minY(), b.minZ(), camera.x(), camera.y(), camera.z()));
                slice = page(used / SLOTS_PER_PAGE).slice((long) base, DhHostBlocks.UNIQUE_SIZE);
                unique.add(slice);
            }
            used++;
        }
        upload(encoder, pageStart, used);
        return new PassSlots(sharedSlice, unique);
    }

    /** Uploads the staged slots [from, to) of one page. */
    private void upload(CommandEncoder encoder, int from, int to) {
        if (to <= from) {
            return;
        }
        int first = from % SLOTS_PER_PAGE;
        int count = to - from;
        ByteBuffer data = staging.duplicate().order(ByteOrder.LITTLE_ENDIAN);
        data.position(first * stride).limit((first + count) * stride);
        encoder.writeToBuffer(page(from / SLOTS_PER_PAGE).slice((long) first * stride, (long) count * stride), data);
    }

    private GpuBuffer page(int index) {
        List<GpuBuffer> own = pages.get(frame);
        while (own.size() <= index) {
            String label = "ShaderBridge DH uniforms #" + frame + "." + own.size();
            own.add(device.createBuffer(() -> label, GpuBuffer.USAGE_UNIFORM | GpuBuffer.USAGE_COPY_DST, (long) SLOTS_PER_PAGE * stride));
        }
        return own.get(index);
    }

    @Override
    public void close() {
        for (List<GpuBuffer> own : pages) {
            own.forEach(GpuBuffer::close);
            own.clear();
        }
    }
}
