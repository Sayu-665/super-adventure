package dev.shaderbridge.config;

import java.io.IOException;
import java.io.StringReader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.NoSuchFileException;
import java.nio.file.Path;
import java.util.Collections;
import java.util.Map;
import java.util.Optional;
import java.util.Properties;
import java.util.SortedMap;
import java.util.TreeMap;

/**
 * The user's option values of one pack, stored like Iris does in {@code shaderpacks/<pack>.txt}:
 * a {@link Properties} file (ISO-8859-1, {@code NAME=value} lines) that holds only the values that
 * differ from the pack defaults. Existing Iris settings files therefore carry over unchanged.
 * Instances are immutable; entries are kept sorted by name.
 */
public final class PackOptionValues {
    private static final String HEADER = "# ShaderBridge / Iris shader pack settings\n";

    private final SortedMap<String, String> values;

    private PackOptionValues(SortedMap<String, String> values) {
        this.values = Collections.unmodifiableSortedMap(values);
    }

    /** @return no changed values */
    public static PackOptionValues empty() {
        return new PackOptionValues(new TreeMap<>());
    }

    /**
     * @param shaderpacksDir the {@code shaderpacks} directory
     * @param packName       pack file name, including {@code .zip} for zip packs
     * @return the settings file of the pack, {@code <shaderpacks>/<packName>.txt}
     */
    public static Path fileFor(Path shaderpacksDir, String packName) {
        return shaderpacksDir.resolve(packName + ".txt");
    }

    /**
     * @param file a settings file
     * @return its values; empty if the file does not exist
     * @throws IOException if the file exists but cannot be read
     */
    public static PackOptionValues read(Path file) throws IOException {
        try {
            return parse(new String(Files.readAllBytes(file), StandardCharsets.ISO_8859_1));
        } catch (NoSuchFileException e) {
            return empty();
        }
    }

    /**
     * @param text {@link Properties} text, e.g. a settings file or the native normalized settings
     * @return the values
     */
    public static PackOptionValues parse(String text) {
        Properties properties = new Properties();
        try {
            properties.load(new StringReader(text));
        } catch (IOException e) {
            throw new IllegalStateException("StringReader does not fail", e);
        }
        SortedMap<String, String> values = new TreeMap<>();
        for (String name : properties.stringPropertyNames()) {
            values.put(name, properties.getProperty(name));
        }
        return new PackOptionValues(values);
    }

    /** @return the values by option name, sorted */
    public Map<String, String> asMap() {
        return values;
    }

    /**
     * @param name option name
     * @return the stored value, if the option was changed
     */
    public Optional<String> get(String name) {
        return Optional.ofNullable(values.get(name));
    }

    /**
     * @param name  option name
     * @param value new value
     * @return a copy with the option set
     */
    public PackOptionValues with(String name, String value) {
        SortedMap<String, String> copy = new TreeMap<>(values);
        copy.put(name, value);
        return new PackOptionValues(copy);
    }

    /**
     * @param name option name
     * @return a copy without the option (it reverts to the pack default)
     */
    public PackOptionValues without(String name) {
        SortedMap<String, String> copy = new TreeMap<>(values);
        copy.remove(name);
        return new PackOptionValues(copy);
    }

    /** @return true if no option is changed */
    public boolean isEmpty() {
        return values.isEmpty();
    }

    /** @return the values as {@code NAME=value} lines, the format the native compiler takes */
    public String toSettingsText() {
        StringBuilder out = new StringBuilder();
        values.forEach((name, value) -> out.append(escape(name, true)).append('=').append(escape(value, false)).append('\n'));
        return out.toString();
    }

    /**
     * Saves the values atomically. Like Iris, an empty set deletes the file.
     *
     * @param file the settings file
     * @throws IOException if the file cannot be written or deleted
     */
    public void write(Path file) throws IOException {
        if (values.isEmpty()) {
            Files.deleteIfExists(file);
        } else {
            AtomicFiles.write(file, HEADER + toSettingsText(), StandardCharsets.ISO_8859_1);
        }
    }

    @Override
    public boolean equals(Object o) {
        return o instanceof PackOptionValues other && values.equals(other.values);
    }

    @Override
    public int hashCode() {
        return values.hashCode();
    }

    @Override
    public String toString() {
        return values.toString();
    }

    /** Escapes like {@link Properties#store}, so that {@link Properties#load} reads the text back. */
    private static String escape(String text, boolean key) {
        StringBuilder out = new StringBuilder(text.length());
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            switch (c) {
                case ' ' -> out.append(key || i == 0 ? "\\ " : " ");
                case '\t' -> out.append("\\t");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\f' -> out.append("\\f");
                case '\\', '=', ':', '#', '!' -> out.append('\\').append(c);
                default -> {
                    if (c < 0x20 || c > 0x7e) {
                        out.append(String.format("\\u%04X", (int) c));
                    } else {
                        out.append(c);
                    }
                }
            }
        }
        return out.toString();
    }
}
