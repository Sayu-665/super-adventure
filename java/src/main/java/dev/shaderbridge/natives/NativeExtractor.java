package dev.shaderbridge.natives;

import java.io.IOException;
import java.nio.channels.FileChannel;
import java.nio.channels.FileLock;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Arrays;
import java.util.Comparator;
import java.util.HexFormat;
import java.util.function.Consumer;
import java.util.regex.Pattern;
import java.util.stream.Stream;

/**
 * Extracts a native library into a content-addressed directory ({@code <root>/<sha256>/<file>})
 * and loads it, so that concurrently running game instances never overwrite a library another
 * instance has loaded, and an updated mod never loads a stale library.
 *
 * <p>Concurrency between processes (several game instances sharing one game directory, possibly
 * with different mod versions) uses an advisory lock on {@code <root>/.lock}:
 * <ul>
 *   <li>extracting and loading hold a <em>shared</em> lock, so any number of instances may do it
 *       at once. The library only appears at its final path through an atomic rename of a fully
 *       written temporary file, so a reader sees either no file or the complete file;</li>
 *   <li>removing other versions requires the <em>exclusive</em> lock, taken without waiting
 *       afterwards. It therefore never runs while any instance is between extracting and loading.
 *       A library that is already loaded may be removed safely on Linux and macOS (the mapping
 *       stays valid); on Windows a loaded DLL is locked and its removal is skipped.</li>
 * </ul>
 * On file systems without lock support the library is still extracted and loaded, and cleanup is
 * skipped. Within one JVM, calls are serialized.
 */
public final class NativeExtractor {
    /** Name of the lock file in the extraction root. */
    static final String LOCK_FILE = ".lock";

    private static final Pattern VERSION_DIR = Pattern.compile("[0-9a-f]{64}");
    private static final String TEMP_SUFFIX = ".tmp";
    /** Java file locks are per process: overlapping locks from two threads of one JVM throw. */
    private static final Object JVM_LOCK = new Object();

    private NativeExtractor() {
    }

    /**
     * Writes the library unless an identical copy already exists, loads it, then removes other
     * versions and leftovers of interrupted extractions (best effort).
     *
     * @param library  the library bytes
     * @param root     extraction root, e.g. {@code <gameDir>/shaderbridge/natives}
     * @param fileName the platform file name, e.g. {@code libsb_jni.so}
     * @param loader   loads the extracted file, e.g. with {@code System.load}; called while other
     *                 instances cannot remove it. Its unchecked exceptions and errors propagate.
     * @return the path of the extracted library
     * @throws IOException if the library cannot be written
     */
    public static Path extractAndLoad(byte[] library, Path root, String fileName, Consumer<Path> loader) throws IOException {
        if (fileName.isEmpty() || fileName.contains("/") || fileName.contains("\\") || fileName.startsWith(".")) {
            throw new IllegalArgumentException("Invalid library file name '" + fileName + "'");
        }
        String hash = sha256(library);
        Path target = root.resolve(hash).resolve(fileName);
        synchronized (JVM_LOCK) {
            Files.createDirectories(root);
            try (FileChannel lock = FileChannel.open(root.resolve(LOCK_FILE), StandardOpenOption.CREATE, StandardOpenOption.READ,
                StandardOpenOption.WRITE)) {
                FileLock shared = lockShared(lock);
                try {
                    write(library, target);
                    loader.accept(target);
                } finally {
                    if (shared != null) {
                        shared.release();
                    }
                }
                if (shared != null) {
                    cleanUp(lock, root, hash);
                }
            }
        }
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

    /** @return the shared lock, or null if the file system does not support locking */
    private static FileLock lockShared(FileChannel channel) {
        try {
            return channel.lock(0, Long.MAX_VALUE, true);
        } catch (IOException | UnsupportedOperationException e) {
            return null;
        }
    }

    private static void write(byte[] library, Path target) throws IOException {
        if (isIntact(target, library)) {
            return;
        }
        Path dir = Files.createDirectories(target.getParent());
        Path temp = Files.createTempFile(dir, target.getFileName() + ".", TEMP_SUFFIX);
        try {
            Files.write(temp, library);
            moveIntoPlace(temp, target, library);
        } finally {
            Files.deleteIfExists(temp);
        }
    }

    private static boolean isIntact(Path target, byte[] library) throws IOException {
        return Files.isRegularFile(target) && Files.size(target) == library.length && Arrays.equals(Files.readAllBytes(target), library);
    }

    /**
     * Renames the complete temporary file onto the target. When another process placed the same
     * library there first (and, on Windows, already loaded and thereby locked it), the move fails
     * but the target is intact, which is success.
     */
    private static void moveIntoPlace(Path temp, Path target, byte[] library) throws IOException {
        try {
            try {
                Files.move(temp, target, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
            } catch (AtomicMoveNotSupportedException e) {
                Files.move(temp, target, StandardCopyOption.REPLACE_EXISTING);
            }
        } catch (IOException e) {
            if (!isIntact(target, library)) {
                throw e;
            }
        }
    }

    /**
     * With the exclusive lock (skipped if another instance is extracting), removes version
     * directories (exactly 64 hex digits, nothing else) other than {@code keep}, and temporary
     * files of extractions that crashed.
     */
    private static void cleanUp(FileChannel channel, Path root, String keep) {
        FileLock exclusive;
        try {
            exclusive = channel.tryLock();
        } catch (IOException e) {
            return;
        }
        if (exclusive == null) {
            return;
        }
        try (DirectoryStream<Path> versions = Files.newDirectoryStream(root, Files::isDirectory)) {
            for (Path version : versions) {
                String name = version.getFileName().toString();
                if (!VERSION_DIR.matcher(name).matches()) {
                    continue;
                }
                if (name.equals(keep)) {
                    deleteTemps(version);
                } else {
                    deleteTree(version);
                }
            }
        } catch (IOException ignored) {
            // Stale versions are only disk clutter.
        } finally {
            try {
                exclusive.release();
            } catch (IOException ignored) {
                // Released when the channel closes.
            }
        }
    }

    private static void deleteTemps(Path dir) {
        try (DirectoryStream<Path> temps = Files.newDirectoryStream(dir, "*" + TEMP_SUFFIX)) {
            for (Path temp : temps) {
                Files.deleteIfExists(temp);
            }
        } catch (IOException ignored) {
            // See cleanUp.
        }
    }

    private static void deleteTree(Path dir) {
        try (Stream<Path> paths = Files.walk(dir)) {
            for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) {
                Files.deleteIfExists(path);
            }
        } catch (IOException ignored) {
            // A loaded DLL is locked on Windows; see cleanUp.
        }
    }
}
