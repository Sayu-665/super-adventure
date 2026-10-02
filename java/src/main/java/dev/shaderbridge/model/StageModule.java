package dev.shaderbridge.model;

/**
 * One compiled shader stage. Payloads live in the blob buffer.
 *
 * @param stage           shader stage
 * @param entryPoint      entry point name
 * @param spirv           SPIR-V for {@link OutputTarget#VULKAN}, or null
 * @param glslVulkan      translated GLSL for {@link OutputTarget#VULKAN}, or null
 * @param glslRenderpearl translated GLSL for {@link OutputTarget#RENDERPEARL}, or null
 * @param sourceFile      original pack file (relative to {@code shaders/})
 */
public record StageModule(
    ShaderStage stage,
    String entryPoint,
    BlobId spirv,
    BlobId glslVulkan,
    BlobId glslRenderpearl,
    String sourceFile
) {
    public StageModule {
        Copies.required(stage, "stage");
    }
}
