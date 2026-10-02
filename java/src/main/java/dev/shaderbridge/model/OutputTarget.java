package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/** GLSL/SPIR-V flavour the compiler emits (ARCHITECTURE §5). */
public enum OutputTarget implements WireEnum {
    /** Explicit set/binding decorations; SPIR-V produced by ShaderBridge. */
    VULKAN,
    /** Mojang renderpearl conventions: no set/binding, names are the interface. */
    RENDERPEARL
}
