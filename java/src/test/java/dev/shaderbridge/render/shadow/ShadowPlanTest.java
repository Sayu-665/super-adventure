package dev.shaderbridge.render.shadow;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.ShadowSettings;
import dev.shaderbridge.render.RenderFixture;
import java.util.List;
import org.junit.jupiter.api.Test;

/** {@link ShadowPlan}: the shadow pass steps per the pack's shadow directives. */
class ShadowPlanTest {
    private static final ShadowSettings BASE = RenderFixture.load(RenderFixture.TUTORIAL4).dim().targets().shadow();

    private static ShadowSettings with(boolean enabled, boolean terrain, boolean translucent, boolean entities) {
        ShadowSettings s = BASE;
        return new ShadowSettings(enabled, s.resolution(), s.fov(), s.distance(), s.nearPlane(), s.farPlane(), s.distanceRenderMul(), s.entityDistanceMul(),
            s.intervalSize(), s.voxelDistance(), s.hardwareFiltering(), s.mipmap(), s.nearest(), s.colorMipmap(), s.colorNearest(), s.culling(), terrain,
            translucent, entities, false, false, false, s.dhShadowEnabled());
    }

    @Test
    void opaqueCastersThenTheDepthCopyThenTranslucentCasters() {
        assertEquals(List.of(ShadowPlan.Step.OPAQUE_TERRAIN, ShadowPlan.Step.COPY_DEPTH, ShadowPlan.Step.TRANSLUCENT_TERRAIN),
            ShadowPlan.of(with(true, true, true, false), false).steps());
        assertEquals(List.of(ShadowPlan.Step.OPAQUE_TERRAIN, ShadowPlan.Step.COPY_DEPTH), ShadowPlan.of(with(true, true, false, false), false).steps());
        assertEquals(List.of(ShadowPlan.Step.COPY_DEPTH, ShadowPlan.Step.TRANSLUCENT_TERRAIN), ShadowPlan.of(with(true, false, true, false), false).steps(),
            "shadowTranslucent does not depend on shadowTerrain, as in the headless executor");
    }

    @Test
    void distantHorizonsLodsAreOpaqueCasters() {
        assertEquals(List.of(ShadowPlan.Step.OPAQUE_TERRAIN, ShadowPlan.Step.DISTANT_TERRAIN, ShadowPlan.Step.COPY_DEPTH,
            ShadowPlan.Step.TRANSLUCENT_TERRAIN), ShadowPlan.of(with(true, true, true, false), true).steps());
        assertEquals(List.of(ShadowPlan.Step.DISTANT_TERRAIN, ShadowPlan.Step.COPY_DEPTH), ShadowPlan.of(with(true, false, false, false), true).steps(),
            "LODs cast shadows without shadowTerrain, as in the headless executor");
        assertTrue(ShadowPlan.of(with(false, true, true, false), true).steps().isEmpty());
    }

    @Test
    void withoutAShadowPassNothingIsDrawn() {
        assertTrue(ShadowPlan.of(with(false, true, true, true), false).steps().isEmpty());
        assertTrue(ShadowPlan.of(with(false, true, true, true), false).notes().isEmpty());
    }

    @Test
    void entityCastersAreReportedAsMissing() {
        assertTrue(ShadowPlan.of(with(true, true, true, false), false).notes().isEmpty());
        assertEquals(1, ShadowPlan.of(with(true, true, true, true), false).notes().size());
    }
}
