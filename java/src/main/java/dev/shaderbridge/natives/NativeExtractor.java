package dev.shaderbridge.natives;

import java.io.IOException;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Arrays;
import java.util.Comparator;
import java.util.HexFormat;
import java.util.stream.Stream;

/**
 * Extracts a native library into a content-addressed directory ({@code <root>/<sha256>/<file>}),
 * so that concurrently running game instances never overwrite a library another instance has
 * loaded, and an updated mod never loads a stale library.
 */
public final class NativeExtractor {
    private NativeExtractor() {
    }

    /**
     * Writes the library unless an identical copy already exists, then removes directories of
     * other versions (best effort: a library still loaded by another process may be locked).
     *
     * @param library  the library bytes
     * @param root     extraction root, e.g. {@code <gameDir>/shaderbridge/natives}
     * @param fileName the platform file name, e.g. {@code libsb_jni.so}
     * @return the path of the extracted library
     * @throws IOException if the library cannot be written
     */
    public static Path extract(byte[] library, Path root, String fileName) throws IOException {
        String hash = sha256(library);
        Path dir = root.resolve(hash);
        Path target = dir.resolve(fileName);
        if (!isIntact(target, library)) {
            Files.createDirectories(dir);
            Path temp = Files.createTempFile(dir, fileName, ".tmp");
            try {
                Files.write(temp, library);
                moveIntoPlace(temp, target);
            } finally {
                Files.deleteIfExists(temp);
            }
        }
        deleteOtherVersions(root, hash);
        return target;
    }

    /**
     * @param data bytes to hash
     * @return the lower-case hex SHA-256 of the bytes
     */
    public static String sha256(byte[] data) {
        try {
            return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(data));
        } catch (NoSuchAlgorithmException e) {
            throw new IllegalStateException("SHA-256 is required by the Java platform", e);
        }
    }

    private static boolean isIntact(Path target, byte[] library) throws IOException {
        return Files.isRegularFile(target) && Files.size(target) == library.length && Arrays.equals(Files.readAllBytes(target), library);
    }

    private static void moveIntoPlace(Path temp, Path target) throws IOException {
        try {
            Files.move(temp, target, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
        } catch (AtomicMoveNotSupportedException e) {
            Files.move(temp, target, StandardCopyOption.REPLACE_EXISTING);
        }
    }

    private static void deleteOtherVersions(Path root, String keep) {
        try (DirectoryStream<Path> versions = Files.newDirectoryStream(root, Files::isDirectory)) {
            for (Path version : versions) {
                if (!version.getFileName().toString().equals(keep)) {
                    deleteTree(version);
                }
            }
        } catch (IOException ignored) {
            // Stale versions are only disk clutter; a locked library is expected on Windows.
        }
    }

    private static void deleteTree(Path dir) {
        try (Stream<Path> paths = Files.walk(dir)) {
            for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) {
                Files.deleteIfExists(path);
            }
        } catch (IOException ignored) {
            // See deleteOtherVersions.
        }
    }
}
