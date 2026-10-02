package dev.shaderbridge.natives;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Optional;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Loads the {@code sb-jni} native library and remembers the outcome. Loading never throws: a
 * missing or broken library leaves the mod in the {@link Status.Failed} state, which disables
 * shader packs and is shown in the GUI.
 *
 * <p>The library is taken, in order of precedence, from the system property
 * {@value #PATH_PROPERTY}, the environment variable {@value #PATH_ENV} (both a path to the library
 * file, for development), or the jar resource {@code /natives/<os>-<arch>/<file>}, which is
 * extracted to {@code <gameDir>/shaderbridge/natives/<sha256>/}.
 */
public final class NativeLibrary {
    /** System property overriding the library path. */
    public static final String PATH_PROPERTY = "shaderbridge.native";
    /** Environment variable overriding the library path. */
    public static final String PATH_ENV = "SHADERBRIDGE_NATIVE";

    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");
    private static volatile Status status = new Status.NotLoaded();

    /** Outcome of loading the library. */
    public sealed interface Status {
        /** {@link #load} has not been called yet. */
        record NotLoaded() implements Status {
        }

        /**
         * The library is loaded and answered {@code version()}.
         *
         * @param path    the loaded file
         * @param version the native ShaderBridge version
         */
        record Loaded(Path path, String version) implements Status {
        }

        /**
         * The library could not be loaded.
         *
         * @param reason a message for the user
         */
        record Failed(String reason) implements Status {
        }
    }

    private NativeLibrary() {
    }

    /** @return the current status */
    public static Status status() {
        return status;
    }

    /** @return true if native methods may be called */
    public static boolean isLoaded() {
        return status instanceof Status.Loaded;
    }

    /**
     * Loads the library once; later calls return the first outcome.
     *
     * @param gameDir the game directory (extraction root parent)
     * @return the outcome
     */
    public static synchronized Status load(Path gameDir) {
        if (status instanceof Status.NotLoaded) {
            status = tryLoad(gameDir);
            switch (status) {
                case Status.Loaded loaded -> LOGGER.info("Loaded ShaderBridge native library {} from {}", loaded.version(), loaded.path());
                case Status.Failed failed -> LOGGER.error("ShaderBridge is disabled: {}", failed.reason());
                case Status.NotLoaded ignored -> throw new IllegalStateException("unreachable");
            }
        }
        return status;
    }

    /** Loading a JNI library is a restricted method since Java 22; that is this class's purpose. */
    @SuppressWarnings("restricted")
    private static Status tryLoad(Path gameDir) {
        Path library;
        try {
            Optional<Path> override = override();
            if (override.isPresent()) {
                library = override.get().toAbsolutePath();
                if (!Files.isRegularFile(library)) {
                    return new Status.Failed("The native library override " + library + " does not exist");
                }
            } else {
                Optional<NativePlatform> platform = NativePlatform.current();
                if (platform.isEmpty()) {
                    return new Status.Failed("Unsupported platform " + System.getProperty("os.name") + " / " + System.getProperty("os.arch"));
                }
                Optional<Path> extracted = extractBundled(platform.get(), gameDir.resolve("shaderbridge").resolve("natives"));
                if (extracted.isEmpty()) {
                    return new Status.Failed("This build of ShaderBridge has no native library for " + platform.get());
                }
                library = extracted.get();
            }
        } catch (IOException e) {
            return new Status.Failed("Cannot extract the native library: " + e.getMessage());
        }
        try {
            System.load(library.toString());
            String version = ShaderBridgeNative.version();
            if (version == null) {
                return new Status.Failed("The native library at " + library + " did not report a version");
            }
            return new Status.Loaded(library, version);
        } catch (UnsatisfiedLinkError | SecurityException e) {
            return new Status.Failed("Cannot load the native library " + library + ": " + e.getMessage());
        }
    }

    private static Optional<Path> override() {
        String path = System.getProperty(PATH_PROPERTY);
        if (path == null || path.isBlank()) {
            path = System.getenv(PATH_ENV);
        }
        return path == null || path.isBlank() ? Optional.empty() : Optional.of(Path.of(path));
    }

    private static Optional<Path> extractBundled(NativePlatform platform, Path root) throws IOException {
        try (InputStream in = NativeLibrary.class.getResourceAsStream(platform.resourcePath())) {
            if (in == null) {
                return Optional.empty();
            }
            return Optional.of(NativeExtractor.extract(in.readAllBytes(), root, platform.libraryFileName()));
        }
    }
}
