package dev.shaderbridge.mixin;

import com.mojang.renderpearl.backend.vulkan.VulkanGpuTexture;
import dev.shaderbridge.render.raw.StorageUsage;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.ModifyArg;

/**
 * Passes the Vulkan usage of every image Mojang creates through {@link StorageUsage#imageUsage},
 * which adds {@code VK_IMAGE_USAGE_STORAGE_BIT} while a requested render target is being created
 * ({@code VulkanDeviceMixin}). Not required: without it no target gets the bit.
 */
@Mixin(value = VulkanGpuTexture.class, remap = false)
abstract class VulkanGpuTextureMixin {
    @ModifyArg(
        method = "<init>(Lcom/mojang/renderpearl/backend/vulkan/VulkanDevice;ILjava/lang/String;Lcom/mojang/renderpearl/api/GpuFormat;IIII)V",
        at = @At(value = "INVOKE", target = "Lorg/lwjgl/vulkan/VkImageCreateInfo;usage(I)Lorg/lwjgl/vulkan/VkImageCreateInfo;"),
        require = 0
    )
    private int shaderbridge$storageUsage(int vkUsage) {
        return StorageUsage.imageUsage(vkUsage);
    }
}
