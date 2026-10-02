package dev.shaderbridge.config;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParseException;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.NoSuchFileException;
import java.nio.file.Path;
import java.util.function.UnaryOperator;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Loads and saves {@link ShaderBridgeConfig}. Missing keys take their defaults, so older files
 * keep working; an unreadable file is logged and replaced by the defaults on the next save.
 * Thread-safe: the current value is published through a volatile field.
 */
public final class ConfigStore {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();

    private final Path file;
    private volatile ShaderBridgeConfig current;

    private ConfigStore(Path file, ShaderBridgeConfig current) {
        this.file = file;
        this.current = current;
    }

    /**
     * @param file the config file, usually {@code config/shaderbridge.json}
     * @return a store holding the file's settings, or the defaults if it is missing or invalid
     */
    public static ConfigStore load(Path file) {
        ShaderBridgeConfig config = ShaderBridgeConfig.DEFAULT;
        try {
            config = fromJson(Files.readString(file, StandardCharsets.UTF_8));
        } catch (NoSuchFileException e) {
            LOGGER.info("No ShaderBridge config at {}, using defaults", file);
        } catch (IOException | JsonParseException | IllegalStateException e) {
            LOGGER.warn("Ignoring unreadable ShaderBridge config {}: {}", file, e.getMessage());
        }
        return new ConfigStore(file, config);
    }

    /** @return the current settings */
    public ShaderBridgeConfig get() {
        return current;
    }

    /**
     * Applies a change and saves the file atomically.
     *
     * @param change function from the current to the new settings
     * @return the new settings
     */
    public synchronized ShaderBridgeConfig update(UnaryOperator<ShaderBridgeConfig> change) {
        ShaderBridgeConfig next = change.apply(current);
        current = next;
        try {
            AtomicFiles.write(file, toJson(next), StandardCharsets.UTF_8);
        } catch (IOException e) {
            LOGGER.error("Cannot save ShaderBridge config {}", file, e);
        }
        return next;
    }

    /**
     * @param json config file text
     * @return the settings, with defaults for missing or mistyped keys
     * @throws JsonParseException if the text is not a JSON object
     */
    static ShaderBridgeConfig fromJson(String json) {
        JsonElement root = JsonParser.parseString(json);
        if (!root.isJsonObject()) {
            throw new JsonParseException("the config is not a JSON object");
        }
        JsonObject o = root.getAsJsonObject();
        ShaderBridgeConfig d = ShaderBridgeConfig.DEFAULT;
        return new ShaderBridgeConfig(
            bool(o, "enabled", d.enabled()),
            string(o, "selectedPack", d.selectedPack()),
            DepthModeSetting.fromConfigName(string(o, "depthMode", d.depthMode().configName())).orElse(d.depthMode()),
            bool(o, "validate", d.validate()),
            bool(o, "debugDumpGlsl", d.debugDumpGlsl()),
            integer(o, "compileThreads", d.compileThreads()),
            bool(o, "showDiagnosticsInChat", d.showDiagnosticsInChat()));
    }

    /**
     * @param config settings
     * @return the config file text
     */
    static String toJson(ShaderBridgeConfig config) {
        JsonObject o = new JsonObject();
        o.addProperty("enabled", config.enabled());
        o.addProperty("selectedPack", config.selectedPack());
        o.addProperty("depthMode", config.depthMode().configName());
        o.addProperty("validate", config.validate());
        o.addProperty("debugDumpGlsl", config.debugDumpGlsl());
        o.addProperty("compileThreads", config.compileThreads());
        o.addProperty("showDiagnosticsInChat", config.showDiagnosticsInChat());
        return GSON.toJson(o) + "\n";
    }

    private static boolean bool(JsonObject o, String key, boolean fallback) {
        JsonElement e = o.get(key);
        return e != null && e.isJsonPrimitive() && e.getAsJsonPrimitive().isBoolean() ? e.getAsBoolean() : fallback;
    }

    private static int integer(JsonObject o, String key, int fallback) {
        JsonElement e = o.get(key);
        return e != null && e.isJsonPrimitive() && e.getAsJsonPrimitive().isNumber() ? e.getAsInt() : fallback;
    }

    private static String string(JsonObject o, String key, String fallback) {
        JsonElement e = o.get(key);
        return e != null && e.isJsonPrimitive() && e.getAsJsonPrimitive().isString() ? e.getAsString() : fallback;
    }
}
