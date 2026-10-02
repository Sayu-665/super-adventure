package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/**
 * Geometry ("world") programs: {@code gbuffers_*}, {@code shadow*} and {@code dh_*}. The wire name
 * is the serde snake_case form of the Rust variant ({@code DamagedBlock} is {@code damaged_block}),
 * which differs from the pack file name for a few programs.
 */
public enum GeometryProgram implements WireEnum {
    BASIC("gbuffers_basic"),
    LINE("gbuffers_line"),
    TEXTURED("gbuffers_textured"),
    TEXTURED_LIT("gbuffers_textured_lit"),
    SKY_BASIC("gbuffers_skybasic"),
    SKY_TEXTURED("gbuffers_skytextured"),
    CLOUDS("gbuffers_clouds"),
    TERRAIN("gbuffers_terrain"),
    TERRAIN_SOLID("gbuffers_terrain_solid"),
    TERRAIN_CUTOUT("gbuffers_terrain_cutout"),
    DAMAGED_BLOCK("gbuffers_damagedblock"),
    BLOCK("gbuffers_block"),
    BLOCK_TRANSLUCENT("gbuffers_block_translucent"),
    BEACON_BEAM("gbuffers_beaconbeam"),
    ITEM("gbuffers_item"),
    ENTITIES("gbuffers_entities"),
    ENTITIES_TRANSLUCENT("gbuffers_entities_translucent"),
    LIGHTNING("gbuffers_lightning"),
    PARTICLES("gbuffers_particles"),
    PARTICLES_TRANSLUCENT("gbuffers_particles_translucent"),
    ENTITIES_GLOWING("gbuffers_entities_glowing"),
    ARMOR_GLINT("gbuffers_armor_glint"),
    SPIDER_EYES("gbuffers_spidereyes"),
    HAND("gbuffers_hand"),
    WEATHER("gbuffers_weather"),
    WATER("gbuffers_water"),
    HAND_WATER("gbuffers_hand_water"),
    SHADOW("shadow"),
    SHADOW_SOLID("shadow_solid"),
    SHADOW_CUTOUT("shadow_cutout"),
    SHADOW_WATER("shadow_water"),
    SHADOW_ENTITIES("shadow_entities"),
    SHADOW_LIGHTNING("shadow_lightning"),
    SHADOW_BLOCK("shadow_block"),
    DH_TERRAIN("dh_terrain"),
    DH_WATER("dh_water"),
    DH_GENERIC("dh_generic"),
    DH_SHADOW("dh_shadow");

    private final String fileName;

    GeometryProgram(String fileName) {
        this.fileName = fileName;
    }

    /** @return the program's base file name in a pack, e.g. {@code gbuffers_terrain} */
    public String fileName() {
        return fileName;
    }
}
