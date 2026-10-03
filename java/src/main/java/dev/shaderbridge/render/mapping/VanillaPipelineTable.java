package dev.shaderbridge.render.mapping;

import static dev.shaderbridge.model.GeometryProgram.ARMOR_GLINT;
import static dev.shaderbridge.model.GeometryProgram.BEACON_BEAM;
import static dev.shaderbridge.model.GeometryProgram.BLOCK;
import static dev.shaderbridge.model.GeometryProgram.BLOCK_TRANSLUCENT;
import static dev.shaderbridge.model.GeometryProgram.CLOUDS;
import static dev.shaderbridge.model.GeometryProgram.DAMAGED_BLOCK;
import static dev.shaderbridge.model.GeometryProgram.ENTITIES;
import static dev.shaderbridge.model.GeometryProgram.ENTITIES_TRANSLUCENT;
import static dev.shaderbridge.model.GeometryProgram.LIGHTNING;
import static dev.shaderbridge.model.GeometryProgram.LINE;
import static dev.shaderbridge.model.GeometryProgram.PARTICLES;
import static dev.shaderbridge.model.GeometryProgram.PARTICLES_TRANSLUCENT;
import static dev.shaderbridge.model.GeometryProgram.SHADOW;
import static dev.shaderbridge.model.GeometryProgram.SHADOW_BLOCK;
import static dev.shaderbridge.model.GeometryProgram.SHADOW_CUTOUT;
import static dev.shaderbridge.model.GeometryProgram.SHADOW_ENTITIES;
import static dev.shaderbridge.model.GeometryProgram.SHADOW_LIGHTNING;
import static dev.shaderbridge.model.GeometryProgram.SHADOW_SOLID;
import static dev.shaderbridge.model.GeometryProgram.SHADOW_WATER;
import static dev.shaderbridge.model.GeometryProgram.SKY_BASIC;
import static dev.shaderbridge.model.GeometryProgram.SKY_TEXTURED;
import static dev.shaderbridge.model.GeometryProgram.SPIDER_EYES;
import static dev.shaderbridge.model.GeometryProgram.TERRAIN_CUTOUT;
import static dev.shaderbridge.model.GeometryProgram.TERRAIN_SOLID;
import static dev.shaderbridge.model.GeometryProgram.TEXTURED;
import static dev.shaderbridge.model.GeometryProgram.WATER;
import static dev.shaderbridge.model.GeometryProgram.WEATHER;

import dev.shaderbridge.model.GeometryProgram;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import net.minecraft.resources.Identifier;

/**
 * Which pack program draws each vanilla Minecraft 26.3 render pipeline, keyed by the pipeline's
 * location ({@code minecraft:pipeline/<name>}). Every location of {@code RenderPipelines} has an
 * explicit entry (enforced by a unit test against the game jar), so a new vanilla pipeline is
 * noticed instead of silently drawing vanilla.
 *
 * <p>The profiles follow each pipeline's vertex format and host bindings: terrain on the
 * MultiDrawIndirect path is {@value #TERRAIN_MULTIDRAW}, on the per-section path
 * {@value #TERRAIN_SECTION}; {@code BLOCK} geometry drawn with {@code DynamicTransforms} (moving
 * blocks, beacon beams, block breaking) is {@value #BLOCK_FORMAT}; and so on. A pipeline that
 * shares a location but uses another vertex format (the {@code *_GLINT_SPECIAL} item pipelines)
 * is still compatible because only the profile's attributes are read; {@link PipelineRouter}
 * checks this per pipeline.
 *
 * <p>The {@code *_glint} pipelines of 26.3 draw the model <em>and</em> its glint in one pass
 * ({@code GLINT} shader define), so they map to the program of the model; only
 * {@code pipeline/glint}, which draws the glint alone over an existing surface, maps to
 * {@code gbuffers_armor_glint}.
 */
