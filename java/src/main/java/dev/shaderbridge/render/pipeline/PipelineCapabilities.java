package dev.shaderbridge.render.pipeline;

/**
 * What the device lets renderpearl pipelines do beyond Mojang's baseline. Minecraft 26.3 does not
 * enable {@code independentBlend} on Vulkan: without it every color attachment of a pipeline must
 * have the same write mask and blend state, so a program that leaves an attachment of the shared
 * gbuffers pass untouched cannot be expressed. The raw-Vulkan integration enables the feature at
 * device creation and reports it here; the OpenGL backend always masks per attachment.
 *
 * @param independentBlend per-attachment write masks and blend enables are allowed
 * @param maxDescriptors   descriptors one pipeline may use (Mojang's Vulkan backend binds them as
 *                         push descriptors)
 */
public record PipelineCapabilities(boolean independentBlend, int maxDescriptors) {
    /** Push descriptors Mojang's Vulkan backend can bind per pipeline on every supported device. */
    public static final int DEFAULT_MAX_DESCRIPTORS = 32;

    /** @return the capabilities of an unmodified Minecraft 26.3 Vulkan device */
    public static PipelineCapabilities baseline() {
        return new PipelineCapabilities(false, DEFAULT_MAX_DESCRIPTORS);
    }
}
