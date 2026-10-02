package dev.shaderbridge.pack;

import com.google.gson.reflect.TypeToken;
import dev.shaderbridge.model.ModelJson;
import dev.shaderbridge.model.json.ModelParseException;
import dev.shaderbridge.natives.NativeLibrary;
import dev.shaderbridge.natives.ShaderBridgeNative;
import java.io.IOException;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Locale;
import java.util.Optional;
import java.util.zip.ZipEntry;
import java.util.zip.ZipException;
import java.util.zip.ZipFile;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * The packs in {@code <gameDir>/shaderpacks/}. Scans through the native {@code listPacks} when the
 * library is loaded (it validates packs the same way the compiler opens them), and with a Java
 * fallback otherwise, so the GUI can list packs even when the native library is missing.
 */
public final class PackRepository {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");
    private static final TypeToken<List<PackEntry>> ENTRY_LIST = new TypeToken<>() { };

    private final Path directory;

    /** @param directory the {@code shaderpacks} directory (created on the first scan if missing) */
    public PackRepository(Path directory) {
        this.directory = directory;
    }

    /** @return the {@code shaderpacks} directory */
    public Path directory() {
        return directory;
    }

    /** @return every pack, valid or not, sorted by name (case-insensitive) */
    public List<PackEntry> scan() {
        try {
            Files.createDirectories(directory);
        } catch (IOException e) {
            LOGGER.warn("Cannot create {}: {}", directory, e.getMessage());
            return List.of();
        }
        List<PackEntry> entries = NativeLibrary.isLoaded() ? scanNative().orElseGet(this::scanJava) : scanJava();
        List<PackEntry> sorted = new ArrayList<>(entries);
        sorted.sort(Comparator.comparing(e -> e.name().toLowerCase(Locale.ROOT)));
        return List.copyOf(sorted);
    }

    /**
     * @param name a pack file name
     * @return the pack, if it exists
     */
    public Optional<PackEntry> find(String name) {
        return scan().stream().filter(e -> e.name().equals(name)).findFirst();
    }

    private Optional<List<PackEntry>> scanNative() {
        String json = ShaderBridgeNative.listPacks(directory.toAbsolutePath().toString());
        if (json == null) {
            LOGGER.warn("Native pack scan failed ({}), using the Java scanner", ShaderBridgeNative.lastError());
            return Optional.empty();
        }
        try {
            return Optional.of(ModelJson.parse(json, ENTRY_LIST));
        } catch (ModelParseException e) {
            LOGGER.warn("Unexpected native pack list, using the Java scanner: {}", e.getMessage());
            return Optional.empty();
        }
    }

    /** Lists directories and zip files the way Iris recognizes packs. */
    List<PackEntry> scanJava() {
        List<PackEntry> entries = new ArrayList<>();
        try (DirectoryStream<Path> children = Files.newDirectoryStream(directory)) {
            for (Path child : children) {
                String name = child.getFileName().toString();
                if (Files.isDirectory(child)) {
                    boolean valid = Files.isDirectory(child.resolve("shaders"));
                    entries.add(new PackEntry(name, child.toAbsolutePath().toString(), PackKind.DIR, valid,
                        valid ? null : "The folder has no shaders/ directory"));
                } else if (name.toLowerCase(Locale.ROOT).endsWith(".zip")) {
                    String error = zipError(child);
                    entries.add(new PackEntry(name, child.toAbsolutePath().toString(), PackKind.ZIP, error == null, error));
                }
            }
        } catch (IOException e) {
            LOGGER.warn("Cannot list {}: {}", directory, e.getMessage());
        }
        return entries;
    }

    /** @return null if the zip has a {@code shaders/} directory at its root or one level below */
    private static String zipError(Path zip) {
        try (ZipFile file = new ZipFile(zip.toFile())) {
            boolean hasShaders = file.stream().map(ZipEntry::getName).anyMatch(PackRepository::isShadersEntry);
            return hasShaders ? null : "The zip has no shaders/ directory";
        } catch (ZipException e) {
            return "The zip is corrupted: " + e.getMessage();
        } catch (IOException e) {
            return "The zip cannot be read: " + e.getMessage();
        }
    }

    private static boolean isShadersEntry(String name) {
        if (name.startsWith("shaders/")) {
            return true;
        }
        int slash = name.indexOf('/');
        return slash > 0 && name.startsWith("shaders/", slash + 1);
    }
}
