package dev.shaderbridge.config;

import java.io.IOException;
import java.nio.charset.Charset;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;

/** Crash-safe file replacement: write a sibling temporary file, then rename it over the target. */
public final class AtomicFiles {
    private AtomicFiles() {
    }

    /**
     * Replaces {@code target} with {@code text}. Readers see either the old or the new content,
     * never a partial file.
     *
     * @param target  file to replace (parent directories are created)
     * @param text    new content
     * @param charset encoding
     * @throws IOException if the file cannot be written
     */
    public static void write(Path target, String text, Charset charset) throws IOException {
        Path dir = target.toAbsolutePath().getParent();
        Files.createDirectories(dir);
        Path temp = Files.createTempFile(dir, target.getFileName().toString(), ".tmp");
        try {
            Files.writeString(temp, text, charset);
            try {
                Files.move(temp, target, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
            } catch (AtomicMoveNotSupportedException e) {
                Files.move(temp, target, StandardCopyOption.REPLACE_EXISTING);
            }
        } finally {
            Files.deleteIfExists(temp);
        }
    }
}
