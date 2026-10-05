package dev.shaderbridge.dh;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.buffers.GpuBuffer;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.device.GpuDevice;
import java.lang.reflect.Proxy;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;
import java.util.List;
import java.util.stream.IntStream;
import org.joml.Matrix4f;
import org.joml.Vector3d;
import org.junit.jupiter.api.Test;

/**
 * {@link LodUniforms}: slot layout and uploads of the Distant Horizons host blocks, with a device
 * and an encoder that keep buffer contents in memory.
 */
class LodUniformsTest {
    private static final int ALIGNMENT = 256;
    private static final Vector3d CAMERA = new Vector3d(1000.5, 70, -2000.25);
    private static final DhHostBlocks.Shared SHARED = new DhHostBlocks.Shared(-64, 0, 3, 1920, 1080, new Matrix4f());

    /** A buffer whose contents live in memory. */
    private static final class MemoryBuffer implements GpuBuffer {
        final ByteBuffer bytes;
        final int usage;

        MemoryBuffer(long size, int usage) {
            this.bytes = ByteBuffer.allocate((int) size).order(ByteOrder.LITTLE_ENDIAN);
            this.usage = usage;
        }

        @Override
        public long size() {
            return bytes.capacity();
        }

        @Override
        public int usage() {
            return usage;
        }

        @Override
        public boolean isClosed() {
            return false;
        }

        @Override
        public GpuBufferSlice.MappedView map(long offset, long length, boolean read, boolean write) {
            throw new UnsupportedOperationException();
        }

        @Override
        public void close() {
        }
    }

    private final List<MemoryBuffer> created = new ArrayList<>();
    private final GpuDevice device = (GpuDevice) Proxy.newProxyInstance(GpuDevice.class.getClassLoader(), new Class<?>[] {GpuDevice.class},
        (proxy, method, args) -> {
            if (method.getName().equals("createBuffer") && args[2] instanceof Long size) {
                MemoryBuffer buffer = new MemoryBuffer(size, (int) args[1]);
                created.add(buffer);
                return buffer;
            }
            throw new UnsupportedOperationException(method.getName());
        });
    private final CommandEncoder encoder = (CommandEncoder) Proxy.newProxyInstance(CommandEncoder.class.getClassLoader(),
        new Class<?>[] {CommandEncoder.class}, (proxy, method, args) -> {
            if (method.getName().equals("writeToBuffer")) {
                GpuBufferSlice slice = (GpuBufferSlice) args[0];
                ByteBuffer data = (ByteBuffer) args[1];
                assertTrue(data.remaining() <= slice.length(), "fits the slice");
                ((MemoryBuffer) slice.buffer()).bytes.put((int) slice.offset(), data, data.position(), data.remaining());
                return null;
            }
            throw new UnsupportedOperationException(method.getName());
        });

    private static List<LodBuffer> buffers(int count) {
        return IntStream.range(0, count).mapToObj(i -> new LodBuffer(1000 + i * 64, -64, -2048, 64, null, null, 6)).toList();
    }

    @Test
    void slotsHoldTheLargerBlockAtTheDeviceAlignment() {
        assertEquals(256, LodUniforms.slotStride(256), "NVIDIA-style alignment");
        assertEquals(128, LodUniforms.slotStride(64), "112-byte shared block rounded up to 64");
        assertEquals(112, LodUniforms.slotStride(16));
        assertEquals(112, LodUniforms.slotStride(1), "std140 blocks never start below 16-byte alignment");
    }

    @Test
    void aPassGetsTheSharedBlockThenOneBlockPerBuffer() {
        LodUniforms uniforms = new LodUniforms(device, ALIGNMENT);
        uniforms.beginFrame();
        LodUniforms.PassSlots slots = uniforms.write(encoder, SHARED, buffers(3), CAMERA);
        assertEquals(0, slots.shared().offset());
        assertEquals(DhHostBlocks.SHARED_SIZE, slots.shared().length());
        assertEquals(List.of(256L, 512L, 768L), slots.unique().stream().map(GpuBufferSlice::offset).toList());
        assertTrue(slots.unique().stream().allMatch(s -> s.length() == DhHostBlocks.UNIQUE_SIZE));
        MemoryBuffer page = (MemoryBuffer) slots.shared().buffer();
        assertEquals(GpuBuffer.USAGE_UNIFORM | GpuBuffer.USAGE_COPY_DST, page.usage);
        assertEquals(-64f, page.bytes.getFloat(4), "uWorldYOffset uploaded");
        assertEquals(1000 - 1000.5f, page.bytes.getFloat(256), "uModelOffset.x of the first buffer");
        assertEquals(1128 - 1000.5f, page.bytes.getFloat(768), "uModelOffset.x of the third buffer");
        assertEquals(-2048 + 2000.25f, page.bytes.getFloat(768 + 8));
    }

    @Test
    void laterPassesOfAFrameUseFreshSlotsAndPagesGrow() {
        LodUniforms uniforms = new LodUniforms(device, ALIGNMENT);
        uniforms.beginFrame();
        LodUniforms.PassSlots first = uniforms.write(encoder, SHARED, buffers(2), CAMERA);
        LodUniforms.PassSlots second = uniforms.write(encoder, SHARED, buffers(LodUniforms.SLOTS_PER_PAGE), CAMERA);
        assertEquals(3L * ALIGNMENT, second.shared().offset(), "after the first pass's three slots");
        GpuBufferSlice last = second.unique().getLast();
        assertNotSame(first.shared().buffer(), last.buffer(), "the second pass spills into a second page");
        assertEquals(3L * ALIGNMENT, last.offset(), "the frame's slot 1027 is slot 3 of the second page");
        float expected = (float) (1000 + (LodUniforms.SLOTS_PER_PAGE - 1) * 64 - 1000.5);
        assertEquals(expected, ((MemoryBuffer) last.buffer()).bytes.getFloat((int) last.offset()), "uploaded into the second page");
        assertEquals(2, created.size());
    }

    @Test
    void eachFrameInFlightOwnsItsPages() {
        LodUniforms uniforms = new LodUniforms(device, ALIGNMENT);
        List<GpuBuffer> pages = new ArrayList<>();
        for (int frame = 0; frame < LodUniforms.FRAMES_IN_FLIGHT + 1; frame++) {
            uniforms.beginFrame();
            pages.add(uniforms.write(encoder, SHARED, buffers(1), CAMERA).shared().buffer());
        }
        assertEquals(3, pages.stream().distinct().count());
        assertSame(pages.getFirst(), pages.getLast(), "the oldest frame's page is reused");
    }
}
