package dev.shaderbridge.render.chunk;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import org.junit.jupiter.api.Test;

class BlockIdMappingTest {
    /** A block state of the fake catalog. */
    private record State(String block, Map<String, String> properties) {
    }

    /** A few blocks and tags with Minecraft's names. */
    private static final class Catalog implements BlockIdMapping.Catalog<State> {
        final Map<String, List<State>> blocks = new LinkedHashMap<>();
        final Map<String, List<String>> tags = new LinkedHashMap<>();

        Catalog() {
            block("minecraft:stone");
            block("minecraft:oak_leaves");
            block("minecraft:birch_leaves");
            block("minecraft:short_grass");
            List<State> wheat = new ArrayList<>();
            for (int age = 0; age <= 7; age++) {
                wheat.add(new State("minecraft:wheat", Map.of("age", Integer.toString(age))));
            }
            blocks.put("minecraft:wheat", wheat);
            blocks.put("minecraft:lantern", List.of(new State("minecraft:lantern", Map.of("hanging", "true", "waterlogged", "false")),
                new State("minecraft:lantern", Map.of("hanging", "true", "waterlogged", "true")),
                new State("minecraft:lantern", Map.of("hanging", "false", "waterlogged", "false")),
                new State("minecraft:lantern", Map.of("hanging", "false", "waterlogged", "true"))));
            blocks.put("minecraft:water", List.of(new State("minecraft:water", Map.of("level", "0")), new State("minecraft:water", Map.of("level", "8"))));
            block("modded:crystal");
            tags.put("minecraft:leaves", List.of("minecraft:oak_leaves", "minecraft:birch_leaves"));
            tags.put("minecraft:crops", List.of("minecraft:wheat"));
        }

        private void block(String name) {
            blocks.put(name, List.of(new State(name, Map.of())));
        }

        State state(String block, Map<String, String> properties) {
            return blocks.get(block).stream().filter(s -> s.properties().equals(properties)).findFirst().orElseThrow();
        }

        @Override
        public List<State> blockStates(String namespace, String path) {
            return blocks.getOrDefault(namespace + ":" + path, List.of());
        }

        @Override
        public List<List<State>> tagStates(String namespace, String path) {
            return tags.getOrDefault(namespace + ":" + path, List.of()).stream().map(blocks::get).toList();
        }

        @Override
        public String value(State state, String property) {
            return state.properties().get(property);
        }
    }

    private static final Catalog CATALOG = new Catalog();

    private static BlockIdMapping.Resolution<State> resolve(Map<Integer, List<String>> lines) {
        return BlockIdMapping.resolve(lines, CATALOG);
    }

    @Test
    void entriesParseTheBlockPropertiesGrammar() {
        BlockIdEntry plain = BlockIdEntry.parse(1, "stone").orElseThrow();
        assertEquals("minecraft:stone", plain.name());
        assertFalse(plain.tag());
        assertEquals(Map.of(), plain.properties());
        BlockIdEntry implicit = BlockIdEntry.parse(2, "wheat:age=7").orElseThrow();
        assertEquals("minecraft:wheat", implicit.name());
        assertEquals(Map.of("age", Set.of("7")), implicit.properties());
        BlockIdEntry full = BlockIdEntry.parse(3, "Modded:Crystal:lit=true:facing=north,south,north:bad:=x:y=").orElseThrow();
        assertEquals("modded:crystal", full.name(), "ids are lower case");
        assertEquals(Map.of("lit", Set.of("true"), "facing", Set.of("north", "south")), full.properties(), "malformed filters are ignored");
        BlockIdEntry tag = BlockIdEntry.parse(4, "%leaves").orElseThrow();
        assertTrue(tag.tag());
        assertEquals("minecraft:leaves", tag.name());
        BlockIdEntry namespacedTag = BlockIdEntry.parse(5, "%c:ores:lit=false").orElseThrow();
        assertEquals("c:ores", namespacedTag.name());
        assertEquals(Map.of("lit", Set.of("false")), namespacedTag.properties());
        assertEquals(Optional.empty(), BlockIdEntry.parse(6, ""));
        assertEquals(Optional.empty(), BlockIdEntry.parse(6, "%"));
        assertEquals(Optional.empty(), BlockIdEntry.parse(6, ":stone"));
        assertEquals(Optional.empty(), BlockIdEntry.parse(6, "age=7"));
    }

