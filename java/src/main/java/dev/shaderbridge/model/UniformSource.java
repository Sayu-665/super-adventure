package dev.shaderbridge.model;

/** Where the value of a block member comes from ({@code #[serde(tag = "type", content = "name")]}). */
public sealed interface UniformSource {
    /**
     * A builtin uniform provided by the host.
     *
     * @param name name in the sb-uniforms registry
     */
    record Builtin(String name) implements UniformSource {
    }

    /**
     * A custom uniform defined in {@code shaders.properties}, evaluated natively.
     *
     * @param name custom uniform name
     */
    record Custom(String name) implements UniformSource {
    }

    /** Declared by the pack but unknown: zero-filled (or its constant initializer). */
    record Unset() implements UniformSource {
    }
}
