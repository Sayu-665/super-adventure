package dev.shaderbridge.natives;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;

/**
 * Child process of {@link NativeExtractorTest#concurrentProcessesNeverLoseTheLibraryTheyAreLoading}:
 * one "game instance" that extracts and "loads" (reads back) libraries in a shared root.
 *
 * <p>Arguments: root, library file name, iterations, seed. Exits with 1 when the loader ever sees
 * a missing or incomplete library.
 */
final class ExtractorChild {
    private ExtractorChild() {
    }

    public static void main(String[] args) throws Exception {
        Path root = Path.of(args[0]);
        String fileName = args[1];
        int iterations = Integer.parseInt(args[2]);
        int seed = Integer.parseInt(args[3]);
        for (int i = 0; i < iterations; i++) {
            byte[] library = library((seed + i) % 3);
            NativeExtractor.extractAndLoad(library, root, fileName, path -> {
                try {
                    // Widen the window in which a racing cleanup could remove the file.
                    Thread.sleep(2);
                    if (!Arrays.equals(library, Files.readAllBytes(path))) {
                        throw new IllegalStateException("incomplete library at " + path);
                    }
                } catch (IOException e) {
                    throw new UncheckedIOException(e);
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    throw new IllegalStateException(e);
                }
            });
        }
    }

    /** A library big enough that writing it is not instantaneous. */
    private static byte[] library(int version) {
        byte[] data = new byte[1 << 20];
        Arrays.fill(data, (byte) version);
        byte[] tag = ("version " + version).getBytes(StandardCharsets.UTF_8);
        System.arraycopy(tag, 0, data, 0, tag.length);
        return data;
    }
}
