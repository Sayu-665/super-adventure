package dev.shaderbridge.uniforms;

import net.minecraft.core.Holder;
import net.minecraft.tags.BiomeTags;
import net.minecraft.world.level.biome.Biome;

/**
 * OptiFine biome categories ({@code CAT_*} constants, by ordinal). Modern Minecraft has no
 * categories, so they are derived from biome tags in Iris' order of precedence.
 */
public enum BiomeCategory {
    NONE, TAIGA, EXTREME_HILLS, JUNGLE, MESA, PLAINS, SAVANNA, ICY, THE_END, BEACH, FOREST, OCEAN, DESERT, RIVER, SWAMP, MUSHROOM, NETHER, MOUNTAIN, UNDERGROUND;

    /**
     * @param biome a biome holder
     * @return its category
     */
    public static BiomeCategory of(Holder<Biome> biome) {
        if (biome.is(BiomeTags.WITHOUT_WANDERING_TRADER_SPAWNS)) {
            return NONE;
        } else if (biome.is(BiomeTags.HAS_VILLAGE_SNOWY)) {
            return ICY;
        } else if (biome.is(BiomeTags.IS_HILL)) {
            return EXTREME_HILLS;
        } else if (biome.is(BiomeTags.IS_TAIGA)) {
            return TAIGA;
        } else if (biome.is(BiomeTags.IS_OCEAN)) {
            return OCEAN;
        } else if (biome.is(BiomeTags.IS_JUNGLE)) {
            return JUNGLE;
        } else if (biome.is(BiomeTags.IS_FOREST)) {
            return FOREST;
        } else if (biome.is(BiomeTags.IS_BADLANDS)) {
            return MESA;
        } else if (biome.is(BiomeTags.IS_NETHER)) {
            return NETHER;
        } else if (biome.is(BiomeTags.IS_END)) {
            return THE_END;
        } else if (biome.is(BiomeTags.IS_BEACH)) {
            return BEACH;
        } else if (biome.is(BiomeTags.HAS_DESERT_PYRAMID)) {
            return DESERT;
        } else if (biome.is(BiomeTags.IS_RIVER)) {
            return RIVER;
        } else if (biome.is(BiomeTags.ALLOWS_SURFACE_SLIME_SPAWNS)) {
            return SWAMP;
        } else if (biome.is(BiomeTags.WITHOUT_ZOMBIE_SIEGES)) {
            return MUSHROOM;
        } else if (biome.is(BiomeTags.IS_MOUNTAIN)) {
            return MOUNTAIN;
        }
        return PLAINS;
    }
}
