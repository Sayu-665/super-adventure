package dev.shaderbridge.model;

import java.util.List;

/**
 * A resource used by a program.
 *
 * @param name           canonical resource name (key into the {@link BindingTable})
 * @param set            descriptor set
 * @param binding        binding within the set
 * @param useAlt         read the alt buffer of a ping-ponged colortex
 * @param stages         stages that use the resource
 * @param shadowEmulated the pack declares this sampler as a depth-comparison sampler
 *                       ({@code sampler2DShadow}) but the program compares in the shader because the
 *                       host has no comparison samplers ({@link DeviceCaps#comparisonSamplers()} false):
 *                       the shaders declare a plain {@code sampler2D}, so the depth texture is bound
 *                       with a plain, non-comparison sampler (the binding's
 *                       {@link ResourceKind.Sampler#shadow()} is false accordingly). serde
 *                       {@code #[serde(default)]}: false when absent.
 */
public record BindingUse(String name, int set, int binding, boolean useAlt, List<ShaderStage> stages, boolean shadowEmulated) {
    public BindingUse {
        Copies.required(name, "name");
        stages = Copies.list(stages);
    }

    /**
     * A use without comparison emulation.
     *
     * @param name    canonical resource name
     * @param set     descriptor set
     * @param binding binding within the set
     * @param useAlt  read the alt buffer of a ping-ponged colortex
     * @param stages  stages that use the resource
     */
    public BindingUse(String name, int set, int binding, boolean useAlt, List<ShaderStage> stages) {
        this(name, set, binding, useAlt, stages, false);
    }
}
