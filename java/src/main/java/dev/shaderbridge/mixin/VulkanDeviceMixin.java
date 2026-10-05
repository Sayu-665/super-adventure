package dev.shaderbridge.mixin;

import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.backend.vulkan.VulkanDevice;
import dev.shaderbridge.render.raw.FormatSupport;
import dev.shaderbridge.render.raw.StorageUsage;
import org.spongepowered.asm.mixin.Mixin;

/**
 * Lets the raw Vulkan path add the storage usage to the render targets a pack binds as storage
 * images: texture creation runs inside {@link StorageUsage#create}, which decides by label and
 * format support whether {@code VulkanGpuTextureMixin} adds the bit. Not required: without it
 * those bindings get a stand-in image and a message.
 */
@Mixin(value = VulkanDevice.class, remap = false)
abstract class VulkanDeviceMixin {
    @WrapMethod(method = "createTexture(Ljava/lang/String;ILcom/mojang/renderpearl/api/GpuFormat;IIII)Lcom/mojang/renderpearl/api/textures/GpuTexture;",
        require = 0)
    private GpuTexture shaderbridge$storageUsage(String label, int usage, GpuFormat format, int width, int height, int depthOrLayers, int mipLevels,
                                                 Operation<GpuTexture> original) {
        VulkanDevice device = (VulkanDevice) (Object) this;
        return StorageUsage.create(label, () -> FormatSupport.storageImage(device, format),
            () -> original.call(label, usage, format, width, height, depthOrLayers, mipLevels));
    }
}
