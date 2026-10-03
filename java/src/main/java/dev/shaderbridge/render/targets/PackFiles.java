package dev.shaderbridge.render.targets;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Enumeration;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.TreeSet;
import java.util.stream.Stream;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

/**
 * Read-only access to the files of a pack's {@code shaders/} root, for the textures the host
 * loads itself (custom textures, {@code texture.noise}). The root is found like sb-pack finds it:
 * {@code <dir>/shaders}, or the directory itself if it holds programs or
 * {@code shaders.properties}; in a zip, {@code shaders/} at the archive root or one level nested
 * ({@code Name/shaders/}, ignoring {@code __MACOSX/}). Paths are normalized ({@code \} and
 * {@code /} separators, {@code .} segments, a leading {@code /}); paths escaping the root are
 * rejected. A missing path falls back to a case-insensitive match, as packs made on Windows
 * expect.
 */
public abstract sealed class PackFiles implements AutoCloseable {
    /** Largest file read (textures are far smaller). */
    public static final long MAX_FILE_SIZE = 256L << 20;

    private PackFiles() {
    }

    /**
     * @param pack a pack directory or zip file
     * @return its files
     * @throws IOException if the pack cannot be read or has no shaders root
     */
    public static PackFiles open(Path pack) throws IOException {
        if (Files.isDirectory(pack)) {
            return new Directory(locateRoot(pack));
        }
        ZipFile zip = new ZipFile(pack.toFile());
        try {
            return new Zip(zip);
        } catch (IOException | RuntimeException e) {
            zip.close();
            throw e;
        }
    }

    /**
     * @param path a path relative to the shaders root
     * @return the file's bytes, or empty if it does not exist
     * @throws IOException if the file exists but cannot be read, or is too large
     */
    public abstract Optional<byte[]> read(String path) throws IOException;

    /**
     * @param path a pack path
     * @return the normalized path ({@code a/b.png}), or empty if it is empty or escapes the root
     */
    public static Optional<String> normalize(String path) {
        List<String> parts = new ArrayList<>();
        for (String segment : path.replace('\\', '/').split("/")) {
            switch (segment) {
                case "", "." -> {
                }
                case ".." -> {
                    if (parts.isEmpty()) {
                        return Optional.empty();
                    }
                    parts.removeLast();
                }
                default -> parts.add(segment);
            }
        }
        return parts.isEmpty() ? Optional.empty() : Optional.of(String.join("/", parts));
    }

    private static Path locateRoot(Path dir) throws IOException {
        Path nested = dir.resolve("shaders");
        if (Files.isDirectory(nested)) {
            return nested;
        }
        try (Stream<Path> files = Files.list(dir)) {
            if (files.map(p -> p.getFileName().toString().toLowerCase(Locale.ROOT)).anyMatch(PackFiles::marksShadersRoot)) {
                return dir;
            }
        }
        throw new IOException(dir.getFileName() + " has no shaders/ directory");
    }

    private static boolean marksShadersRoot(String fileName) {
        return fileName.equals("shaders.properties") || fileName.matches(".*\\.(vsh|fsh|gsh|csh|tcs|tes)");
    }

    /** A pack directory. */
    private static final class Directory extends PackFiles {
        private final Path root;

        Directory(Path root) {
            this.root = root;
        }

        @Override
        public Optional<byte[]> read(String path) throws IOException {
            Optional<String> normalized = normalize(path);
            if (normalized.isEmpty()) {
                return Optional.empty();
            }
            Path file = root.resolve(normalized.get());
            if (!Files.isRegularFile(file)) {
                Optional<Path> match = caseInsensitive(normalized.get());
                if (match.isEmpty()) {
                    return Optional.empty();
                }
                file = match.get();
            }
            if (Files.size(file) > MAX_FILE_SIZE) {
                throw new IOException(path + " is larger than " + MAX_FILE_SIZE + " bytes");
            }
            return Optional.of(Files.readAllBytes(file));
        }

