package dev.shaderbridge.model;

/**
 * Index into {@link CompiledPack#blobs()} ({@code #[serde(transparent)]}, a bare number in JSON).
 *
 * @param index zero-based blob index
 */
public record BlobId(int index) {
    public BlobId {
        if (index < 0) {
            throw new IllegalArgumentException("negative blob index " + index);
        }
    }
}