public final class VanillaPipelineTable {
    /** Namespace of vanilla pipeline locations. */
    public static final String NAMESPACE = "minecraft";
    /** Terrain on the MultiDrawIndirect path ({@code BLOCK} + {@code CHUNK_DATA_INSTANCED}). */
    public static final String TERRAIN_MULTIDRAW = "vanilla_terrain_basic";
    /** Terrain on the per-section path ({@code BLOCK} + the {@code ChunkSection} block). */
    public static final String TERRAIN_SECTION = "vanilla_terrain_section";
    /** {@code BLOCK} vertices with {@code DynamicTransforms}. */
    public static final String BLOCK_FORMAT = "vanilla_block";
    /** {@code ENTITY} vertices with {@code DynamicTransforms}. */
    public static final String ENTITY_FORMAT = "vanilla_entity";
    /** {@code PARTICLE} vertices. */
    public static final String PARTICLE_FORMAT = "vanilla_particle";
    /** {@code POSITION} vertices. */
    public static final String POSITION = "vanilla_position";
    /** {@code POSITION_COLOR} vertices. */
    public static final String POSITION_COLOR = "vanilla_position_color";
    /** {@code POSITION_TEX} vertices. */
    public static final String POSITION_TEX = "vanilla_position_tex";
    /** {@code POSITION_TEX_COLOR} vertices. */
    public static final String POSITION_TEX_COLOR = "vanilla_position_tex_color";
    /** {@code POSITION_COLOR_LIGHTMAP} vertices. */
    public static final String POSITION_COLOR_LIGHTMAP = "vanilla_position_color_lightmap";
    /** {@code POSITION_TEX_LIGHTMAP_COLOR} in-world text. */
    public static final String TEXT = "vanilla_text";
    /** {@code POSITION_COLOR_NORMAL_LINE_WIDTH} lines. */
    public static final String LINES = "vanilla_lines";
    /** Clouds generated from the {@code CloudFaces} texel buffer (no vertex buffer). */
    public static final String CLOUD_FACES = "vanilla_clouds";

    /** Location path prefix of the order-independent-transparency pipelines. */
    static final String OIT_PREFIX = "pipeline/oit_";

    private static final Map<String, PipelineMapping> TABLE = build();

    private VanillaPipelineTable() {
    }

    /**
     * @param location a render pipeline location
     * @return the mapping of the pipeline; unknown locations (other mods, newer versions) draw vanilla
     */
    public static PipelineMapping lookup(Identifier location) {
        if (!NAMESPACE.equals(location.getNamespace())) {
            return new PipelineMapping.Vanilla("not a vanilla pipeline");
        }
        return lookupPath(location.getPath());
    }

    /**
     * @param path a vanilla pipeline location path ({@code pipeline/solid_terrain})
     * @return the mapping; unknown paths draw vanilla
     */
    public static PipelineMapping lookupPath(String path) {
        PipelineMapping mapping = TABLE.get(path);
        if (mapping != null) {
            return mapping;
        }
        if (path.startsWith(OIT_PREFIX)) {
            return new PipelineMapping.Vanilla("order-independent transparency is disabled while a pack is active");
        }
        return new PipelineMapping.Vanilla("not in ShaderBridge's Minecraft 26.3 pipeline table");
    }

    /**
     * @param path a vanilla pipeline location path
     * @return whether the table decides the path explicitly (an entry or the OIT family)
     */
    public static boolean covers(String path) {
        return TABLE.containsKey(path) || path.startsWith(OIT_PREFIX);
    }

    /** @return the explicit entries by location path, in table order */
    public static Map<String, PipelineMapping> entries() {
        return TABLE;
    }

