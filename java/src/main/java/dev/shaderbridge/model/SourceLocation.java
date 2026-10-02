package dev.shaderbridge.model;

import dev.shaderbridge.model.json.OmitIfNull;

/**
 * A location in an original pack file.
 *
 * @param file   path relative to {@code shaders/}, with {@code /} separators
 * @param line   1-based line
 * @param column 1-based column, or null
 */
public record SourceLocation(String file, int line, @OmitIfNull Integer column) {
    @Override
    public String toString() {
        return column == null ? file + ":" + line : file + ":" + line + ":" + column;
    }
}
