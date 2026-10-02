package dev.shaderbridge.model;

import dev.shaderbridge.model.json.OmitIfNull;

/**
 * {@code uniform.<type>.<name>=<expr>} or {@code variable.<type>.<name>=<expr>}.
 *
 * @param name       uniform name
 * @param ty         declared type
 * @param expression expression source
 * @param isVariable {@code variable.*} (not uploaded) rather than {@code uniform.*}
 * @param location   definition site, or null
 */
public record CustomUniform(String name, GlslType ty, String expression, boolean isVariable, @OmitIfNull SourceLocation location) {
    public CustomUniform {
        Copies.required(name, "name");
        Copies.required(ty, "ty");
    }
}
