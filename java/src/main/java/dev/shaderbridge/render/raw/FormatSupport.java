package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.backend.vulkan.VulkanConst;
import com.mojang.renderpearl.backend.vulkan.VulkanDevice;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VkFormatProperties;
import org.lwjgl.vulkan.VkPhysicalDevice;

/** The optimal-tiling format features of the physical device behind Minecraft's Vulkan device, cached. Thread-safe. */
public final class FormatSupport {
    private static final Map<Key, Integer> FEATURES = new ConcurrentHashMap<>();

    private record Key(long physicalDevice, int vkFormat) {
    }

    private FormatSupport() {
    }

    /**
     * @param device Minecraft's Vulkan device
     * @param format a texture format
     * @return whether images of the format can be storage images
     */
    public static boolean storageImage(VulkanDevice device, GpuFormat format) {
        return (features(device.vkDevice().getPhysicalDevice(), VulkanConst.toVk(format)) & VK10.VK_FORMAT_FEATURE_STORAGE_IMAGE_BIT) != 0;
    }

    /**
     * @param physical a physical device
     * @param vkFormat a {@code VkFormat}
     * @return its {@code optimalTilingFeatures}
     */
    public static int features(VkPhysicalDevice physical, int vkFormat) {
        return FEATURES.computeIfAbsent(new Key(physical.address(), vkFormat), k -> {
            try (MemoryStack stack = MemoryStack.stackPush()) {
                VkFormatProperties properties = VkFormatProperties.calloc(stack);
                VK10.vkGetPhysicalDeviceFormatProperties(physical, vkFormat, properties);
                return properties.optimalTilingFeatures();
            }
        });
    }
}
