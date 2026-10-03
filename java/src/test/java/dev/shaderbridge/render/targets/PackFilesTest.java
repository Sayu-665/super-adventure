package dev.shaderbridge.render.targets;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.Optional;
import java.util.zip.ZipEntry;
import java.util.zip.ZipOutputStream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class PackFilesTest {
    @TempDir
    Path dir;

    private static byte[] bytes(String s) {
        return s.getBytes(StandardCharsets.UTF_8);
    }

    private Path zip(String name, Map<String, String> entries) throws IOException {
        Path zip = dir.resolve(name);
        try (OutputStream out = Files.newOutputStream(zip); ZipOutputStream z = new ZipOutputStream(out)) {
            for (Map.Entry<String, String> e : entries.entrySet()) {
                z.putNextEntry(new ZipEntry(e.getKey()));
                z.write(bytes(e.getValue()));
                z.closeEntry();
            }
        }
        return zip;
    }

    @Test
    void normalizesPathsAndRejectsEscapes() {
        assertEquals(Optional.of("textures/noise.png"), PackFiles.normalize("/textures/./noise.png"));
        assertEquals(Optional.of("textures/noise.png"), PackFiles.normalize("textures\\lib\\..\\noise.png"));
        assertEquals(Optional.empty(), PackFiles.normalize("../secret"));
        assertEquals(Optional.empty(), PackFiles.normalize("textures/../../secret"));
        assertEquals(Optional.empty(), PackFiles.normalize("/"));
    }

    @Test
    void readsADirectoryPackWithACaseInsensitiveFallback() throws IOException {
        Path shaders = Files.createDirectories(dir.resolve("Pack/shaders/Textures"));
        Files.write(shaders.resolve("Noise.png"), bytes("png"));
        try (PackFiles files = PackFiles.open(dir.resolve("Pack"))) {
            assertArrayEquals(bytes("png"), files.read("Textures/Noise.png").orElseThrow());
            assertArrayEquals(bytes("png"), files.read("textures/noise.png").orElseThrow());
            assertEquals(Optional.empty(), files.read("textures/missing.png"));
            assertEquals(Optional.empty(), files.read("../Pack/shaders/Textures/Noise.png"));
        }
    }

    @Test
    void aDirectoryWithProgramsIsItsOwnRoot() throws IOException {
        Path root = Files.createDirectories(dir.resolve("loose"));
        Files.write(root.resolve("composite.fsh"), bytes("void main() {}"));
        Files.write(root.resolve("lut.bin"), bytes("lut"));
        try (PackFiles files = PackFiles.open(root)) {
            assertArrayEquals(bytes("lut"), files.read("lut.bin").orElseThrow());
        }
        assertThrows(IOException.class, () -> PackFiles.open(Files.createDirectories(dir.resolve("empty"))));
    }

    @Test
    void readsZipsWithARootOrNestedShadersDirectory() throws IOException {
        try (PackFiles files = PackFiles.open(zip("a.zip", Map.of("shaders/textures/a.png", "a", "readme.txt", "x")))) {
            assertArrayEquals(bytes("a"), files.read("textures/a.png").orElseThrow());
            assertArrayEquals(bytes("a"), files.read("TEXTURES/A.PNG").orElseThrow());
            assertEquals(Optional.empty(), files.read("readme.txt"));
        }
        try (PackFiles files = PackFiles.open(zip("b.zip", Map.of("__MACOSX/shaders/x.png", "mac", "My Pack/shaders/x.png", "real")))) {
            assertArrayEquals(bytes("real"), files.read("x.png").orElseThrow());
        }
        assertThrows(IOException.class, () -> PackFiles.open(zip("c.zip", Map.of("textures/a.png", "a"))));
        assertTrue(Files.exists(dir.resolve("c.zip")));
    }
}
