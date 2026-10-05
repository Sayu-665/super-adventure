package dev.shaderbridge.mixin;

import com.mojang.renderpearl.backend.vulkan.VulkanDevice;
import com.mojang.renderpearl.backend.vulkan.init.FeatureSet;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.gen.Accessor;

/**
 * The extensions and features Minecraft created its Vulkan device with, including the optional
 * features ShaderBridge requested ({@code VulkanFeatureSetsMixin}) that the device supports.
 */
@Mixin(value = VulkanDevice.class, remap = false)
public interface VulkanDeviceAccess {
    /** @return the enabled feature set */
    @Accessor("enabledFeatures")
    FeatureSet shaderbridge$enabledFeatures();
}
