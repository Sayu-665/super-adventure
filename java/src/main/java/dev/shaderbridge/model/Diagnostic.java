package dev.shaderbridge.model;

import dev.shaderbridge.model.json.OmitIfNull;

/**
 * An error, warning or note with a location in the original pack sources.
 *
 * @param severity severity
 * @param code     stable machine-readable code, e.g. {@code spv.compile}
 * @param message  human-readable message
 * @param location original file and line, or null
 * @param program  program the diagnostic belongs to, or null
 * @param stage    stage the diagnostic belongs to, or null
 */
public record Diagnostic(Severity severity, String code, String message, @OmitIfNull SourceLocation location,
    @OmitIfNull String program, @OmitIfNull ShaderStage stage) {
    public Diagnostic {
        Copies.required(severity, "severity");
        Copies.required(code, "code");
        Copies.required(message, "message");
    }

    /** @return the diagnostic in the same format as the Rust {@code Display} implementation */
    @Override
    public String toString() {
        StringBuilder out = new StringBuilder(severity.wireName()).append('[').append(code).append(']');
        if (program != null) {
            out.append(' ').append(program);
            if (stage != null) {
                out.append(" (").append(stage.wireName()).append(')');
            }
        }
        if (location != null) {
            out.append(" at ").append(location);
        }
        return out.append(": ").append(message).toString();
    }
}
