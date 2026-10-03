package dev.shaderbridge.config;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.io.StringReader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.Properties;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class PackOptionValuesTest {
    @TempDir
    Path dir;

    @Test
    void readsAnIrisSettingsFile() throws IOException {
        Path file = PackOptionValues.fileFor(dir, "BSL_v8.2.09.zip");
        assertEquals("BSL_v8.2.09.zip.txt", file.getFileName().toString());
        Files.writeString(file, "#Fri Oct 02 12:00:00 CEST 2026\nSHADOW=false\nAO_STRENGTH=1.25\nsunPathRotation=-30.0\n", StandardCharsets.ISO_8859_1);
        PackOptionValues values = PackOptionValues.read(file);
        assertEquals(Map.of("SHADOW", "false", "AO_STRENGTH", "1.25", "sunPathRotation", "-30.0"), values.asMap());
        assertEquals("AO_STRENGTH=1.25\nSHADOW=false\nsunPathRotation=-30.0\n", values.toSettingsText());
    }

    /** Iris saves with {@code Properties.store(out, null)}: a date comment, escapes, ISO-8859-1. */
    @Test
    void readsWhatIrisWrites() throws IOException {
        Properties iris = new Properties();
        iris.setProperty("SHADOW_RES", "2048");
        iris.setProperty("NAME", "a=b: c #d é");
        iris.setProperty("FLAG", "false");
        Path file = dir.resolve("pack.zip.txt");
        try (var out = Files.newOutputStream(file)) {
            iris.store(out, null);
        }
        PackOptionValues values = PackOptionValues.read(file);
        assertEquals(Map.of("SHADOW_RES", "2048", "NAME", "a=b: c #d é", "FLAG", "false"), values.asMap());
        values.write(file);
        Properties back = new Properties();
        try (var in = Files.newInputStream(file)) {
            back.load(in);
        }
        assertEquals(iris, back, "Iris reads the file ShaderBridge writes back");
    }

    @Test
    void missingFileIsEmpty() throws IOException {
        assertTrue(PackOptionValues.read(dir.resolve("none.txt")).isEmpty());
    }

    @Test
    void writeIsReadableByProperties() throws IOException {
        PackOptionValues values = PackOptionValues.empty()
            .with("COLOR", " leading space")
            .with("PATH", "a=b:c#d!e\\f")
            .with("UNICODE", "é中");
        Path file = dir.resolve("pack.txt");
        values.write(file);
        Properties properties = new Properties();
        try (var in = Files.newInputStream(file)) {
            properties.load(in);
        }
        assertEquals(" leading space", properties.getProperty("COLOR"));
        assertEquals("a=b:c#d!e\\f", properties.getProperty("PATH"));
        assertEquals("é中", properties.getProperty("UNICODE"));
        assertEquals(values, PackOptionValues.read(file));
    }

    @Test
    void settingsTextRoundTrips() throws IOException {
        PackOptionValues values = PackOptionValues.empty().with("A B", "x y").with("Z", "1");
        Properties properties = new Properties();
        properties.load(new StringReader(values.toSettingsText()));
        assertEquals("x y", properties.getProperty("A B"));
        assertEquals(values, PackOptionValues.parse(values.toSettingsText()));
    }

    @Test
    void emptyValuesDeleteTheFile() throws IOException {
        Path file = dir.resolve("pack.txt");
        PackOptionValues.empty().with("A", "1").write(file);
        assertTrue(Files.exists(file));
        PackOptionValues.read(file).without("A").write(file);
        assertFalse(Files.exists(file));
    }
}
