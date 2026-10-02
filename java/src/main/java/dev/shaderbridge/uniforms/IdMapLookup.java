package dev.shaderbridge.uniforms;

import dev.shaderbridge.model.IdMaps;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.function.Function;
import java.util.function.Predicate;

/**
 * Resolves the pack's {@code block.properties}, {@code item.properties} and
 * {@code entity.properties} ids. Block entries follow the grammar {@code [%][namespace:]name[:key=value[,value...]]...}
 * ({@code %} marks a block tag); a missing namespace means {@code minecraft}. When several
 * entries match, the first one in file order wins. Unmapped objects resolve to {@code -1}.
 */
public final class IdMapLookup {
    /** A lookup without any mapping. */
    public static final IdMapLookup EMPTY = new IdMapLookup(Map.of(), List.of(), Map.of(), Map.of());

    private final Map<String, List<BlockEntry>> blocks;
    private final List<BlockEntry> blockTags;
    private final Map<String, Integer> items;
    private final Map<String, Integer> entities;

    /**
     * One parsed block entry.
     *
     * @param id     the pack id
     * @param order  position of the entry in {@code block.properties}
     * @param name   namespaced block or tag id
     * @param tag    the entry names a block tag
     * @param states required property values (any of the listed values matches)
     */
    record BlockEntry(int id, int order, String name, boolean tag, Map<String, Set<String>> states) {
        boolean matches(Function<String, String> propertyValue) {
            for (Map.Entry<String, Set<String>> state : states.entrySet()) {
                String value = propertyValue.apply(state.getKey());
                if (value == null || !state.getValue().contains(value)) {
                    return false;
                }
            }
            return true;
        }
    }

    private IdMapLookup(Map<String, List<BlockEntry>> blocks, List<BlockEntry> blockTags, Map<String, Integer> items, Map<String, Integer> entities) {
        this.blocks = blocks;
        this.blockTags = blockTags;
        this.items = items;
        this.entities = entities;
    }

    /**
     * @param maps the id maps of a compiled pack
     * @return the lookup
     */
    public static IdMapLookup of(IdMaps maps) {
        Map<String, List<BlockEntry>> blocks = new HashMap<>();
        List<BlockEntry> tags = new ArrayList<>();
        int[] order = {0};
        maps.blocks().forEach((id, entries) -> {
            for (String raw : entries) {
                BlockEntry entry = parseBlock(id, order[0]++, raw);
                if (entry.tag()) {
                    tags.add(entry);
                } else {
                    blocks.computeIfAbsent(entry.name(), k -> new ArrayList<>()).add(entry);
                }
            }
        });
        return new IdMapLookup(blocks, tags, simple(maps.items()), simple(maps.entities()));
    }

    /**
     * Parses one block entry such as {@code minecraft:wheat:age=7}, {@code oak_leaves},
     * {@code wheat:age=6,7} or {@code %minecraft:logs}.
     */
    static BlockEntry parseBlock(int id, int order, String raw) {
        boolean tag = raw.startsWith("%");
        String[] segments = (tag ? raw.substring(1) : raw).split(":");
        int statesStart;
        String name;
        if (segments.length == 1 || segments[1].contains("=")) {
            name = "minecraft:" + segments[0];
            statesStart = 1;
        } else {
            name = segments[0] + ":" + segments[1];
            statesStart = 2;
        }
        Map<String, Set<String>> states = new LinkedHashMap<>();
        for (int i = statesStart; i < segments.length; i++) {
            int eq = segments[i].indexOf('=');
            if (eq > 0 && eq < segments[i].length() - 1) {
                states.put(segments[i].substring(0, eq), Set.of(segments[i].substring(eq + 1).split(",")));
            }
        }
        return new BlockEntry(id, order, name, tag, states);
    }

    private static Map<String, Integer> simple(Map<Integer, List<String>> map) {
        Map<String, Integer> out = new HashMap<>();
        map.forEach((id, entries) -> {
            for (String raw : entries) {
                if (!raw.startsWith("%")) {
                    out.putIfAbsent(raw.contains(":") ? raw : "minecraft:" + raw, id);
                }
            }
        });
        return out;
    }

    /**
     * @param namespacedId item id, e.g. {@code minecraft:torch}
     * @return its pack id, or -1
     */
    public int item(String namespacedId) {
        return items.getOrDefault(namespacedId, -1);
    }

    /**
     * @param namespacedId entity type id, e.g. {@code minecraft:zombie}
     * @return its pack id, or -1
     */
    public int entity(String namespacedId) {
        return entities.getOrDefault(namespacedId, -1);
    }

    /**
     * @param namespacedId  block id, e.g. {@code minecraft:wheat}
     * @param propertyValue the block state's value of a property name (null if it has none)
     * @param inTag         whether the block is in a namespaced block tag
     * @return the pack id of the first matching entry, or -1
     */
    public int block(String namespacedId, Function<String, String> propertyValue, Predicate<String> inTag) {
        int best = -1;
        int bestOrder = Integer.MAX_VALUE;
        List<BlockEntry> direct = blocks.getOrDefault(namespacedId, List.of());
        for (BlockEntry entry : direct) {
            if (entry.matches(propertyValue)) {
                best = entry.id();
                bestOrder = entry.order();
                break;
            }
        }
        for (BlockEntry entry : blockTags) {
            if (entry.order() >= bestOrder) {
                break;
            }
            if (inTag.test(entry.name()) && entry.matches(propertyValue)) {
                return entry.id();
            }
        }
        return best;
    }
}
