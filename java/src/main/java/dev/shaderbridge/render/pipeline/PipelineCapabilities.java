package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.ShaderStage;
import java.nio.ByteBuffer;
import java.util.List;

/**
 * What the device lets renderpearl pipelines do beyond Mojang's baseline. Minecraft 26.3 does not
 * enable {@code independentBlend} on Vulkan: without it every color attachment of a pipeline must
 * have the same write mask and blend state, so a program that leaves an attachment of the shared
 * gbuffers pass untouched cannot be expressed. The raw-Vulkan integration enables the feature at
 * device creation and reports it here, with the SPIR-V capabilities the device can execute; the
 * OpenGL backend always masks per attachment.
 *
 * @param independentBlend per-attachment write masks and blend enables are allowed
 * @param maxDescriptors   descriptors one pipeline may use (Mojang's Vulkan backend binds them as
 *                         push descriptors)
 * @param modules          which SPIR-V modules the device can run
 */
public record PipelineCapabilities(boolean independentBlend, int maxDescriptors, ModuleSupport modules) {
    /** Push descriptors Mojang's Vulkan backend can bind per pipeline on every supported device. */
    public static final int DEFAULT_MAX_DESCRIPTORS = 32;

    /**
     * Capabilities without a SPIR-V capability check ({@link ModuleSupport#ANY}).
     *
     * @param independentBlend per-attachment write masks and blend enables are allowed
     * @param maxDescriptors   descriptors one pipeline may use
     */
    public PipelineCapabilities(boolean independentBlend, int maxDescriptors) {
        this(independentBlend, maxDescriptors, ModuleSupport.ANY);
    }

    /** @return Mojang's limits without device feature knowledge: no independent blend, no SPIR-V capability check */
    public static PipelineCapabilities baseline() {
        return new PipelineCapabilities(false, DEFAULT_MAX_DESCRIPTORS);
    }

    /** Which SPIR-V modules a device can run, by the capabilities they declare. */
    @FunctionalInterface
    public interface ModuleSupport {
        /** Accepts every module (a backend that reports failures itself, such as OpenGL's shader compiler). */
        ModuleSupport ANY = (module, stage) -> List.of();

        /**
         * @param module a SPIR-V module (read, not consumed)
         * @param stage  its stage
         * @return why the device cannot run it, empty if it can
         */
        List<String> problems(ByteBuffer module, ShaderStage stage);
    }
}
