package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.backend.vulkan.Destroyable;
import java.nio.LongBuffer;
import org.lwjgl.PointerBuffer;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.util.vma.Vma;
import org.lwjgl.util.vma.VmaAllocationCreateInfo;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VkDevice;
import org.lwjgl.vulkan.VkImageCreateInfo;
import org.lwjgl.vulkan.VkImageViewCreateInfo;

/**
 * An image the raw path allocates itself through Minecraft's VMA allocator (custom images,
 * 1D/3D textures, stand-ins), with one view of its whole extent. It starts {@code UNDEFINED};
 * {@link ResourceInit} moves it to {@code GENERAL} before its first use, where it stays, like
 * Minecraft's own images.
 *
 * @param image      the {@code VkImage}
 * @param allocation its VMA allocation
 * @param view       the view of all of it ({@code 1D}, {@code 2D} or {@code 3D})
 * @param vkFormat   its {@code VkFormat}
 * @param dimensions 1, 2 or 3
 * @param width      width in texels
 * @param height     height in texels
 * @param depth      depth in texels
 * @param vma        the allocator
 * @param device     the device (for the view)
 */
public record VmaImage(long image, long allocation, long view, int vkFormat, int dimensions, int width, int height, int depth, long vma,
                       VkDevice device) implements Destroyable {
    /** Usage of custom images and stand-ins: storage, sampled, and transfer for clears. */
    public static final int USAGE = VK10.VK_IMAGE_USAGE_STORAGE_BIT | VK10.VK_IMAGE_USAGE_SAMPLED_BIT | VK10.VK_IMAGE_USAGE_TRANSFER_DST_BIT
        | VK10.VK_IMAGE_USAGE_TRANSFER_SRC_BIT;

    /** Usage of raw textures: sampled, and transfer for their upload. */
    public static final int TEXTURE_USAGE = VK10.VK_IMAGE_USAGE_SAMPLED_BIT | VK10.VK_IMAGE_USAGE_TRANSFER_DST_BIT;

    /**
     * Creates an image and its view.
     *
     * @param ctx        the Vulkan context
     * @param vkFormat   the format
     * @param dimensions 1, 2 or 3
     * @param width      width in texels
     * @param height     height in texels
     * @param depth      depth in texels
     * @param usage      {@code VkImageUsageFlags}
     * @return the image
     * @throws RawVulkanException if Vulkan fails
     */
    public static VmaImage create(VulkanContext ctx, int vkFormat, int dimensions, int width, int height, int depth, int usage) {
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkImageCreateInfo info = VkImageCreateInfo.calloc(stack).sType$Default()
                .imageType(switch (dimensions) {
                    case 1 -> VK10.VK_IMAGE_TYPE_1D;
                    case 2 -> VK10.VK_IMAGE_TYPE_2D;
                    default -> VK10.VK_IMAGE_TYPE_3D;
                })
                .format(vkFormat)
                .mipLevels(1)
                .arrayLayers(1)
                .samples(VK10.VK_SAMPLE_COUNT_1_BIT)
                .tiling(VK10.VK_IMAGE_TILING_OPTIMAL)
                .usage(usage)
                .sharingMode(VK10.VK_SHARING_MODE_EXCLUSIVE)
                .initialLayout(VK10.VK_IMAGE_LAYOUT_UNDEFINED);
            info.extent().set(width, height, depth);
            VmaAllocationCreateInfo allocation = VmaAllocationCreateInfo.calloc(stack).usage(Vma.VMA_MEMORY_USAGE_AUTO_PREFER_DEVICE);
            LongBuffer pImage = stack.mallocLong(1);
            PointerBuffer pAllocation = stack.mallocPointer(1);
            RawVulkanException.check(Vma.vmaCreateImage(ctx.vma(), info, allocation, pImage, pAllocation, null), "vmaCreateImage");
            long image = pImage.get(0);
            long memory = pAllocation.get(0);
            VkImageViewCreateInfo viewInfo = VkImageViewCreateInfo.calloc(stack).sType$Default()
                .image(image)
                .viewType(switch (dimensions) {
                    case 1 -> VK10.VK_IMAGE_VIEW_TYPE_1D;
                    case 2 -> VK10.VK_IMAGE_VIEW_TYPE_2D;
                    default -> VK10.VK_IMAGE_VIEW_TYPE_3D;
                })
                .format(vkFormat);
            viewInfo.subresourceRange().set(VK10.VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1);
            LongBuffer pView = stack.mallocLong(1);
            int result = VK10.vkCreateImageView(ctx.vk(), viewInfo, null, pView);
            if (result != VK10.VK_SUCCESS) {
                Vma.vmaDestroyImage(ctx.vma(), image, memory);
                throw new RawVulkanException("vkCreateImageView", result);
            }
            return new VmaImage(image, memory, pView.get(0), vkFormat, dimensions, width, height, depth, ctx.vma(), ctx.vk());
        }
    }

    @Override
    public void destroy() {
        VK10.vkDestroyImageView(device, view, null);
        Vma.vmaDestroyImage(vma, image, allocation);
    }
}
