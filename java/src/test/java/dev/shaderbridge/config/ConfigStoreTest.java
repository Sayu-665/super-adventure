package dev.shaderbridge.config;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.DepthMode;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ConfigStoreTest {
    @TempDir
    Path dir;

    @Test
    void missingFileGivesDefaults() {
        ConfigStore store = ConfigStore.load(dir.resolve("config/shaderbridge.json"));
        assertEquals(ShaderBridgeConfig.DEFAULT, store.get());
        assertFalse(store.get().isActive());
    }

    @Test
    void updateSavesAtomicallyAndReloads() throws IOException {
        Path file = dir.resolve("config/shaderbridge.json");
        ConfigStore store = ConfigStore.load(file);
        store.update(c -> c.withSelectedPack("BSL.zip").withEnabled(true));
        assertTrue(Files.isRegularFile(file));
        try (var files = Files.list(file.getParent())) {
            assertEquals(1, files.count(), "no temporary file is left behind");
        }
        ShaderBridgeConfig reloaded = ConfigStore.load(file).get();
        assertEquals("BSL.zip", reloaded.selectedPack());
        assertTrue(reloaded.isActive());
    }

    @Test
    void missingAndMistypedKeysUseDefaults() {
        ShaderBridgeConfig config = ConfigStore.fromJson("{\"enabled\": false, \"depthMode\": \"sideways\", \"compileThreads\": \"many\", \"validate\": true}");
        assertFalse(config.enabled());
        assertTrue(config.validate());
        assertEquals(DepthModeSetting.AUTO, config.depthMode());
        assertEquals(0, config.compileThreads());
        assertTrue(config.showDiagnosticsInChat());
        assertNull(config.selectedPack());
    }

    @Test
    void roundTripsEveryField() {
        ShaderBridgeConfig config = new ShaderBridgeConfig(false, "Pack One", DepthModeSetting.FORWARD, true, true, 6, false);
        assertEquals(config, ConfigStore.fromJson(ConfigStore.toJson(config)));
        assertTrue(ConfigStore.toJson(config).contains("\"depthMode\": \"forward\""));
    }

    @Test
    void invalidFileFallsBackToDefaults() throws IOException {
        Path file = dir.resolve("shaderbridge.json");
        Files.writeString(file, "[not, an object");
        assertEquals(ShaderBridgeConfig.DEFAULT, ConfigStore.load(file).get());
    }

    @Test
    void depthModeResolution() {
        assertEquals(DepthMode.REVERSED_ZERO_TO_ONE, DepthModeSetting.AUTO.resolve(true));
        assertEquals(DepthMode.GL_NEG_ONE_TO_ONE, DepthModeSetting.AUTO.resolve(false));
        assertEquals(DepthMode.FORWARD_ZERO_TO_ONE, DepthModeSetting.FORWARD.resolve(true));
        assertEquals(DepthMode.REVERSED_ZERO_TO_ONE, DepthModeSetting.REVERSED.resolve(false));
    }
}
