package dev.shaderbridge.pack;

import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.CompileEnvironment;
import dev.shaderbridge.model.CompiledPack;
import dev.shaderbridge.model.ModelJson;
import dev.shaderbridge.model.ModelValidation;
import dev.shaderbridge.model.OptionsModel;
import dev.shaderbridge.model.json.ModelParseException;
import dev.shaderbridge.natives.NativeLibrary;
import dev.shaderbridge.natives.ShaderBridgeNative;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Path;
import java.util.List;
import java.util.concurrent.locks.ReentrantLock;
import java.util.function.LongConsumer;

/**
 * An open pack in the native library. Native calls are serialized per session
 * ({@link #withHandle}); long-running calls ({@link #compile}, an on-demand variant compile)
 * block and belong on a worker thread. Closing never waits for them: {@link #close} releases
 * the native session at once when no call runs, and otherwise when the running call returns.
 * Closing is idempotent, and calls on a closed (or closing) session fail with a
 * {@link PackException} instead of touching a dead native handle.
 */
public final class PackSession implements AutoCloseable {
    private final Path path;
    private final LongConsumer closer;
    private final ReentrantLock lock = new ReentrantLock();
    /** Written under {@link #lock}; volatile so {@link #isReleased} needs no lock. */
    private volatile long handle;
    private volatile boolean closeRequested;

    PackSession(Path path, long handle, LongConsumer closer) {
        this.path = path;
        this.handle = handle;
        this.closer = closer;
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
        return new PackSession(path, handle, ShaderBridgeNative::closePack);
    }

    /**
     * A session without a native handle, for tests of code that only passes sessions around.
     *
     * @param path a pack path
     * @return a session whose native calls fail
     */
    static PackSession detached(Path path) {
        return new PackSession(path, 0, h -> { });
    }

    /** A native call on a session's handle. */
    @FunctionalInterface
    public interface HandleCall<T> {
        /**
         * @param handle the open native session
         * @return the call's result
         * @throws PackException if the call fails
         */
        T apply(long handle) throws PackException;
    }

    /**
     * Runs native calls on this session's handle, serialized with every other call on it (the
     * native session keeps per-call buffers, such as a variant's blobs, until the next call). If
     * the session was closed while the call ran, it is released when the call returns.
     *
     * @param call the native calls
     * @param <T>  result type
     * @return the call's result
     * @throws PackException if the session is closed or the call fails
     */
    public <T> T withHandle(HandleCall<T> call) throws PackException {
        lock.lock();
        try {
            if (handle == 0 || closeRequested) {
                throw new PackException("The session of " + path.getFileName() + " is closed");
            }
            return call.apply(handle);
        } finally {
            lock.unlock();
            releaseIfClosed();
        }
    }

    /** @return the pack directory or zip file */
    public Path path() {
        return path;
    }

    /**
     * The raw native handle, for wrappers of handle-taking natives such as the uniform evaluator.
     * Prefer {@link #withHandle}, which keeps the session open during the call.
     *
     * @return the handle
     * @throws PackException if the session is closed
     */
    public long handle() throws PackException {
        return withHandle(h -> h);
    }

    /**
     * @param language Minecraft language code for lang strings, e.g. {@code en_us}
     * @return the options model
     * @throws PackException if the native call fails or returns an invalid model
     */
    public OptionsModel options(String language) throws PackException {
        String json = withHandle(h -> ShaderBridgeNative.getOptions(h, language));
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
    public String normalizeOptionValues(String optionValues) throws PackException {
        String normalized = withHandle(h -> ShaderBridgeNative.normalizeOptionValues(h, optionValues));
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
    public CompileResult compile(CompileEnvironment environment, String optionValues, CompileSettings settings) throws PackException {
        return withHandle(session -> compileOn(session, environment, optionValues, settings));
    }

    private CompileResult compileOn(long session, CompileEnvironment environment, String optionValues, CompileSettings settings) throws PackException {
        String json = ShaderBridgeNative.compile(session, ModelJson.toJson(environment), optionValues, settings.toJson());
        if (json == null) {
            throw new PackException("Compiling " + path.getFileName() + " failed: " + lastError());
        }
        CompiledPack model = parse(json, CompiledPack.class);
        if (model.formatVersion() != CompiledPack.FORMAT_VERSION) {
            throw new PackException("The native library produced model version " + model.formatVersion()
                + ", this mod understands version " + CompiledPack.FORMAT_VERSION);
        }
        List<String> problems = ModelValidation.problems(model);
        if (!problems.isEmpty()) {
            throw new PackException("The native library returned an inconsistent model: " + String.join("; ", problems));
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

    /**
     * Releases the native session: at once when no call runs, otherwise as soon as the running
     * call returns. Never waits for a running call (a compile on a worker thread).
     */
    @Override
    public void close() {
        closeRequested = true;
        releaseIfClosed();
    }

    /** @return whether the native session has been released (never blocks) */
    public boolean isReleased() {
        return handle == 0;
    }

    /**
     * Releases the native session if closing was requested and no call holds it. A call that
     * holds it releases it itself when it returns (it checks after unlocking, and the request is
     * made before {@link #close} tries the lock, so one of the two always sees the other).
     */
    private void releaseIfClosed() {
        if (closeRequested && lock.tryLock()) {
            try {
                if (handle != 0) {
                    long h = handle;
                    handle = 0;
                    closer.accept(h);
                }
            } finally {
                lock.unlock();
            }
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
