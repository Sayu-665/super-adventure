package dev.shaderbridge.uniforms;

import static org.junit.jupiter.api.Assertions.assertEquals;

import dev.shaderbridge.model.IdMaps;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import org.junit.jupiter.api.Test;

class IdMapLookupTest {
    private static final Map<String, String> WHEAT_AGE_7 = Map.of("age", "7");

    private static IdMapLookup lookup() {
        Map<Integer, List<String>> blocks = new LinkedHashMap<>();
        blocks.put(10010, List.of("minecraft:wheat:age=7", "carrots:age=6,7"));
        blocks.put(10001, List.of("oak_leaves", "%minecraft:leaves"));
        blocks.put(10020, List.of("minecraft:wheat"));
        blocks.put(10030, List.of("modded:crystal:lit=true:facing=north,south"));
        Map<Integer, List<String>> items = new LinkedHashMap<>();
        items.put(20001, List.of("torch", "minecraft:lantern"));
        items.put(20002, List.of("minecraft:torch"));
        Map<Integer, List<String>> entities = Map.of(30001, List.of("minecraft:lightning_bolt"));
        return IdMapLookup.of(new IdMaps(blocks, items, entities, Map.of(), Map.of()));
    }

    @Test
    void parsesTheEntryGrammar() {
        IdMapLookup.BlockEntry plain = IdMapLookup.parseBlock(1, 0, "stone");
        assertEquals("minecraft:stone", plain.name());
        assertEquals(Map.of(), plain.states());
        IdMapLookup.BlockEntry implicitNamespace = IdMapLookup.parseBlock(1, 0, "wheat:age=7");
        assertEquals("minecraft:wheat", implicitNamespace.name());
        assertEquals(Map.of("age", Set.of("7")), implicitNamespace.states());
        IdMapLookup.BlockEntry full = IdMapLookup.parseBlock(1, 0, "mod:block:a=1,2:b=x");
        assertEquals("mod:block", full.name());
        assertEquals(Map.of("a", Set.of("1", "2"), "b", Set.of("x")), full.states());
        assertEquals(true, IdMapLookup.parseBlock(1, 0, "%minecraft:logs").tag());
    }

    @Test
    void blockStatesMatchTheFirstEntry() {
        IdMapLookup ids = lookup();
        assertEquals(10010, ids.block("minecraft:wheat", WHEAT_AGE_7::get, tag -> false));
        assertEquals(10020, ids.block("minecraft:wheat", Map.of("age", "3")::get, tag -> false));
        assertEquals(10010, ids.block("minecraft:carrots", Map.of("age", "6")::get, tag -> false));
        assertEquals(-1, ids.block("minecraft:carrots", Map.of("age", "5")::get, tag -> false));
        assertEquals(10030, ids.block("modded:crystal", Map.of("lit", "true", "facing", "south")::get, tag -> false));
        assertEquals(-1, ids.block("modded:crystal", Map.of("lit", "true", "facing", "east")::get, tag -> false));
        assertEquals(-1, ids.block("modded:crystal", Map.of("facing", "north")::get, tag -> false), "a missing property never matches");
    }

    @Test
    void tagsMatchInFileOrder() {
        IdMapLookup ids = lookup();
        assertEquals(10001, ids.block("minecraft:birch_leaves", Map.<String, String>of()::get, "minecraft:leaves"::equals));
        assertEquals(10001, ids.block("minecraft:oak_leaves", Map.<String, String>of()::get, "minecraft:leaves"::equals));
        assertEquals(10010, ids.block("minecraft:wheat", WHEAT_AGE_7::get, tag -> true), "an earlier direct entry beats a later tag");
    }

    @Test
    void itemsAndEntities() {
        IdMapLookup ids = lookup();
        assertEquals(20001, ids.item("minecraft:torch"), "the first id wins");
        assertEquals(20001, ids.item("minecraft:lantern"));
        assertEquals(-1, ids.item("minecraft:stick"));
        assertEquals(30001, ids.entity("minecraft:lightning_bolt"));
        assertEquals(-1, IdMapLookup.EMPTY.entity("minecraft:zombie"));
    }
}
