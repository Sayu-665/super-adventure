package dev.shaderbridge.render.pipeline;

import static dev.shaderbridge.model.GeometryProgram.BASIC;
import static dev.shaderbridge.model.GeometryProgram.BLOCK;
import static dev.shaderbridge.model.GeometryProgram.DH_TERRAIN;
import static dev.shaderbridge.model.GeometryProgram.ENTITIES;
import static dev.shaderbridge.model.GeometryProgram.HAND;
import static dev.shaderbridge.model.GeometryProgram.PARTICLES;
import static dev.shaderbridge.model.GeometryProgram.SHADOW;
import static dev.shaderbridge.model.GeometryProgram.SHADOW_ENTITIES;
import static dev.shaderbridge.model.GeometryProgram.TERRAIN;
import static dev.shaderbridge.model.GeometryProgram.TEXTURED;
import static dev.shaderbridge.model.GeometryProgram.TEXTURED_LIT;

import dev.shaderbridge.model.GeometryProgram;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;

/**
 * The Iris fallback chain of geometry programs ({@code sb_core::GeometryProgram::fallback}): when
 * a pack lacks a program, or ShaderBridge cannot run it, the next program of the chain draws the
 * geometry ({@code gbuffers_water} falls back to {@code gbuffers_terrain}, then
 * {@code gbuffers_textured_lit}, ...). The compiled model already resolves missing programs; the
 * host walks the chain further when a present program cannot run.
 */
public final class GeometryChain {
    private GeometryChain() {
    }

    /**
     * @param program a geometry program
     * @return the next program of its chain, if any
     */
    public static Optional<GeometryProgram> fallback(GeometryProgram program) {
        return Optional.ofNullable(switch (program) {
            case BASIC, SHADOW, DH_TERRAIN, DH_SHADOW -> null;
            case LINE, TEXTURED, SKY_BASIC -> BASIC;
            case TEXTURED_LIT, SKY_TEXTURED, CLOUDS, BEACON_BEAM, ARMOR_GLINT, SPIDER_EYES -> TEXTURED;
            case TERRAIN, ITEM, ENTITIES, PARTICLES, HAND, WEATHER -> TEXTURED_LIT;
            case TERRAIN_SOLID, TERRAIN_CUTOUT, DAMAGED_BLOCK, BLOCK, WATER -> TERRAIN;
            case BLOCK_TRANSLUCENT -> BLOCK;
            case ENTITIES_TRANSLUCENT, LIGHTNING, ENTITIES_GLOWING -> ENTITIES;
            case PARTICLES_TRANSLUCENT -> PARTICLES;
            case HAND_WATER -> HAND;
            case SHADOW_SOLID, SHADOW_CUTOUT, SHADOW_WATER, SHADOW_ENTITIES, SHADOW_BLOCK -> SHADOW;
            case SHADOW_LIGHTNING -> SHADOW_ENTITIES;
            case DH_WATER, DH_GENERIC -> DH_TERRAIN;
        });
    }

    /**
     * @param program a geometry program
     * @return the program followed by every fallback, in order
     */
    public static List<GeometryProgram> chain(GeometryProgram program) {
        List<GeometryProgram> out = new ArrayList<>();
        for (Optional<GeometryProgram> p = Optional.of(program); p.isPresent(); p = fallback(p.get())) {
            out.add(p.get());
        }
        return List.copyOf(out);
    }
}