    private static Map<String, PipelineMapping> build() {
        Table t = new Table();
        // Chunk terrain.
        t.map(TERRAIN_SOLID, SHADOW_SOLID, TERRAIN_SECTION, "solid_terrain");
        t.map(TERRAIN_SOLID, SHADOW_SOLID, TERRAIN_MULTIDRAW, "solid_terrain_multidraw");
        t.map(TERRAIN_CUTOUT, SHADOW_CUTOUT, TERRAIN_SECTION, "cutout_terrain");
        t.map(TERRAIN_CUTOUT, SHADOW_CUTOUT, TERRAIN_MULTIDRAW, "cutout_terrain_multidraw");
        t.map(WATER, SHADOW_WATER, TERRAIN_SECTION, "translucent_terrain");
        t.map(WATER, SHADOW_WATER, TERRAIN_MULTIDRAW, "translucent_terrain_multidraw");
        t.vanilla("the debug wireframe view", "wireframe", "wireframe_multidraw");
        // Blocks outside chunk meshes (moving pistons, falling blocks), block decals and beams.
        t.map(BLOCK, SHADOW_CUTOUT, BLOCK_FORMAT, "solid_block", "cutout_block");
        t.map(BLOCK_TRANSLUCENT, SHADOW_WATER, BLOCK_FORMAT, "translucent_block");
        t.map(DAMAGED_BLOCK, null, BLOCK_FORMAT, "crumbling");
        t.map(BEACON_BEAM, SHADOW_ENTITIES, BLOCK_FORMAT, "beacon_beam_opaque", "beacon_beam_translucent");
        t.map(BLOCK, SHADOW_BLOCK, POSITION, "end_portal", "end_gateway");
        // Entities, block entities, items, armor.
        t.map(ENTITIES, SHADOW_ENTITIES, ENTITY_FORMAT, "entity_solid", "entity_solid_offset_forward", "entity_cutout", "entity_cutout_cull",
            "entity_cutout_z_offset", "entity_cutout_dissolve", "armor_cutout_no_cull", "armor_decal_cutout_no_cull", "armor_translucent",
            "energy_swirl", "breeze_wind", "end_crystal_beam", "banner_pattern", "item_cutout", "armor_cutout_no_cull_glint",
            "entity_solid_glint");
        t.map(ENTITIES_TRANSLUCENT, SHADOW_ENTITIES, ENTITY_FORMAT, "entity_translucent", "entity_translucent_cull", "entity_translucent_emissive",
            "item_translucent", "item_translucent_glint");
        t.map(SPIDER_EYES, SHADOW_ENTITIES, ENTITY_FORMAT, "eyes");
        t.map(ARMOR_GLINT, null, POSITION_TEX, "glint");
        t.map(LIGHTNING, SHADOW_LIGHTNING, POSITION_COLOR, "lightning", "dragon_rays");
        // Particles and weather.
        t.map(PARTICLES, SHADOW, PARTICLE_FORMAT, "opaque_particle");
        t.map(PARTICLES_TRANSLUCENT, SHADOW, PARTICLE_FORMAT, "translucent_particle");
        t.map(WEATHER, null, PARTICLE_FORMAT, "weather");
        // Sky and clouds.
        t.map(SKY_BASIC, null, POSITION, "sky", "stars");
        t.map(SKY_BASIC, null, POSITION_COLOR, "sunrise_sunset");
        t.map(SKY_TEXTURED, null, POSITION_TEX, "celestial");
        t.map(SKY_TEXTURED, null, POSITION_TEX_COLOR, "end_sky");
        t.map(CLOUDS, null, CLOUD_FACES, "clouds", "flat_clouds");
        // Lines, leads, text and the world border.
        t.map(LINE, null, LINES, "lines", "lines_translucent", "lines_translucent_no_depth_write", "lines_depth_bias", "secondary_block_outline");
        t.map(LINE, SHADOW, POSITION_COLOR_LIGHTMAP, "leash");
        t.map(TEXTURED, SHADOW_ENTITIES, TEXT, "text", "text_grayscale", "text_polygon_offset", "text_grayscale_polygon_offset");
        t.map(TEXTURED, null, POSITION_TEX_COLOR, "text_see_through", "text_grayscale_see_through");
        t.map(TEXTURED, null, POSITION_TEX, "world_border");
        // Everything else draws vanilla.
        t.vanilla("a depth-only mask that keeps water out of boats", "water_mask", "oit_water_mask");
        t.vanilla("the vanilla entity shadow decal", "entity_shadow");
        t.vanilla("GUI and screen overlays", "gui", "gui_invert", "gui_text", "gui_text_grayscale", "gui_text_highlight", "gui_textured",
            "gui_textured_premultiplied_alpha", "gui_opaque_textured_background", "gui_nausea_overlay", "block_screen_effect", "fire_screen_effect",
            "vignette", "crosshair", "mojang_logo", "panorama");
        t.vanilla("debug renderers", "debug_points", "debug_filled_box", "debug_quads", "debug_triangle_fan");
        t.vanilla("the glowing-entity outline effect", "outline_cull", "outline_no_cull", "entity_outline_blit");
        t.vanilla("texture generation (lightmap, animated sprites)", "lightmap", "animate_sprite_blit", "animate_sprite_interpolate");
        t.vanilla("internal blits", "blit_depth", "blit_depth_bounds", "blit_depth_during_depth_bounds", "integrate_depth", "tracy_blit");
        t.vanilla("order-independent transparency is disabled while a pack is active", "oit_composite", "oit_depth_bounds_cull");
        return Collections.unmodifiableMap(t.entries);
    }

    /** Collects entries and rejects duplicates. */
    private static final class Table {
        final Map<String, PipelineMapping> entries = new LinkedHashMap<>();

        void map(GeometryProgram gbuffers, GeometryProgram shadow, String profile, String... names) {
            put(new PipelineMapping.Mapped(gbuffers, Optional.ofNullable(shadow), profile), names);
        }

        void vanilla(String reason, String... names) {
            put(new PipelineMapping.Vanilla(reason), names);
        }

        private void put(PipelineMapping mapping, String... names) {
            for (String name : List.of(names)) {
                if (entries.put("pipeline/" + name, mapping) != null) {
                    throw new IllegalStateException("duplicate pipeline table entry " + name);
                }
            }
        }
    }
}
