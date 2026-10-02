package dev.shaderbridge.natives;

import java.nio.ByteBuffer;

/**
 * JNI bindings of {@code sb-jni}. This class mirrors the native contract exactly; use the wrappers
 * in {@code dev.shaderbridge.pack} instead of calling it directly.
 *
 * <p>Every method reports failure as {@code 0}, {@code null} or {@code false} and records the
 * reason in a thread-local error readable with {@link #lastError()}. Handles are opaque ids into a
 * native registry, never raw pointers: a closed or unknown handle is an error, not undefined
 * behaviour. Compile methods block for the duration of the compile and must not run on the render
 * thread. Methods may only be called after {@link NativeLibrary#load} succeeded.
 */
public final class ShaderBridgeNative {
    private ShaderBridgeNative() {
    }

    /** @return the ShaderBridge version string, e.g. {@code 0.1.0} */
    public static native String version();

    /** @return the last error message on this thread, or null */
    public static native String lastError();

    /**
     * Lists shader packs (directories and {@code .zip} files) in a directory.
     *
     * @param shaderpacksDir the {@code shaderpacks} directory
     * @return JSON array of {@code {"name", "path", "kind": "dir"|"zip", "valid", "error"}}, or null
     */
    public static native String listPacks(String shaderpacksDir);

    /**
     * @param packPath a pack directory or zip file
     * @return a session handle, or 0 on failure
     */
    public static native long openPack(String packPath);

    /** @param session a session handle; closing twice is harmless */
    public static native void closePack(long session);

    /**
     * @param session  a session handle
     * @param language Minecraft language code for lang strings, e.g. {@code en_us}
     * @return {@code sb_core::model::OptionsModel} JSON, or null
     */
    public static native String getOptions(long session, String language);

    /**
     * Compiles the pack. Binary payloads are fetched afterwards with {@link #blobData}.
     *
     * @param session      a session handle
     * @param envJson      {@code sb_core::model::CompileEnvironment} JSON
     * @param optionValues Iris-format settings text ({@code NAME=value} lines), may be empty
     * @param settingsJson {@code {"dimensions": [...]|null, "validate": bool, "cacheDir": String|null}}
     * @return CompiledPack JSON with its {@code blobs} index filled, or null on failure
     */
    public static native String compile(long session, String envJson, String optionValues, String settingsJson);

    /**
     * @param session a session handle
     * @return size in bytes of the concatenated blob buffer of the last successful compile
     */
    public static native long blobSize(long session);

    /**
     * @param session a session handle
     * @param dst     direct buffer with {@code remaining() >= blobSize(session)}
     * @return false on error
     */
    public static native boolean blobData(long session, ByteBuffer dst);

    /**
     * Compiles one extra (geometry program, draw profile) variant of a folder after {@link #compile}.
     *
     * @param session         a session handle
     * @param folder          world folder
     * @param geometryProgram geometry program
     * @param profile         draw profile id
     * @return {@code {"program": Program, "blobs": [BlobInfo...]}}, or null on failure
     */
    public static native String compileVariant(long session, String folder, String geometryProgram, String profile);

    /**
     * @param session a session handle
     * @param dst     direct buffer with {@code remaining() >= variantBlobSize(session)}
     * @return false on error
     */
    public static native boolean variantBlobData(long session, ByteBuffer dst);

    /**
     * @param session a session handle
     * @return size in bytes of the blob buffer of the last variant
     */
    public static native long variantBlobSize(long session);

    /**
     * Registers an extra draw profile for all subsequent compiles of all sessions.
     *
     * @param toml profile definition (see sb-transform {@code profiles/README.md})
     * @return null on success, or an error message
     */
    public static native String registerProfile(String toml);

    /**
     * @param session a session handle
     * @param folder  a compiled world folder
     * @return an evaluator handle, or 0 if the folder is unknown or has no custom uniforms
     */
    public static native long createUniformEvaluator(long session, String folder);

    /**
     * Evaluates the custom uniforms in place. Builtin-sourced members must already be written.
     *
     * @param evaluator          an evaluator handle
     * @param frameBlock         direct buffer holding the {@code sb_Frame} block (at least {@code frame.size} bytes)
     * @param frameDeltaSeconds  time since the previous evaluation, for {@code smooth()}
     * @return false on error
     */
    public static native boolean evaluateUniforms(long evaluator, ByteBuffer frameBlock, float frameDeltaSeconds);

    /** @param evaluator an evaluator handle; destroying twice is harmless */
    public static native void destroyUniformEvaluator(long evaluator);

    /**
     * Normalizes Iris-format settings text against the pack options.
     *
     * @param session      a session handle
     * @param optionValues Iris-format settings text
     * @return normalized settings text with unknown options dropped, or null
     */
    public static native String normalizeOptionValues(long session, String optionValues);
}
