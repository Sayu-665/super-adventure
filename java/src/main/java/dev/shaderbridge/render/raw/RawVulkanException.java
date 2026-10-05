package dev.shaderbridge.render.raw;

import org.lwjgl.vulkan.VK10;

/** A Vulkan call of the raw path failed; the program or resource it was for is not used. */
public final class RawVulkanException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    /**
     * @param call   the Vulkan function
     * @param result its {@code VkResult}
     */
    public RawVulkanException(String call, int result) {
        super(call + " failed with VkResult " + result);
    }

    /**
     * @param result a {@code VkResult}
     * @param call   the Vulkan function that returned it
     * @throws RawVulkanException unless it is {@code VK_SUCCESS}
     */
    public static void check(int result, String call) {
        if (result != VK10.VK_SUCCESS) {
            throw new RawVulkanException(call, result);
        }
    }
}
