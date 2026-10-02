package dev.shaderbridge.natives;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class NativeExtractorTest {
    @TempDir
    Path root;

    @Test
    void extractsIntoAContentAddressedDirectory() throws IOException {
        byte[] library = "fake library v1".getBytes(StandardCharsets.UTF_8);
        Path path = NativeExtractor.extract(library, root, "libsb_jni.so");
        assertEquals(root.resolve(NativeExtractor.sha256(library)).resolve("libsb_jni.so"), path);
        assertArrayEquals(library, Files.readAllBytes(path));
    }

    @Test
    void repairsATruncatedCopy() throws IOException {
        byte[] library = "fake library v1".getBytes(StandardCharsets.UTF_8);
        Path path = NativeExtractor.extract(library, root, "libsb_jni.so");
        Files.writeString(path, "broken");
        NativeExtractor.extract(library, root, "libsb_jni.so");
        assertArrayEquals(library, Files.readAllBytes(path));
    }

    @Test
    void removesOtherVersions() throws IOException {
        Path old = NativeExtractor.extract("v1".getBytes(StandardCharsets.UTF_8), root, "libsb_jni.so");
        Path current = NativeExtractor.extract("v2".getBytes(StandardCharsets.UTF_8), root, "libsb_jni.so");
        assertFalse(Files.exists(old.getParent()));
        assertTrue(Files.exists(current));
        try (var files = Files.list(current.getParent())) {
            assertEquals(1, files.count(), "no temporary files are left behind");
        }
    }

    @Test
    void sha256IsHex() {
        assertEquals("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", NativeExtractor.sha256(new byte[0]));
    }
}
