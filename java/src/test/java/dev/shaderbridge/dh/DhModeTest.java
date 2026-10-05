package dev.shaderbridge.dh;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.DhPipeline;
import dev.shaderbridge.model.DhStrategy;
import dev.shaderbridge.model.ShadowSettings;
import dev.shaderbridge.render.RenderFixture;
import org.junit.jupiter.api.Test;

/** {@link DhMode}: how LODs are drawn per Distant Horizons strategy, against real compiled packs. */
class DhModeTest {
    private static final RenderFixture SYNTHESIZED = RenderFixture.load(RenderFixture.TUTORIAL4);
    private static final RenderFixture NATIVE = RenderFixture.load(RenderFixture.GLIMMER);

    @Test
    void strategySelectsTheMode() {
        assertEquals(DhMode.NATIVE, DhMode.select(NATIVE.dim().distantHorizons(), true));
        assertEquals(DhMode.SYNTHESIZED, DhMode.select(SYNTHESIZED.dim().distantHorizons(), true));
        assertEquals(DhMode.OFF, DhMode.select(new DhPipeline(DhStrategy.DISABLED, false, false), true));
        assertEquals(DhMode.NATIVE, DhMode.select(new DhPipeline(DhStrategy.SYNTHESIZED, false, false), true),
            "a synthesized pipeline without the unified projection draws like a native one");
    }

    @Test
    void withoutDistantHorizonsRenderingNoLodsAreDrawn() {
        assertEquals(DhMode.OFF, DhMode.select(NATIVE.dim().distantHorizons(), false));
        assertEquals(DhMode.OFF, DhMode.select(SYNTHESIZED.dim().distantHorizons(), false));
    }

    @Test
    void modesFollowTheHostConventions() {
        assertTrue(DhMode.NATIVE.separateDepth(), "native programs get dhDepthTex0/1 of their own");
        assertFalse(DhMode.NATIVE.unifiedProjection(), "native LODs also cover the vanilla area, with DH's own planes");
        assertFalse(DhMode.SYNTHESIZED.separateDepth(), "synthesized programs share Minecraft's depth");
        assertTrue(DhMode.SYNTHESIZED.unifiedProjection(), "one projection reaching the DH far plane, LODs beyond the vanilla area");
        assertFalse(DhMode.OFF.drawsLods());
        assertTrue(DhMode.NATIVE.drawsLods() && DhMode.SYNTHESIZED.drawsLods());
    }

    @Test
    void lodsCastShadowsWhenThePackDrawsDhShadow() {
        ShadowSettings shadow = SYNTHESIZED.dim().targets().shadow();
        assertTrue(DhMode.castsShadows(SYNTHESIZED.dim().distantHorizons(), shadow), "tutorial 4 synthesizes dh_shadow");
        assertFalse(DhMode.castsShadows(NATIVE.dim().distantHorizons(), NATIVE.dim().targets().shadow()), "glimmer has no dh_shadow");
        assertFalse(DhMode.castsShadows(new DhPipeline(DhStrategy.DISABLED, false, true), shadow));
    }
}
