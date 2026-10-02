package dev.shaderbridge.model;

/**
 * A vertex attribute.
 *
 * @param location attribute location
 * @param name     attribute name (equals the host vertex format element name)
 * @param ty       GLSL type name
 * @param semantic semantic it feeds ({@code position}, {@code color}, ...), or null
 */
public record VertexInput(int location, String name, String ty, String semantic) {
}