    @Test
    void theFirstMatchingBlockEntryWins() {
        Map<Integer, List<String>> lines = new LinkedHashMap<>();
        lines.put(10010, List.of("minecraft:wheat:age=7"));
        lines.put(10020, List.of("wheat", "stone"));
        lines.put(10030, List.of("minecraft:stone"));
        BlockIdMapping.Resolution<State> r = resolve(lines);
        assertEquals(10010, r.ids().get(CATALOG.state("minecraft:wheat", Map.of("age", "7"))));
        assertEquals(10020, r.ids().get(CATALOG.state("minecraft:wheat", Map.of("age", "3"))));
        assertEquals(10020, r.ids().get(CATALOG.state("minecraft:stone", Map.of())), "a later line does not override");
        assertEquals(4, r.entries());
        assertEquals(0, r.unknownEntries());
    }

    @Test
    void blockEntriesBeatTagsWhateverTheirOrder() {
        // Iris applies every block entry before any tag entry.
        Map<Integer, List<String>> lines = new LinkedHashMap<>();
        lines.put(10001, List.of("%minecraft:leaves"));
        lines.put(10002, List.of("minecraft:birch_leaves"));
        lines.put(10003, List.of("%crops:age=7"));
        BlockIdMapping.Resolution<State> r = resolve(lines);
        assertEquals(10002, r.ids().get(CATALOG.state("minecraft:birch_leaves", Map.of())));
        assertEquals(10001, r.ids().get(CATALOG.state("minecraft:oak_leaves", Map.of())));
        assertEquals(10003, r.ids().get(CATALOG.state("minecraft:wheat", Map.of("age", "7"))));
        assertNull(r.ids().get(CATALOG.state("minecraft:wheat", Map.of("age", "6"))), "tag filters apply");
    }

    @Test
    void propertyFilters() {
        Map<Integer, List<String>> lines = new LinkedHashMap<>();
        // OptiFine lists: any listed value matches.
        lines.put(10010, List.of("wheat:age=6,7"));
        // Unfiltered properties match any value: a hanging lantern is matched waterlogged or not.
        lines.put(10020, List.of("lantern:hanging=true"));
        // A property the block does not have is ignored (Iris), the others still filter.
        lines.put(10030, List.of("lantern:lit=true:hanging=false", "stone:axis=y"));
        BlockIdMapping.Resolution<State> r = resolve(lines);
        assertEquals(10010, r.ids().get(CATALOG.state("minecraft:wheat", Map.of("age", "6"))));
        assertEquals(10010, r.ids().get(CATALOG.state("minecraft:wheat", Map.of("age", "7"))));
        assertNull(r.ids().get(CATALOG.state("minecraft:wheat", Map.of("age", "5"))));
        assertEquals(10020, r.ids().get(CATALOG.state("minecraft:lantern", Map.of("hanging", "true", "waterlogged", "true"))));
        assertEquals(10020, r.ids().get(CATALOG.state("minecraft:lantern", Map.of("hanging", "true", "waterlogged", "false"))));
        assertEquals(10030, r.ids().get(CATALOG.state("minecraft:lantern", Map.of("hanging", "false", "waterlogged", "true"))));
        assertEquals(10030, r.ids().get(CATALOG.state("minecraft:stone", Map.of())));
    }

    @Test
    void unknownBlocksAndTagsAreSkipped() {
        Map<Integer, List<String>> lines = new LinkedHashMap<>();
        lines.put(10001, List.of("othermod:thing", "%othermod:things", "minecraft:short_grass", "18", ""));
        lines.put(10002, List.of("minecraft:water"));
        BlockIdMapping.Resolution<State> r = resolve(lines);
        assertEquals(10001, r.ids().get(CATALOG.state("minecraft:short_grass", Map.of())));
        assertEquals(10002, r.ids().get(CATALOG.state("minecraft:water", Map.of("level", "8"))));
        assertEquals(3, r.ids().size());
        assertEquals(2, r.entries());
        assertEquals(4, r.unknownEntries(), "two unknown names, a legacy numeric id and an empty entry");
    }
}
