package dev.shaderbridge.render.raw;

import dev.shaderbridge.render.pipeline.PipelineCapabilities;
import java.util.EnumSet;
import java.util.Set;

/**
 * What Minecraft's Vulkan device was created with, as far as pack programs care: which of
 * ShaderBridge's {@link RawFeature}s it enabled, and its subgroup support (Vulkan 1.1 core,
 * no feature to enable).
 *
 * @param features           the enabled features
 * @param subgroupStages     {@code VkPhysicalDeviceSubgroupProperties.supportedStages}
 * @param subgroupOperations {@code VkPhysicalDeviceSubgroupProperties.supportedOperations}
 */
public record EnabledFeatures(Set<RawFeature> features, int subgroupStages, int subgroupOperations) {
    public EnabledFeatures {
        features = features.isEmpty() ? Set.of() : Set.copyOf(EnumSet.copyOf(features));
    }

    /** @return a device with none of the features and no subgroup support */
    public static EnabledFeatures none() {
        return new EnabledFeatures(Set.of(), 0, 0);
    }

    /**
     * @param feature a feature
     * @return whether the device has it enabled
     */
    public boolean has(RawFeature feature) {
        return features.contains(feature);
    }

    /** @return what renderpearl pipelines may do on this device */
    public PipelineCapabilities capabilities() {
        return new PipelineCapabilities(has(RawFeature.INDEPENDENT_BLEND), PipelineCapabilities.DEFAULT_MAX_DESCRIPTORS);
    }
}
