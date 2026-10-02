package dev.shaderbridge.model;

import java.util.List;

/**
 * A resource used by a program.
 *
 * @param name    canonical resource name (key into the {@link BindingTable})
 * @param set     descriptor set
 * @param binding binding within the set
 * @param useAlt  read the alt buffer of a ping-ponged colortex
 * @param stages  stages that use the resource
 */
public record BindingUse(String name, int set, int binding, boolean useAlt, List<ShaderStage> stages) {
    public BindingUse {
        Copies.required(name, "name");
        stages = Copies.list(stages);
    }
}
