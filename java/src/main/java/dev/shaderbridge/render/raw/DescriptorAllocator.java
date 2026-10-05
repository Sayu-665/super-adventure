package dev.shaderbridge.render.raw;

import java.nio.LongBuffer;
import java.util.ArrayDeque;
import java.util.Deque;
import java.util.List;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VK11;
import org.lwjgl.vulkan.VkDescriptorPoolCreateInfo;
import org.lwjgl.vulkan.VkDescriptorPoolSize;
import org.lwjgl.vulkan.VkDescriptorSetAllocateInfo;

/**
 * Allocates the descriptor sets of raw dispatches and draws: fresh sets for every use, from pools
 * of {@link DescriptorBudget} size. A pool that cannot serve an allocation is retired through
 * Minecraft's deferred destruction, which runs once the submits that used its sets completed, and
 * then reset and reused. Render thread only.
 */
final class DescriptorAllocator implements AutoCloseable {
    private final VulkanContext ctx;
    private final Deque<Long> free = new ArrayDeque<>();
    private long current = VK10.VK_NULL_HANDLE;
    private boolean closed;

    DescriptorAllocator(VulkanContext ctx) {
        this.ctx = ctx;
    }

    /**
     * @param layouts the set layouts, one set each
     * @return the sets, valid for recording in the current frame
     * @throws RawVulkanException if even a fresh pool cannot serve them
     */
    long[] allocate(List<Long> layouts) {
        if (layouts.isEmpty()) {
            return new long[0];
        }
        boolean fresh = false;
        while (true) {
            if (current == VK10.VK_NULL_HANDLE) {
                current = free.isEmpty() ? createPool() : free.pop();
                fresh = true;
            }
            try (MemoryStack stack = MemoryStack.stackPush()) {
                LongBuffer pLayouts = stack.mallocLong(layouts.size());
                layouts.forEach(pLayouts::put);
                pLayouts.flip();
                VkDescriptorSetAllocateInfo info = VkDescriptorSetAllocateInfo.calloc(stack).sType$Default().descriptorPool(current).pSetLayouts(pLayouts);
                LongBuffer pSets = stack.mallocLong(layouts.size());
                int result = VK10.vkAllocateDescriptorSets(ctx.vk(), info, pSets);
                if (result == VK10.VK_SUCCESS) {
                    long[] sets = new long[layouts.size()];
                    pSets.get(sets);
                    return sets;
                }
                if (fresh || result != VK11.VK_ERROR_OUT_OF_POOL_MEMORY && result != VK10.VK_ERROR_FRAGMENTED_POOL) {
                    throw new RawVulkanException("vkAllocateDescriptorSets", result);
                }
                retire(current);
                current = VK10.VK_NULL_HANDLE;
            }
        }
    }

    private long createPool() {
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkDescriptorPoolSize.Buffer sizes = VkDescriptorPoolSize.calloc(DescriptorBudget.DESCRIPTORS.size(), stack);
            DescriptorBudget.DESCRIPTORS.forEach((type, count) -> sizes.get().type(type).descriptorCount(count));
            sizes.flip();
            VkDescriptorPoolCreateInfo info = VkDescriptorPoolCreateInfo.calloc(stack).sType$Default().maxSets(DescriptorBudget.SETS).pPoolSizes(sizes);
            LongBuffer pPool = stack.mallocLong(1);
            RawVulkanException.check(VK10.vkCreateDescriptorPool(ctx.vk(), info, null, pPool), "vkCreateDescriptorPool");
            return pPool.get(0);
        }
    }

    /** Resets the pool once the GPU is done with its sets, and reuses it (or destroys it after {@link #close}). */
    private void retire(long pool) {
        ctx.destroyLater(() -> {
            if (closed) {
                VK10.vkDestroyDescriptorPool(ctx.vk(), pool, null);
            } else {
                VK10.vkResetDescriptorPool(ctx.vk(), pool, 0);
                free.push(pool);
            }
        });
    }

    @Override
    public void close() {
        closed = true;
        if (current != VK10.VK_NULL_HANDLE) {
            retire(current);
            current = VK10.VK_NULL_HANDLE;
        }
        free.forEach(pool -> VK10.vkDestroyDescriptorPool(ctx.vk(), pool, null));
        free.clear();
    }
}
