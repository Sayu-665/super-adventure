package dev.shaderbridge.uniforms;

import static org.junit.jupiter.api.Assertions.assertEquals;

import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

class BiomeIdsTest {
    @Test
    void idsFollowTheSortedBiomeIds() {
        Map<String, Integer> ids = BiomeIds.assign(List.of("minecraft:plains", "minecraft:badlands", "minecraft:the_void", "minecraft:desert"));
        assertEquals(List.of("BIOME_BADLANDS", "BIOME_DESERT", "BIOME_PLAINS", "BIOME_THE_VOID"), List.copyOf(ids.keySet()));
        assertEquals(2, ids.get("BIOME_PLAINS"));
    }

    @Test
    void theFirstNamespaceWinsAPath() {
        Map<String, Integer> ids = BiomeIds.assign(List.of("zmod:plains", "minecraft:plains", "amod:crystal_caves"));
        assertEquals(Map.of("BIOME_CRYSTAL_CAVES", 0, "BIOME_PLAINS", 1), ids);
        assertEquals("BIOME_DEEP_DARK", BiomeIds.macroName("minecraft:deep_dark"));
    }

    @Test
    void macrosRoundTrip() {
        Map<String, String> macros = Map.of("BIOME_PLAINS", "40", "BIOME_BROKEN", "x", "OTHER", "1");
        assertEquals(Map.of("BIOME_PLAINS", 40), BiomeIds.fromMacros(macros));
    }
}
