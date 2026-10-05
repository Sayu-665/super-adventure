package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.backend.vulkan.Destroyable;
import com.mojang.renderpearl.backend.vulkan.VulkanCommandEncoder;
import com.mojang.renderpearl.backend.vulkan.VulkanDevice;
import dev.shaderbridge.mixin.FrontendGpuDeviceAccess;
import dev.shaderbridge.mixin.VulkanDeviceAccess;
import java.util.List;
import java.util.Optional;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.vulkan.VK11;
import org.lwjgl.vulkan.VkDevice;
import org.lwjgl.vulkan.VkPhysicalDevice;
import org.lwjgl.vulkan.VkPhysicalDeviceLimits;
import org.lwjgl.vulkan.VkPhysicalDeviceProperties2;
import org.lwjgl.vulkan.VkPhysicalDeviceSubgroupProperties;

/**
 * Minecraft's Vulkan device as the raw path uses it, reached through Mojang's device facade
 * ({@code FrontendGpuDevice.backend}): the {@code VkDevice} and VMA allocator, the command encoder
 * (transient command buffers, deferred destruction), and what the device can do (the enabled
 * features, compute and image limits, format features).
 *
 * @param device         the backend device
 * @param features       the enabled features
 * @param compute        compute limits
 * @param images         image extent limits
 * @param maxBufferRange {@code maxStorageBufferRange}
 */
public record VulkanContext(VulkanDevice device, EnabledFeatures features, ComputeLimits compute, ResourceSizes.ImageLimits images,
                            long maxBufferRange) {
    /**
     * @param gpu Minecraft's GPU device
     * @return the Vulkan context, empty on another backend
     */
    public static Optional<VulkanContext> of(GpuDevice gpu) {
        if (!(gpu instanceof FrontendGpuDeviceAccess facade) || !(facade.shaderbridge$backend() instanceof VulkanDevice device)) {
            return Optional.empty();
        }
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkPhysicalDeviceSubgroupProperties subgroup = VkPhysicalDeviceSubgroupProperties.calloc(stack).sType$Default();
            VkPhysicalDeviceProperties2 properties = VkPhysicalDeviceProperties2.calloc(stack).sType$Default().pNext(subgroup);
            VK11.vkGetPhysicalDeviceProperties2(device.vkDevice().getPhysicalDevice(), properties);
            VkPhysicalDeviceLimits limits = properties.properties().limits();
            EnabledFeatures features = new EnabledFeatures(RawFeatureSets.enabled(((VulkanDeviceAccess) device).shaderbridge$enabledFeatures()),
                subgroup.supportedStages(), subgroup.supportedOperations());
            ComputeLimits compute = new ComputeLimits(
                List.of(limits.maxComputeWorkGroupCount(0), limits.maxComputeWorkGroupCount(1), limits.maxComputeWorkGroupCount(2)),
                List.of(limits.maxComputeWorkGroupSize(0), limits.maxComputeWorkGroupSize(1), limits.maxComputeWorkGroupSize(2)),
                limits.maxComputeWorkGroupInvocations());
            ResourceSizes.ImageLimits images = new ResourceSizes.ImageLimits(limits.maxImageDimension1D(), limits.maxImageDimension2D(),
                limits.maxImageDimension3D());
            return Optional.of(new VulkanContext(device, features, compute, images, Integer.toUnsignedLong(limits.maxStorageBufferRange())));
        }
    }

    /** @return the logical device */
    public VkDevice vk() {
        return device.vkDevice();
    }

    /** @return the physical device */
    public VkPhysicalDevice physical() {
        return device.vkDevice().getPhysicalDevice();
    }

    /** @return the VMA allocator */
    public long vma() {
        return device.vma();
    }

    /** @return Minecraft's command encoder (one per device) */
    public VulkanCommandEncoder encoder() {
        return device.createCommandEncoder();
    }

    /**
     * Destroys an object once the GPU no longer uses it (after the submits in flight completed).
     * Render thread only.
     *
     * @param destroyable the object
     */
    public void destroyLater(Destroyable destroyable) {
        encoder().queueForDestroy(destroyable);
    }

    /**
     * @param vkFormat a {@code VkFormat}
     * @return its optimal-tiling features on this device
     */
    public int formatFeatures(int vkFormat) {
        return FormatSupport.features(physical(), vkFormat);
    }

    /**
     * @param vkFormat a {@code VkFormat}
     * @param feature  {@code VkFormatFeatureFlagBits}
     * @return whether the device supports the feature for the format
     */
    public boolean supports(int vkFormat, int feature) {
        return (formatFeatures(vkFormat) & feature) == feature;
    }
}
