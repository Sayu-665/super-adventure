package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.backend.vulkan.VulkanFeatureSets;
import com.mojang.renderpearl.backend.vulkan.init.FeatureSet;
import com.mojang.renderpearl.backend.vulkan.init.VulkanFeature;
import it.unimi.dsi.fastutil.objects.ObjectOpenHashSet;
import java.util.EnumSet;
import java.util.Set;

/**
 * ShaderBridge's {@link RawFeature}s as Mojang's optional device feature sets. Minecraft enables
 * each optional set the physical device supports when it creates its Vulkan device and logs
 * {@code Enabling optional FeatureSet [ShaderBridge <feature>]}; one set per feature, so a device
 * lacking one still gets the others.
 */
public final class RawFeatureSets {
    /** Name prefix of ShaderBridge's feature sets in Minecraft's log. */
    public static final String PREFIX = "ShaderBridge ";

    private RawFeatureSets() {
    }

    /**
     * @param optional Minecraft's optional feature sets
     * @return them plus ShaderBridge's
     */
    public static Set<FeatureSet> withRequested(Set<FeatureSet> optional) {
        Set<FeatureSet> out = new ObjectOpenHashSet<>(optional);
        for (RawFeature feature : RawFeature.values()) {
            out.add(new FeatureSet(PREFIX + feature.vkName(), Set.of(), Set.of(vulkanFeature(feature))));
        }
        return out;
    }

    /**
     * @param enabled the feature set Minecraft created its device with
     * @return ShaderBridge's features among it
     */
    public static Set<RawFeature> enabled(FeatureSet enabled) {
        Set<RawFeature> out = EnumSet.noneOf(RawFeature.class);
        for (RawFeature feature : RawFeature.values()) {
            if (enabled.features().contains(vulkanFeature(feature))) {
                out.add(feature);
            }
        }
        return out;
    }

    private static VulkanFeature vulkanFeature(RawFeature feature) {
        return new VulkanFeature(VulkanFeatureSets.VK10_FEATURES_STRUCT, feature.vkName());
    }
}
