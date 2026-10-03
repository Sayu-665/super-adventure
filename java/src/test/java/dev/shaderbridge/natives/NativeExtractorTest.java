package dev.shaderbridge.natives;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.File;
import java.io.IOException;
import java.net.URISyntaxException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.TimeUnit;
import java.util.stream.Stream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class NativeExtractorTest {
    private static final String FILE = "libsb_jni.so";

    @TempDir
    Path root;

    private Path extract(String content) throws IOException {
        byte[] library = content.getBytes(StandardCharsets.UTF_8);
        return NativeExtractor.extractAndLoad(library, root, FILE, path -> assertLoadable(path, library));
    }

    private static void assertLoadable(Path path, byte[] library) {
        try {
            assertArrayEquals(library, Files.readAllBytes(path), "the loader sees the complete library");
        } catch (IOException e) {
            throw new AssertionError(e);
        }
    }

    @Test
    void extractsIntoAContentAddressedDirectory() throws IOException {
        Path path = extract("fake library v1");
        assertEquals(root.resolve(NativeExtractor.sha256("fake library v1".getBytes(StandardCharsets.UTF_8))).resolve(FILE), path);
        assertArrayEquals("fake library v1".getBytes(StandardCharsets.UTF_8), Files.readAllBytes(path));
    }

    @Test
    void repairsATruncatedCopy() throws IOException {
        Path path = extract("fake library v1");
        Files.writeString(path, "broken");
        extract("fake library v1");
        assertEquals("fake library v1", Files.readString(path));
    }

    @Test
    void removesOtherVersionsAndAbandonedTemporaryFilesOnly() throws IOException {
        Path old = extract("v1");
        Path foreign = Files.createDirectories(root.resolve("not-a-version"));
        Path current = extract("v2");
        Path abandoned = Files.writeString(current.resolveSibling(FILE + ".123.tmp"), "partial");
        extract("v2");
        assertFalse(Files.exists(old.getParent()), "other versions are removed");
        assertFalse(Files.exists(abandoned), "temporary files of crashed extractions are removed");
        assertTrue(Files.isDirectory(foreign), "directories that are not versions are left alone");
        assertTrue(Files.exists(current));
        try (Stream<Path> files = Files.list(current.getParent())) {
            assertEquals(1, files.count(), "no temporary files are left behind");
        }
    }

    @Test
    void loaderFailuresPropagateAndReleaseTheLock() throws IOException {
        byte[] library = "v1".getBytes(StandardCharsets.UTF_8);
        UnsatisfiedLinkError error = assertThrows(UnsatisfiedLinkError.class,
            () -> NativeExtractor.extractAndLoad(library, root, FILE, path -> {
                throw new UnsatisfiedLinkError("wrong architecture");
            }));
        assertEquals("wrong architecture", error.getMessage());
        assertTrue(Files.exists(extract("v2")), "a later call can lock again");
    }

    @Test
    void rejectsFileNamesThatEscapeTheVersionDirectory() {
        for (String name : List.of("", "../libsb_jni.so", "a/b.so", "a\\b.dll", ".lock")) {
            assertThrows(IllegalArgumentException.class, () -> NativeExtractor.extractAndLoad(new byte[1], root, name, path -> { }), name);
        }
    }

    /**
     * Several game instances with different mod versions share one game directory and start at
     * the same time. Each child JVM repeatedly extracts one of three versions and, in its loader,
     * checks that the complete library is there while other instances remove stale versions.
     */
    @Test
    void concurrentProcessesNeverLoseTheLibraryTheyAreLoading() throws Exception {
        String java = Path.of(System.getProperty("java.home"), "bin", "java").toString();
        String classpath = String.join(File.pathSeparator, codeSource(NativeExtractor.class), codeSource(ExtractorChild.class));
        List<Process> children = new ArrayList<>();
        for (int i = 0; i < 4; i++) {
            children.add(new ProcessBuilder(java, "-cp", classpath, ExtractorChild.class.getName(), root.toString(), FILE, "25",
                String.valueOf(i))
                .redirectErrorStream(true)
                .start());
        }
        for (Process child : children) {
            assertTrue(child.waitFor(120, TimeUnit.SECONDS), "child finished");
            String output = new String(child.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
            assertEquals(0, child.exitValue(), output);
        }
        try (Stream<Path> files = Files.walk(root)) {
            assertTrue(files.noneMatch(p -> p.getFileName().toString().endsWith(".tmp")), "no temporary files are left behind");
        }
    }

    private static String codeSource(Class<?> type) throws URISyntaxException {
        return Path.of(type.getProtectionDomain().getCodeSource().getLocation().toURI()).toString();
    }

    @Test
    void sha256IsHex() {
        assertEquals("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", NativeExtractor.sha256(new byte[0]));
    }
}
