package dev.shaderbridge.model.json;

/** Thrown when JSON produced by the native library does not match the model contract. */
public final class ModelParseException extends Exception {
    /**
     * @param message what was being parsed
     * @param cause   the underlying Gson or validation error
     */
    public ModelParseException(String message, Throwable cause) {
        super(message + ": " + cause.getMessage(), cause);
    }
}
