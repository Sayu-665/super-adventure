package dev.shaderbridge.uniforms;

import dev.shaderbridge.natives.ShaderBridgeNative;
import dev.shaderbridge.pack.PackException;
import dev.shaderbridge.pack.PackSession;
import java.nio.ByteBuffer;
import java.util.Optional;

/**
 * The native evaluator of a dimension's custom uniforms ({@code uniform.<type>.<name>} and
 * {@code variable.*} in {@code shaders.properties}). It writes the custom members of an
 * {@code sb_Frame} block whose builtin members are already filled.
 */
public final class UniformEvaluator implements AutoCloseable {
    private long handle;

    private UniformEvaluator(long handle) {
        this.handle = handle;
    }

    /**
     * @param session the session that compiled the pack
     * @param folder  the dimension's world folder
     * @return the evaluator, or empty if the folder has no custom uniforms
     * @throws PackException if the session is closed
     */
    public static Optional<UniformEvaluator> create(PackSession session, String folder) throws PackException {
        long handle = session.withHandle(h -> ShaderBridgeNative.createUniformEvaluator(h, folder));
        return handle == 0 ? Optional.empty() : Optional.of(new UniformEvaluator(handle));
    }

    /**
     * Evaluates the custom uniforms in place.
     *
     * @param frameBlock        direct buffer holding the whole {@code sb_Frame} block
     * @param frameDeltaSeconds time since the previous evaluation (drives {@code smooth()})
     * @return the native error message, or empty on success
     */
    public Optional<String> evaluate(ByteBuffer frameBlock, float frameDeltaSeconds) {
        if (handle == 0) {
            return Optional.of("the evaluator is closed");
        }
        if (ShaderBridgeNative.evaluateUniforms(handle, frameBlock, frameDeltaSeconds)) {
            return Optional.empty();
        }
        String error = ShaderBridgeNative.lastError();
        return Optional.of(error == null ? "unknown error" : error);
    }

    @Override
    public void close() {
        if (handle != 0) {
            ShaderBridgeNative.destroyUniformEvaluator(handle);
            handle = 0;
        }
    }
}
