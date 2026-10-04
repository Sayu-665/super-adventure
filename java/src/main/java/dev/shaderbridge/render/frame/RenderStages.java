package dev.shaderbridge.render.frame;

import dev.shaderbridge.model.GeometryProgram;

/**
 * The {@code renderStage} uniform ({@code MC_RENDER_STAGE_*}, Iris' {@code WorldRenderingPhase}
 * ordinals) of a draw, derived from the geometry slot the vanilla pipeline maps to. Iris tracks the
 * phase vanilla is in; ShaderBridge only sees pipelines, so draws that share a slot share a stage
 * (the sun and the moon both report {@code SUN}, all entity-like draws {@code ENTITIES}).
 */
public final class RenderStages {
    /** {@code MC_RENDER_STAGE_NONE}. */
    public static final int NONE = 0;
    /** {@code MC_RENDER_STAGE_SKY}. */
    public static final int SKY = 1;
    /** {@code MC_RENDER_STAGE_SUN}. */
    public static final int SUN = 4;
    /** {@code MC_RENDER_STAGE_TERRAIN_SOLID}. */
    public static final int TERRAIN_SOLID = 8;
    /** {@code MC_RENDER_STAGE_TERRAIN_CUTOUT}. */
    public static final int TERRAIN_CUTOUT = 10;
    /** {@code MC_RENDER_STAGE_ENTITIES}. */
    public static final int ENTITIES = 11;
    /** {@code MC_RENDER_STAGE_BLOCK_ENTITIES}. */
    public static final int BLOCK_ENTITIES = 12;
    /** {@code MC_RENDER_STAGE_DESTROY}. */
    public static final int DESTROY = 13;
    /** {@code MC_RENDER_STAGE_OUTLINE}. */
    public static final int OUTLINE = 14;
    /** {@code MC_RENDER_STAGE_TERRAIN_TRANSLUCENT}. */
    public static final int TERRAIN_TRANSLUCENT = 17;
    /** {@code MC_RENDER_STAGE_PARTICLES}. */
    public static final int PARTICLES = 19;
    /** {@code MC_RENDER_STAGE_CLOUDS}. */
    public static final int CLOUDS = 20;
    /** {@code MC_RENDER_STAGE_RAIN_SNOW}. */
    public static final int RAIN_SNOW = 21;

    private RenderStages() {
    }

    /**
     * @param slot the geometry slot a draw is routed to (before fallback; shadow slots count as
     *             their gbuffers counterpart)
     * @return the draw's render stage
     */
    public static int of(GeometryProgram slot) {
        return switch (slot) {
            case SKY_BASIC -> SKY;
            case SKY_TEXTURED -> SUN;
            case TERRAIN, TERRAIN_SOLID, SHADOW, SHADOW_SOLID, DH_TERRAIN, DH_GENERIC, DH_SHADOW -> TERRAIN_SOLID;
            case TERRAIN_CUTOUT, SHADOW_CUTOUT -> TERRAIN_CUTOUT;
            case WATER, SHADOW_WATER, DH_WATER, HAND_WATER -> TERRAIN_TRANSLUCENT;
            case ENTITIES, ENTITIES_TRANSLUCENT, ENTITIES_GLOWING, SPIDER_EYES, ARMOR_GLINT, ITEM, LIGHTNING, SHADOW_ENTITIES, SHADOW_LIGHTNING,
                 HAND -> ENTITIES;
            case BLOCK, BLOCK_TRANSLUCENT, BEACON_BEAM, SHADOW_BLOCK -> BLOCK_ENTITIES;
            case DAMAGED_BLOCK -> DESTROY;
            case LINE -> OUTLINE;
            case PARTICLES, PARTICLES_TRANSLUCENT -> PARTICLES;
            case CLOUDS -> CLOUDS;
            case WEATHER -> RAIN_SNOW;
            case BASIC, TEXTURED, TEXTURED_LIT -> NONE;
        };
    }
}
