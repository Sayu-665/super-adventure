package dev.shaderbridge.model;

/**
 * One resource binding.
 *
 * @param name     canonical GLSL name in translated shaders
 * @param set      descriptor set
 * @param binding  binding within the set
 * @param kind     descriptor kind
 * @param resource what the host binds
 */
public record BindingEntry(String name, int set, int binding, ResourceKind kind, ResourceRef resource) {
    public BindingEntry {
        Copies.required(name, "name");
        Copies.required(kind, "kind");
        Copies.required(resource, "resource");
    }
}
