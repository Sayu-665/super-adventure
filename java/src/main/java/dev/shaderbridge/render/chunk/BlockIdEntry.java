package dev.shaderbridge.render.chunk;

import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.Set;

/**
 * One entry of a {@code block.<id>=} line of a pack's {@code block.properties}, as the
 * {@code CompiledPack} id maps keep them: {@code [%][namespace:]name[:property=value[,value...]]...}.
 * A leading {@code %} names a block tag instead of a block, a missing namespace means
 * {@code minecraft}, and every {@code property=value} pair filters the block states (a state
 * matches when its value of the property is one of the listed values; OptiFine allows several,
 * Iris one). Pairs without exactly one {@code =} are ignored, as Iris does.
 *
 * @param id         the pack id ({@code mc_Entity.x})
 * @param tag        the entry names a block tag
 * @param namespace  namespace of the block or tag, lower case
 * @param path       path of the block or tag, lower case
 * @param properties required values per property name, in entry order
 */
public record BlockIdEntry(int id, boolean tag, String namespace, String path, Map<String, Set<String>> properties) {
    public BlockIdEntry {
        properties = Collections.unmodifiableMap(new LinkedHashMap<>(properties));
    }

    /**
     * @param id  the pack id of the line
     * @param raw one whitespace-separated entry of the line
     * @return the entry, or empty if it names no block or tag
     */
    public static Optional<BlockIdEntry> parse(int id, String raw) {
        String text = raw.strip();
        boolean tag = text.startsWith("%");
        if (tag) {
            text = text.substring(1);
        }
        if (text.isEmpty()) {
            return Optional.empty();
        }
        String[] segments = text.split(":", -1);
        String namespace;
        String path;
        int properties;
        if (segments.length == 1 || segments[1].contains("=")) {
            namespace = "minecraft";
            path = segments[0];
            properties = 1;
        } else {
            namespace = segments[0];
            path = segments[1];
            properties = 2;
        }
        if (namespace.isEmpty() || path.isEmpty() || path.contains("=")) {
            return Optional.empty();
        }
        Map<String, Set<String>> filters = new LinkedHashMap<>();
        for (int i = properties; i < segments.length; i++) {
            String[] pair = segments[i].split("=", -1);
            if (pair.length != 2 || pair[0].isEmpty() || pair[1].isEmpty()) {
                continue;
            }
            Set<String> values = new LinkedHashSet<>();
            for (String value : pair[1].split(",")) {
                if (!value.isEmpty()) {
                    values.add(value);
                }
            }
            if (!values.isEmpty()) {
                filters.put(pair[0], Collections.unmodifiableSet(values));
            }
        }
        return Optional.of(new BlockIdEntry(id, tag, namespace.toLowerCase(Locale.ROOT), path.toLowerCase(Locale.ROOT), filters));
    }

    /** @return {@code namespace:path} */
    public String name() {
        return namespace + ":" + path;
    }
}
