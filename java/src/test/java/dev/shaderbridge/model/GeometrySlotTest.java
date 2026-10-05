package dev.shaderbridge.model;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/**
 * The blend and alpha test a slot gives its program, as {@code sb_core::model::GeometrySlot}
 * computes them: one {@code gbuffers_terrain} draws solid terrain without an alpha test, cutout
 * terrain at 0.5 and water at 0.1 with blending, as in Iris.
 */
class GeometrySlotTest {
    private static final BlendMode TRANSLUCENT = new BlendMode(BlendFactor.SRC_ALPHA, BlendFactor.ONE_MINUS_SRC_ALPHA, BlendFactor.ONE,
        BlendFactor.ONE_MINUS_SRC_ALPHA);
    private static final AlphaTest GREATER_01 = new AlphaTest(AlphaFunc.GREATER, 0.1f);

    private static Program terrain(BlendMode blend, AlphaTest alphaTest, boolean inheritBlend) {
        return new Program("world0/gbuffers_terrain", new ProgramKind.Geometry(GeometryProgram.TERRAIN), "vanilla_terrain", false, List.of(),
            List.of(0), List.of(0), List.of("float"), blend, Map.of(), alphaTest, new ViewportScale(1, 0, 0), List.of(), List.of(), List.of(), 0,
            null, null, null, inheritBlend);
    }

    private static GeometrySlot slot(BlendMode blend, AlphaTest alphaTest) {
        return new GeometrySlot(0, GeometryProgram.TERRAIN, Map.of(), blend, alphaTest);
    }

    @Test
    void slotsGiveTheirOwnBlendAndAlphaReference() {
        Program program = terrain(null, GREATER_01, true);
        Program solid = slot(null, new AlphaTest(AlphaFunc.ALWAYS, 0)).drawn(program);
        assertNull(solid.blend());
        assertEquals(new AlphaTest(AlphaFunc.GREATER, -Float.MAX_VALUE), solid.alphaTest(), "a reference every alpha passes");
        assertTrue(solid.inheritBlend(), "a host replacing a vanilla draw still uses that draw's blend");
        assertEquals(new AlphaTest(AlphaFunc.GREATER, 0.5f), slot(null, new AlphaTest(AlphaFunc.GREATER, 0.5f)).drawn(program).alphaTest());
        Program water = slot(TRANSLUCENT, GREATER_01).drawn(program);
        assertEquals(TRANSLUCENT, water.blend());
        assertEquals(GREATER_01, water.alphaTest());
    }

    @Test
    void directivesAndOlderModelsKeepTheProgramsState() {
        BlendMode add = new BlendMode(BlendFactor.ONE, BlendFactor.ONE, BlendFactor.ONE, BlendFactor.ONE);
        Program directed = terrain(add, GREATER_01, false);
        assertEquals(add, slot(null, new AlphaTest(AlphaFunc.ALWAYS, 0)).drawn(directed).blend());
        Program older = new GeometrySlot(0, GeometryProgram.TERRAIN).drawn(terrain(TRANSLUCENT, GREATER_01, false));
        assertEquals(TRANSLUCENT, older.blend());
        assertEquals(GREATER_01, older.alphaTest());
        // No compiled test: nothing to feed.
        assertNull(slot(null, null).drawn(terrain(null, null, true)).alphaTest());
    }
}
