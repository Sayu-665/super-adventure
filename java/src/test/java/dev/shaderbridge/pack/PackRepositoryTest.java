package dev.shaderbridge.pack;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.io.OutputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.zip.ZipEntry;
import java.util.zip.ZipOutputStream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class PackRepositoryTest {
    @TempDir
    Path dir;

    private void zip(String name, String... entries) throws IOException {
        try (OutputStream out = Files.newOutputStream(dir.resolve(name)); ZipOutputStream zip = new ZipOutputStream(out)) {
            for (String entry : entries) {
                zip.putNextEntry(new ZipEntry(entry));
                zip.closeEntry();
            }
        }
    }

    @Test
    void javaScanFindsDirectoriesAndZips() throws IOException {
        Files.createDirectories(dir.resolve("Zeta/shaders"));
        Files.createDirectories(dir.resolve("alpha/notshaders"));
        zip("Beta.zip", "shaders/final.fsh");
        zip("gamma.zip", "Gamma v1/shaders/composite.fsh");
        zip("delta.zip", "readme.txt");
        Files.writeString(dir.resolve("Broken.zip"), "not a zip");
        Files.writeString(dir.resolve("Beta.zip.txt"), "SHADOWS=false");

        List<PackEntry> packs = new PackRepository(dir).scan();
        assertEquals(List.of("alpha", "Beta.zip", "Broken.zip", "delta.zip", "gamma.zip", "Zeta"), packs.stream().map(PackEntry::name).toList());
        assertFalse(packs.get(0).valid());
        assertEquals(PackKind.DIR, packs.get(0).kind());
        assertTrue(packs.get(1).valid());
        assertNull(packs.get(1).error());
        assertEquals(PackKind.ZIP, packs.get(1).kind());
        assertFalse(packs.get(2).valid());
        assertTrue(packs.get(2).error().contains("corrupted") || packs.get(2).error().contains("cannot be read"));
        assertFalse(packs.get(3).valid());
        assertTrue(packs.get(4).valid());
        assertTrue(packs.get(5).valid());
        assertEquals(dir.resolve("Zeta").toAbsolutePath(), packs.get(5).file());
    }

    @Test
    void createsAMissingDirectory() {
        Path missing = dir.resolve("shaderpacks");
        assertEquals(List.of(), new PackRepository(missing).scan());
        assertTrue(Files.isDirectory(missing));
        assertTrue(new PackRepository(missing).find("nope").isEmpty());
    }
}