        private Optional<Path> caseInsensitive(String path) throws IOException {
            Path current = root;
            for (String segment : path.split("/")) {
                Path exact = current.resolve(segment);
                if (Files.exists(exact)) {
                    current = exact;
                    continue;
                }
                if (!Files.isDirectory(current)) {
                    return Optional.empty();
                }
                try (Stream<Path> children = Files.list(current)) {
                    Optional<Path> match = children.filter(c -> c.getFileName().toString().equalsIgnoreCase(segment)).sorted().findFirst();
                    if (match.isEmpty()) {
                        return Optional.empty();
                    }
                    current = match.get();
                }
            }
            return Files.isRegularFile(current) ? Optional.of(current) : Optional.empty();
        }

        @Override
        public void close() {
            // Nothing is held open.
        }
    }

    /** A zipped pack. */
    private static final class Zip extends PackFiles {
        private final ZipFile zip;
        /** Normalized path relative to the root -> entry. */
        private final Map<String, ZipEntry> files = new HashMap<>();
        /** Lower-cased path -> first entry in path order. */
        private final Map<String, ZipEntry> folded = new HashMap<>();

        Zip(ZipFile zip) throws IOException {
            this.zip = zip;
            List<ZipEntry> entries = new ArrayList<>();
            List<String> names = new ArrayList<>();
            for (Enumeration<? extends ZipEntry> e = zip.entries(); e.hasMoreElements(); ) {
                ZipEntry entry = e.nextElement();
                Optional<String> name = normalize(entry.getName());
                if (name.isPresent() && !entry.isDirectory()) {
                    entries.add(entry);
                    names.add(name.get());
                }
            }
            String prefix = rootPrefix(names).orElseThrow(() -> new IOException(zip.getName() + " has no shaders/ directory"));
            TreeSet<String> sorted = new TreeSet<>();
            for (int i = 0; i < entries.size(); i++) {
                if (names.get(i).startsWith(prefix)) {
                    String relative = names.get(i).substring(prefix.length());
                    files.putIfAbsent(relative, entries.get(i));
                    sorted.add(relative);
                }
            }
            for (String relative : sorted) {
                folded.putIfAbsent(relative.toLowerCase(Locale.ROOT), files.get(relative));
            }
        }

        /** {@code shaders/} at the root, else the first {@code <Name>/shaders/} in name order. */
        static Optional<String> rootPrefix(List<String> names) {
            if (names.stream().anyMatch(n -> n.startsWith("shaders/"))) {
                return Optional.of("shaders/");
            }
            TreeSet<String> nested = new TreeSet<>();
            for (String name : names) {
                String[] parts = name.split("/", 3);
                if (parts.length == 3 && parts[1].equals("shaders") && !parts[0].equals("__MACOSX")) {
                    nested.add(parts[0] + "/shaders/");
                }
            }
            return nested.isEmpty() ? Optional.empty() : Optional.of(nested.first());
        }

        @Override
        public Optional<byte[]> read(String path) throws IOException {
            Optional<String> normalized = normalize(path);
            if (normalized.isEmpty()) {
                return Optional.empty();
            }
            ZipEntry entry = files.get(normalized.get());
            if (entry == null) {
                entry = folded.get(normalized.get().toLowerCase(Locale.ROOT));
            }
            if (entry == null) {
                return Optional.empty();
            }
            if (entry.getSize() > MAX_FILE_SIZE) {
                throw new IOException(path + " is larger than " + MAX_FILE_SIZE + " bytes");
            }
            try (InputStream in = zip.getInputStream(entry)) {
                byte[] bytes = in.readNBytes((int) MAX_FILE_SIZE + 1);
                if (bytes.length > MAX_FILE_SIZE) {
                    throw new IOException(path + " is larger than " + MAX_FILE_SIZE + " bytes");
                }
                return Optional.of(bytes);
            }
        }

        @Override
        public void close() throws IOException {
            zip.close();
        }
    }

    @Override
    public abstract void close() throws IOException;
}
