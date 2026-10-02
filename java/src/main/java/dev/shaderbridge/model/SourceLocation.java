package dev.shaderbridge.model;

/**
 * A location in an original pack file.
 *
 * @param file   path relative to {@code shaders/}, with {@code /} separators
 * @param line   1-based line
 * @param column 1-based column, or null
 */
public record SourceLocation(String file, int line, Integer column) {
    @Override
    public String toString() {
        return column == null ? file + ":" + line : file + ":" + line + ":" + column;
    }
}
