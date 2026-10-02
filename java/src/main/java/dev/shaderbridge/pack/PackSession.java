package dev.shaderbridge.pack;

import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.CompileEnvironment;
import dev.shaderbridge.model.CompiledPack;
import dev.shaderbridge.model.ModelJson;
import dev.shaderbridge.model.OptionsModel;
import dev.shaderbridge.model.json.ModelParseException;
import dev.shaderbridge.natives.NativeLibrary;
import dev.shaderbridge.natives.ShaderBridgeNative;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Path;

/**
 * An open pack in the native library. Calls are serialized per session; long-running calls
 * ({@link #compile}) block and belong on a worker thread. Closing is idempotent, and calls on a
 * closed session fail with a {@link PackException} instead of touching a dead native handle.
 */
public final class PackSession implements AutoCloseable {
    private final Path path;
    private long handle;

    private PackSession(Path path, long handle) {
        this.path = path;
        this.handle = handle;
    }

    /**
     * @param path a pack directory or zip file
     * @return the open session
     * @throws PackException if the native library is unavailable or the pack cannot be opened
     */
    public static PackSession open(Path path) throws PackException {
        if (!NativeLibrary.isLoaded()) {
            throw new PackException("The ShaderBridge native library is not loaded");
        }
        long handle = ShaderBridgeNative.openPack(path.toAbsolutePath().toString());
        if (handle == 0) {
            throw new PackException("Cannot open " + path.getFileName() + ": " + lastError());
        }
        return new PackSession(path, handle);
    }

    /**
     * A session without a native handle, for tests of code that only passes sessions around.
     *
     * @param path a pack path
     * @return a session whose native calls fail
     */
    static PackSession detached(Path path) {
        return new PackSession(path, 0);
    }

    /** @return the pack directory or zip file */
    public Path path() {
        return path;
    }

    /**
     * The raw native handle, for wrappers of handle-taking natives such as the uniform evaluator.
     *
     * @return the handle
     * @throws PackException if the session is closed
     */
    public synchronized long handle() throws PackException {
        if (handle == 0) {
            throw new PackException("The session of " + path.getFileName() + " is closed");
        }
        return handle;
    }

    /**
     * @param language Minecraft language code for lang strings, e.g. {@code en_us}
     * @return the options model
     * @throws PackException if the native call fails or returns an invalid model
     */
    public synchronized OptionsModel options(String language) throws PackException {
        String json = ShaderBridgeNative.getOptions(handle(), language);
        if (json == null) {
            throw new PackException("Cannot read the options of " + path.getFileName() + ": " + lastError());
        }
        return parse(json, OptionsModel.class);
    }

    /**
     * Drops unknown options and normalizes values against the pack.
     *
     * @param optionValues Iris-format settings text
     * @return normalized settings text
     * @throws PackException if the native call fails
     */
    public synchronized String normalizeOptionValues(String optionValues) throws PackException {
        String normalized = ShaderBridgeNative.normalizeOptionValues(handle(), optionValues);
        if (normalized == null) {
            throw new PackException("Cannot normalize the settings of " + path.getFileName() + ": " + lastError());
        }
        return normalized;
    }

    /**
     * Compiles the pack and fetches its binary payloads. Blocks for the duration of the compile.
     *
     * @param environment  the compile environment
     * @param optionValues Iris-format settings text, may be empty
     * @param settings     compile settings
     * @return the compiled model and its blobs
     * @throws PackException if compilation fails outright (per-program problems are diagnostics, not exceptions)
     */
    public synchronized CompileResult compile(CompileEnvironment environment, String optionValues, CompileSettings settings) throws PackException {
        long session = handle();
        String json = ShaderBridgeNative.compile(session, ModelJson.toJson(environment), optionValues, settings.toJson());
        if (json == null) {
            throw new PackException("Compiling " + path.getFileName() + " failed: " + lastError());
        }
        CompiledPack model = parse(json, CompiledPack.class);
        if (model.formatVersion() != CompiledPack.FORMAT_VERSION) {
            throw new PackException("The native library produced model version " + model.formatVersion()
                + ", this mod understands version " + CompiledPack.FORMAT_VERSION);
        }
        long size = ShaderBridgeNative.blobSize(session);
        if (size < 0 || size > Integer.MAX_VALUE) {
            throw new PackException("Invalid blob buffer size " + size + ": " + lastError());
        }
        ByteBuffer buffer = ByteBuffer.allocateDirect((int) size).order(ByteOrder.LITTLE_ENDIAN);
        if (size > 0 && !ShaderBridgeNative.blobData(session, buffer)) {
            throw new PackException("Cannot fetch the compiled shaders of " + path.getFileName() + ": " + lastError());
        }
        try {
            return new CompileResult(model, Blobs.of(buffer, model.blobs()));
        } catch (IllegalArgumentException e) {
            throw new PackException("Inconsistent compiled shaders: " + e.getMessage(), e);
        }
    }

    @Override
    public synchronized void close() {
        if (handle != 0) {
            ShaderBridgeNative.closePack(handle);
            handle = 0;
        }
    }

    private static <T> T parse(String json, Class<T> type) throws PackException {
        try {
            return ModelJson.parse(json, type);
        } catch (ModelParseException e) {
            throw new PackException("The native library returned an invalid " + type.getSimpleName() + ": " + e.getMessage(), e);
        }
    }

    private static String lastError() {
        String error = ShaderBridgeNative.lastError();
        return error == null ? "unknown error" : error;
    }

    /**
     * Output of {@link #compile}.
     *
     * @param model the compiled pack
     * @param blobs its binary payloads
     */
    public record CompileResult(CompiledPack model, Blobs blobs) {
    }
}
