package dev.shaderbridge.model;

/**
 * Resolved program of a geometry type.
 *
 * @param program      index into {@link DimensionPipeline#programs()}
 * @param resolvedFrom the program actually used (may be a fallback)
 */
public record GeometrySlot(int program, GeometryProgram resolvedFrom) {
}
