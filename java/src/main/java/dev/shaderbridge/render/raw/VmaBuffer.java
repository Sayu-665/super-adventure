package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.backend.vulkan.Destroyable;
import java.nio.ByteBuffer;
import java.nio.LongBuffer;
import org.lwjgl.PointerBuffer;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.system.MemoryUtil;
import org.lwjgl.util.vma.Vma;
import org.lwjgl.util.vma.VmaAllocationCreateInfo;
import org.lwjgl.util.vma.VmaAllocationInfo;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VkBufferCreateInfo;

/**
 * A buffer the raw path allocates through Minecraft's VMA allocator: a pack's shader storage
 * buffer or the zero-filled stand-in for a missing one (device-local), or the staging buffer of
 * an upload (host-visible).
 *
 * @param buffer     the {@code VkBuffer}
 * @param allocation its VMA allocation
 * @param size       its size in bytes
 * @param vma        the allocator
 */
public record VmaBuffer(long buffer, long allocation, long size, long vma) implements Destroyable {
    /** Usage of storage buffers: storage, indirect dispatch arguments, transfer for fills and uploads. */
    public static final int STORAGE_USAGE = VK10.VK_BUFFER_USAGE_STORAGE_BUFFER_BIT | VK10.VK_BUFFER_USAGE_INDIRECT_BUFFER_BIT
        | VK10.VK_BUFFER_USAGE_TRANSFER_DST_BIT | VK10.VK_BUFFER_USAGE_TRANSFER_SRC_BIT;

    /**
     * @param ctx   the Vulkan context
     * @param size  size in bytes
     * @param usage {@code VkBufferUsageFlags}
     * @return the buffer
     * @throws RawVulkanException if Vulkan fails
     */
    public static VmaBuffer create(VulkanContext ctx, long size, int usage) {
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkBufferCreateInfo info = VkBufferCreateInfo.calloc(stack).sType$Default()
                .size(size)
                .usage(usage)
                .sharingMode(VK10.VK_SHARING_MODE_EXCLUSIVE);
            VmaAllocationCreateInfo allocation = VmaAllocationCreateInfo.calloc(stack).usage(Vma.VMA_MEMORY_USAGE_AUTO_PREFER_DEVICE);
            LongBuffer pBuffer = stack.mallocLong(1);
            PointerBuffer pAllocation = stack.mallocPointer(1);
            RawVulkanException.check(Vma.vmaCreateBuffer(ctx.vma(), info, allocation, pBuffer, pAllocation, null), "vmaCreateBuffer");
            return new VmaBuffer(pBuffer.get(0), pAllocation.get(0), size, ctx.vma());
        }
    }

    /**
     * Creates a host-visible buffer holding data, the source of a copy into a raw resource. Destroy
     * it through {@link VulkanContext#destroyLater} after recording the copy.
     *
     * @param ctx  the Vulkan context
     * @param data the data, from its position to its limit
     * @return the buffer
     * @throws RawVulkanException if Vulkan fails
     */
    public static VmaBuffer staging(VulkanContext ctx, ByteBuffer data) {
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkBufferCreateInfo info = VkBufferCreateInfo.calloc(stack).sType$Default()
                .size(Math.max(1, data.remaining()))
                .usage(VK10.VK_BUFFER_USAGE_TRANSFER_SRC_BIT)
                .sharingMode(VK10.VK_SHARING_MODE_EXCLUSIVE);
            VmaAllocationCreateInfo allocation = VmaAllocationCreateInfo.calloc(stack).usage(Vma.VMA_MEMORY_USAGE_AUTO)
                .flags(Vma.VMA_ALLOCATION_CREATE_HOST_ACCESS_SEQUENTIAL_WRITE_BIT | Vma.VMA_ALLOCATION_CREATE_MAPPED_BIT);
            VmaAllocationInfo allocated = VmaAllocationInfo.calloc(stack);
            LongBuffer pBuffer = stack.mallocLong(1);
            PointerBuffer pAllocation = stack.mallocPointer(1);
            RawVulkanException.check(Vma.vmaCreateBuffer(ctx.vma(), info, allocation, pBuffer, pAllocation, allocated), "vmaCreateBuffer");
            MemoryUtil.memCopy(MemoryUtil.memAddress(data), allocated.pMappedData(), data.remaining());
            Vma.vmaFlushAllocation(ctx.vma(), pAllocation.get(0), 0, VK10.VK_WHOLE_SIZE);
            return new VmaBuffer(pBuffer.get(0), pAllocation.get(0), data.remaining(), ctx.vma());
        }
    }

    @Override
    public void destroy() {
        Vma.vmaDestroyBuffer(vma, buffer, allocation);
    }
}
