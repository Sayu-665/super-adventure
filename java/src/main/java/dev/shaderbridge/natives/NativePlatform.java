package dev.shaderbridge.natives;

import java.util.Locale;
import java.util.Optional;

/**
 * Operating system and CPU architecture in the naming of the native jar resources
 * ({@code /natives/<os>-<arch>/<file>}).
 *
 * @param os   {@code linux}, {@code windows} or {@code macos}
 * @param arch {@code x86_64} or {@code aarch64}
 */
public record NativePlatform(String os, String arch) {
    /** Base name of the native library ({@code sb_jni}). */
    public static final String LIBRARY_NAME = "sb_jni";

    /**
     * @return the platform of this JVM, if a native library can exist for it
     */
    public static Optional<NativePlatform> current() {
        return detect(System.getProperty("os.name", ""), System.getProperty("os.arch", ""));
    }

    /**
     * Maps Java's {@code os.name} and {@code os.arch} to the resource naming.
     *
     * @param osName {@code os.name}, e.g. {@code Windows 11}, {@code Mac OS X}, {@code Linux}
     * @param osArch {@code os.arch}, e.g. {@code amd64}, {@code aarch64}
     * @return the platform, or empty if it is not supported
     */
    public static Optional<NativePlatform> detect(String osName, String osArch) {
        String name = osName.toLowerCase(Locale.ROOT);
        String os;
        if (name.startsWith("windows")) {
            os = "windows";
        } else if (name.startsWith("mac") || name.startsWith("darwin")) {
            os = "macos";
        } else if (name.startsWith("linux")) {
            os = "linux";
        } else {
            return Optional.empty();
        }
        String arch = switch (osArch.toLowerCase(Locale.ROOT)) {
            case "amd64", "x86_64", "x64" -> "x86_64";
            case "aarch64", "arm64" -> "aarch64";
            default -> null;
        };
        return arch == null ? Optional.empty() : Optional.of(new NativePlatform(os, arch));
    }

    /** @return the platform's file name of the library, e.g. {@code libsb_jni.so} */
    public String libraryFileName() {
        return switch (os) {
            case "windows" -> LIBRARY_NAME + ".dll";
            case "macos" -> "lib" + LIBRARY_NAME + ".dylib";
            default -> "lib" + LIBRARY_NAME + ".so";
        };
    }

    /** @return the jar resource path of the library, e.g. {@code /natives/linux-x86_64/libsb_jni.so} */
    public String resourcePath() {
        return "/natives/" + this + "/" + libraryFileName();
    }

    /** @return {@code <os>-<arch>} */
    @Override
    public String toString() {
        return os + "-" + arch;
    }
}
