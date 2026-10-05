package dev.shaderbridge.mixin;

import com.mojang.renderpearl.backend.api.GpuDeviceBackend;
import com.mojang.renderpearl.frontend.FrontendGpuDevice;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.gen.Accessor;

/**
 * The backend behind Mojang's device facade: on the Vulkan backend a {@code VulkanDevice}, whose
 * {@code VkDevice}, VMA allocator and command encoder the raw Vulkan path works with.
 */
@Mixin(value = FrontendGpuDevice.class, remap = false)
public interface FrontendGpuDeviceAccess {
    /** @return the backend device ({@code VulkanDevice} or the OpenGL device) */
    @Accessor("backend")
    GpuDeviceBackend shaderbridge$backend();
}
