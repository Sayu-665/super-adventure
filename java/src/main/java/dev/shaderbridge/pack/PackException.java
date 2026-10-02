package dev.shaderbridge.pack;

/** A native pack operation failed; the message is suitable for the user. */
public final class PackException extends Exception {
    /** @param message what failed and why */
    public PackException(String message) {
        super(message);
    }

    /**
     * @param message what failed and why
     * @param cause   the underlying error
     */
    public PackException(String message, Throwable cause) {
        super(message, cause);
    }
}
