package dev.shaderbridge.render.raw;

/**
 * The Vulkan 1.0 device features ShaderBridge asks Minecraft to enable, in addition to its own:
 * per-attachment blending and write masks for pack pipelines, and what translated pack shaders
 * may declare as SPIR-V capabilities (geometry and tessellation stages, stores and atomics outside
 * compute, format-less and extended-format storage images, 64- and 16-bit types, ...). Each is
 * enabled only when the device supports it; programs needing a missing one are rejected by
 * {@link SpirvCapabilities}. The names are the members of {@code VkPhysicalDeviceFeatures}.
 */
public enum RawFeature {
    INDEPENDENT_BLEND("independentBlend"),
    GEOMETRY_SHADER("geometryShader"),
    TESSELLATION_SHADER("tessellationShader"),
    FRAGMENT_STORES_AND_ATOMICS("fragmentStoresAndAtomics"),
    VERTEX_PIPELINE_STORES_AND_ATOMICS("vertexPipelineStoresAndAtomics"),
    STORAGE_IMAGE_EXTENDED_FORMATS("shaderStorageImageExtendedFormats"),
    STORAGE_IMAGE_READ_WITHOUT_FORMAT("shaderStorageImageReadWithoutFormat"),
    STORAGE_IMAGE_WRITE_WITHOUT_FORMAT("shaderStorageImageWriteWithoutFormat"),
    IMAGE_CUBE_ARRAY("imageCubeArray"),
    IMAGE_GATHER_EXTENDED("shaderImageGatherExtended"),
    TESSELLATION_AND_GEOMETRY_POINT_SIZE("shaderTessellationAndGeometryPointSize"),
    SAMPLE_RATE_SHADING("sampleRateShading"),
    CLIP_DISTANCE("shaderClipDistance"),
    CULL_DISTANCE("shaderCullDistance"),
    FLOAT64("shaderFloat64"),
    INT64("shaderInt64"),
    INT16("shaderInt16"),
    UNIFORM_BUFFER_ARRAY_DYNAMIC_INDEXING("shaderUniformBufferArrayDynamicIndexing"),
    SAMPLED_IMAGE_ARRAY_DYNAMIC_INDEXING("shaderSampledImageArrayDynamicIndexing"),
    STORAGE_BUFFER_ARRAY_DYNAMIC_INDEXING("shaderStorageBufferArrayDynamicIndexing"),
    STORAGE_IMAGE_ARRAY_DYNAMIC_INDEXING("shaderStorageImageArrayDynamicIndexing");

    private final String vkName;

    RawFeature(String vkName) {
        this.vkName = vkName;
    }

    /** @return the member name in {@code VkPhysicalDeviceFeatures} */
    public String vkName() {
        return vkName;
    }
}
