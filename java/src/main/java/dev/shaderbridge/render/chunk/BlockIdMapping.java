package dev.shaderbridge.render.chunk;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;

/**
 * Resolves a pack's {@code block.properties} ids to block states with Iris' precedence:
 * every block entry is applied before any tag entry, each in file order, and a state keeps the
 * first id that matches it (OptiFine parity). A property filter naming a property the block does
 * not have is ignored for that block, as Iris does; the other filters still apply. Entries naming
 * blocks or tags the game does not know are skipped (packs list blocks of many mods and versions).
 * Independent of Minecraft so it can be unit-tested; {@link BlockIdTable} binds it to the game's
 * registries.
 */
public final class BlockIdMapping {
    private BlockIdMapping() {
    }

    /**
     * The blocks the game knows, as seen by the resolution.
     *
     * @param <S> block state type
     */
    public interface Catalog<S> {
        /**
         * @param namespace block namespace
         * @param path      block path
         * @return every state of the block, empty if there is no such block
         */
        List<S> blockStates(String namespace, String path);

        /**
         * @param namespace tag namespace
         * @param path      tag path
         * @return the states of every block in the block tag, one list per block, empty if there
         *     is no such tag
         */
        List<List<S>> tagStates(String namespace, String path);

        /**
         * @param state    a block state
         * @param property a property name
         * @return the state's value of the property as it is written in block state strings, or
         *     null if the state's block has no such property
         */
        String value(S state, String property);
    }

    /**
     * Outcome of {@link #resolve}.
     *
     * @param ids            the id of every mapped state
     * @param entries        entries applied
     * @param unknownEntries entries that name no known block or tag, or do not parse
     * @param <S>            block state type
     */
    public record Resolution<S>(Map<S, Integer> ids, int entries, int unknownEntries) {
    }

    /**
     * @param blocks  the pack's block id map ({@code IdMaps.blocks()}): id to raw entries, in file
     *                order
     * @param catalog the known blocks
     * @param <S>     block state type
     * @return the id of every state some entry matches
     */
    public static <S> Resolution<S> resolve(Map<Integer, List<String>> blocks, Catalog<S> catalog) {
        List<BlockIdEntry> direct = new ArrayList<>();
        List<BlockIdEntry> tags = new ArrayList<>();
        int unknown = 0;
        for (Map.Entry<Integer, List<String>> line : blocks.entrySet()) {
            if (line.getKey() == null || line.getValue() == null) {
                continue;
            }
            for (String raw : line.getValue()) {
                Optional<BlockIdEntry> entry = raw == null ? Optional.empty() : BlockIdEntry.parse(line.getKey(), raw);
                if (entry.isEmpty()) {
                    unknown++;
                } else if (entry.get().tag()) {
                    tags.add(entry.get());
                } else {
                    direct.add(entry.get());
                }
            }
        }
        Map<S, Integer> ids = new HashMap<>();
        int applied = 0;
        for (BlockIdEntry entry : direct) {
            List<S> states = catalog.blockStates(entry.namespace(), entry.path());
            if (states.isEmpty()) {
                unknown++;
                continue;
            }
            applied++;
            apply(entry, states, catalog, ids);
        }
        for (BlockIdEntry entry : tags) {
            List<List<S>> members = catalog.tagStates(entry.namespace(), entry.path());
            if (members.isEmpty()) {
                unknown++;
                continue;
            }
            applied++;
            for (List<S> states : members) {
                apply(entry, states, catalog, ids);
            }
        }
        return new Resolution<>(ids, applied, unknown);
    }

    private static <S> void apply(BlockIdEntry entry, List<S> states, Catalog<S> catalog, Map<S, Integer> ids) {
        for (S state : states) {
            if (matches(entry, state, catalog)) {
                ids.putIfAbsent(state, entry.id());
            }
        }
    }

    /**
     * @return whether the state passes every filter of the entry whose property its block has
     */
    static <S> boolean matches(BlockIdEntry entry, S state, Catalog<S> catalog) {
        for (Map.Entry<String, Set<String>> filter : entry.properties().entrySet()) {
            String value = catalog.value(state, filter.getKey());
            if (value != null && !filter.getValue().contains(value)) {
                return false;
            }
        }
        return true;
    }
}
