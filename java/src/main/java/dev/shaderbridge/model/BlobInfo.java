package dev.shaderbridge.model;

/**
 * Location of one blob in the concatenated blob buffer.
 *
 * @param kind   payload kind
 * @param offset byte offset into the buffer (8-byte aligned)
 * @param len    length in bytes
 */
public record BlobInfo(BlobKind kind, long offset, long len) {
    public BlobInfo {
        Copies.required(kind, "kind");
        if (offset < 0 || len < 0) {
            throw new IllegalArgumentException("negative blob range " + offset + "+" + len);
        }
    }
}
