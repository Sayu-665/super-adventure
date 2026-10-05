package dev.shaderbridge.render.raw;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.render.pipeline.PipelineCapabilities;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.util.Arrays;
import java.util.EnumSet;
import java.util.Locale;
import java.util.Set;
import org.junit.jupiter.api.Test;

class RawFeatureTest {
    /**
     * Mojang's {@code VulkanFeature(VK10_FEATURES_STRUCT, name)} finds a feature through
     * {@code VkPhysicalDeviceFeatures.<name>()} and its {@code <NAME>} offset constant, and throws
     * otherwise; checked here without initializing the struct class (which loads LWJGL natives).
     */
    @Test
    void everyFeatureIsAVulkan10FeatureMember() throws Exception {
        Class<?> features = Class.forName("org.lwjgl.vulkan.VkPhysicalDeviceFeatures", false, RawFeatureTest.class.getClassLoader());
        for (RawFeature feature : RawFeature.values()) {
            Method getter = features.getMethod(feature.vkName());
            assertEquals(boolean.class, getter.getReturnType(), feature.vkName());
            Field offset = features.getField(feature.vkName().toUpperCase(Locale.ROOT));
            assertEquals(int.class, offset.getType(), feature.vkName());
            assertTrue(Modifier.isStatic(offset.getModifiers()), feature.vkName());
        }
    }

    @Test
    void theFeaturesTheRawPathAndPackPipelinesNeedAreRequested() {
        Set<String> names = Set.copyOf(Arrays.stream(RawFeature.values()).map(RawFeature::vkName).toList());
        assertTrue(names.containsAll(Set.of("independentBlend", "fragmentStoresAndAtomics", "shaderStorageImageWriteWithoutFormat",
            "geometryShader", "tessellationShader")), names.toString());
    }

    @Test
    void independentBlendLiftsTheBlendingLimitOfPackPipelines() {
        assertFalse(EnabledFeatures.none().capabilities().independentBlend());
        PipelineCapabilities caps = new EnabledFeatures(EnumSet.of(RawFeature.INDEPENDENT_BLEND), 0, 0).capabilities();
        assertTrue(caps.independentBlend());
        assertEquals(PipelineCapabilities.DEFAULT_MAX_DESCRIPTORS, caps.maxDescriptors());
    }
}
