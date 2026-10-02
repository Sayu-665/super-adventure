package dev.shaderbridge.model;

import static org.junit.jupiter.api.Assumptions.assumeTrue;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * A minimal reader for the serde-annotated type declarations in the Rust sources of the
 * repository, used to keep the Java mirror in sync with the contract.
 */
final class RustSource {
    private static final Pattern ITEM = Pattern.compile("(?m)^((?:#\\[[^\\n]*\\]\\n|///[^\\n]*\\n)*)pub (struct|enum) (\\w+) \\{\\n");
    private static final Pattern FIELD = Pattern.compile("(?m)^    pub (\\w+): ");
    private static final Pattern VARIANT = Pattern.compile("(?m)^    (\\w+)\\s*(?:[({,]|$)");
    private static final Pattern SKIPPED_FIELD = Pattern.compile(
        "(?m)^    #\\[serde\\([^\\n]*skip_serializing_if[^\\n]*\\)\\]\\n(?:    (?:///|#\\[)[^\\n]*\\n)*    pub (\\w+): ");

    /** A struct or enum declaration. */
    record Item(String kind, String name, String attributes, String body) {
        /** @return the JSON field names of a struct */
        List<String> fields() {
            List<String> out = new ArrayList<>();
            Matcher m = FIELD.matcher(body);
            while (m.find()) {
                out.add(m.group(1));
            }
            return out;
        }

        /** @return the JSON field names of a struct that serde omits when they are {@code None} */
        List<String> skippedFields() {
            List<String> out = new ArrayList<>();
            Matcher m = SKIPPED_FIELD.matcher(body);
            while (m.find()) {
                out.add(m.group(1));
            }
            return out;
        }

        /** @return the Rust variant identifiers of an enum */
        List<String> variants() {
            List<String> out = new ArrayList<>();
            Matcher m = VARIANT.matcher(body);
            while (m.find()) {
                out.add(m.group(1));
            }
            return out;
        }

        /** @return the serde {@code rename_all} rule, or null */
        String renameAll() {
            Matcher m = Pattern.compile("rename_all = \"(\\w+)\"").matcher(attributes);
            return m.find() ? m.group(1) : null;
        }
    }

    private RustSource() {
    }

    /** @return the repository root (the parent of {@code java/}) */
    static Path repoRoot() {
        String configured = System.getProperty("shaderbridge.repoRoot");
        Path root = configured != null ? Path.of(configured) : Path.of("..").toAbsolutePath().normalize();
        assumeTrue(Files.isDirectory(root.resolve("crates")), "Rust sources not available at " + root);
        return root;
    }

    /**
     * @param relative path below the repository root
     * @return the file's text
     */
    static String read(String relative) throws IOException {
        Path file = repoRoot().resolve(relative);
        assumeTrue(Files.isRegularFile(file), "missing " + file);
        return Files.readString(file);
    }

    /**
     * @param source Rust source text
     * @return every top-level {@code pub struct}/{@code pub enum} with braces, by name
     */
    static Map<String, Item> items(String source) {
        Map<String, Item> out = new LinkedHashMap<>();
        Matcher m = ITEM.matcher(source);
        while (m.find()) {
            int end = source.indexOf("\n}\n", m.end());
            out.put(m.group(3), new Item(m.group(2), m.group(3), m.group(1), source.substring(m.end(), end + 1)));
        }
        return out;
    }
}
