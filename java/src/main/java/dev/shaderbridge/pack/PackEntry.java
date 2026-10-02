package dev.shaderbridge.pack;

import java.nio.file.Path;

/**
 * A pack found in the {@code shaderpacks} directory (an element of the native {@code listPacks} JSON).
 *
 * @param name  file name, used as the pack's identity (including {@code .zip})
 * @param path  absolute path of the directory or zip file
 * @param kind  directory or zip
 * @param valid the pack has a {@code shaders/} directory and can be opened
 * @param error why the pack is invalid, or null
 */
public record PackEntry(String name, String path, PackKind kind, boolean valid, String error) {
    public PackEntry {
        if (name == null || path == null || kind == null) {
            throw new IllegalArgumentException("pack entries need a name, path and kind");
        }
    }

    /** @return {@link #path()} as a path */
    public Path file() {
        return Path.of(path);
    }
}
