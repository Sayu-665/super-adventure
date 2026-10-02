package dev.shaderbridge.model;

import dev.shaderbridge.model.json.OmitIfNull;

/**
 * Device capabilities relevant to translation.
 *
 * @param geometryShader                  geometry shaders are supported
 * @param tessellationShader              tessellation shaders are supported
 * @param storageImageReadWithoutFormat   {@code shaderStorageImageReadWithoutFormat}
 * @param storageImageWriteWithoutFormat  {@code shaderStorageImageWriteWithoutFormat}
 * @param depthClipControl                {@code VK_EXT_depth_clip_control} is available
 * @param maxPushConstantsSize            {@code maxPushConstantsSize} in bytes
 * @param maxColorAttachments             {@code maxColorAttachments}
 * @param comparisonSamplers              the host can create depth-comparison samplers (default true)
 * @param maxDescriptorsPerProgram        hard per-program descriptor limit, or null if none
 */
public record DeviceCaps(
    boolean geometryShader,
    boolean tessellationShader,
    boolean storageImageReadWithoutFormat,
    boolean storageImageWriteWithoutFormat,
    boolean depthClipControl,
    int maxPushConstantsSize,
    int maxColorAttachments,
    boolean comparisonSamplers,
    @OmitIfNull Integer maxDescriptorsPerProgram
) {
}
