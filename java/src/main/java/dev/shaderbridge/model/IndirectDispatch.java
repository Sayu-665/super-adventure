package dev.shaderbridge.model;

/**
 * {@code indirect.<prog>=<ssbo> <offset>}, serialized as the pair {@code [buffer, offset]}.
 *
 * @param buffer SSBO index holding the dispatch arguments
 * @param offset byte offset of the arguments
 */
public record IndirectDispatch(int buffer, int offset) {
}
