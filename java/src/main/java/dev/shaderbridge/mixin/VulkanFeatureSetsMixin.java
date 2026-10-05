package dev.shaderbridge.mixin;

import com.llamalad7.mixinextras.injector.ModifyReturnValue;
import com.mojang.renderpearl.backend.vulkan.VulkanFeatureSets;
import com.mojang.renderpearl.backend.vulkan.init.FeatureSet;
import dev.shaderbridge.render.raw.RawFeatureSets;
import java.util.Set;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;

/**
 * Requests the device features of ShaderBridge's raw Vulkan path and pack pipelines
 * (independent blending, geometry and tessellation shaders, stores outside compute, storage image
 * formats, ...) as optional feature sets, which {@code VulkanBackend.createDevice} enables where
 * the device supports them. Not required: without it the features stay off, pack pipelines keep
 * Minecraft's blending limits and programs that need a feature are skipped with a message.
 */
@Mixin(value = VulkanFeatureSets.class, remap = false)
abstract class VulkanFeatureSetsMixin {
    @ModifyReturnValue(method = "optionalFeatureSets()Ljava/util/Set;", at = @At("RETURN"), require = 0)
    private static Set<FeatureSet> shaderbridge$requestRawFeatures(Set<FeatureSet> optional) {
        return RawFeatureSets.withRequested(optional);
    }
}
