package dev.shaderbridge.render.draw;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertSame;

import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.render.mapping.PipelineMapping;
import dev.shaderbridge.render.pipeline.PipelineShape;
import dev.shaderbridge.render.pipeline.ProgramResolution;
import dev.shaderbridge.render.pipeline.ProgramResolver;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import net.minecraft.client.renderer.RenderPipelines;
import org.junit.jupiter.api.Test;

/** {@link DrawSubstitution}: routing, fallback to vanilla, and which decisions are final. */
class DrawSubstitutionTest {
    private static final DimensionPipeline DIM = RenderFixture.load(RenderFixture.TUTORIAL4).dim();
    private static final RenderPipeline SOLID = RenderPipelines.SOLID_TERRAIN_MULTIDRAW;
    private static final PipelineMapping.Mapped TERRAIN = new PipelineMapping.Mapped(GeometryProgram.TERRAIN_SOLID,
        Optional.of(GeometryProgram.SHADOW_SOLID), "vanilla_terrain_basic");
    private static final ProgramResolution.Renderpearl READY = new ProgramResolution.Renderpearl(null, VanillaClonesTest.compiled());

    /** Resolves every slot to its own program with a scripted resolution, recording the calls. */
    private static final class Programs implements DrawSubstitution.GeometryPrograms {
        final List<String> calls = new ArrayList<>();
        ProgramResolution next;

        @Override
        public ProgramResolver.GeometryResolution geometry(GeometryProgram slot, String profile, PipelineShape shape, boolean shadowPass) {
            calls.add(slot.fileName() + " " + profile + (shadowPass ? " shadow" : "") + (shape.cull() ? " cull" : ""));
            return new ProgramResolver.GeometryResolution(slot, next);
        }
    }

    @Test
    void unmappedPipelinesDrawVanillaForGood() {
        List<RenderPipeline> routed = new ArrayList<>();
        Programs programs = new Programs();
        DrawSubstitution s = new DrawSubstitution(DIM, p -> {
            routed.add(p);
            return new PipelineMapping.Vanilla("gui");
        }, programs);
        assertInstanceOf(DrawSubstitution.Decision.Vanilla.class, s.decide(SOLID, false));
        assertInstanceOf(DrawSubstitution.Decision.Vanilla.class, s.decide(SOLID, false));
        assertEquals(1, routed.size());
        assertEquals(List.of(), programs.calls);
    }

    @Test
    void geometryWithoutAShadowProgramDrawsNothingOfItsOwnInTheShadowPass() {
        Programs programs = new Programs();
        programs.next = READY;
        DrawSubstitution s = new DrawSubstitution(DIM, p -> new PipelineMapping.Mapped(GeometryProgram.CLOUDS, Optional.empty(), "vanilla_clouds"), programs);
        assertInstanceOf(DrawSubstitution.Decision.Vanilla.class, s.decide(SOLID, true));
        assertEquals(List.of(), programs.calls);
        assertInstanceOf(DrawSubstitution.Decision.Pack.class, s.decide(SOLID, false));
    }

    @Test
    void compiledProgramsReplaceTheDrawAndThePassKindSelectsTheProgram() {
        Programs programs = new Programs();
        programs.next = READY;
        DrawSubstitution s = new DrawSubstitution(DIM, p -> TERRAIN, programs);
        DrawSubstitution.Decision.Pack gbuffers = assertInstanceOf(DrawSubstitution.Decision.Pack.class, s.decide(SOLID, false));
        assertEquals(GeometryProgram.TERRAIN_SOLID, gbuffers.routed());
        assertSame(DIM.programFor(GeometryProgram.TERRAIN_SOLID).orElseThrow(), gbuffers.program());
        assertSame(READY, gbuffers.resolution());
        DrawSubstitution.Decision.Pack shadow = assertInstanceOf(DrawSubstitution.Decision.Pack.class, s.decide(SOLID, true));
        assertEquals(GeometryProgram.TERRAIN_SOLID, shadow.routed(), "the render stage follows the gbuffers slot");
        s.decide(SOLID, false);
        s.decide(SOLID, true);
        assertEquals(List.of("gbuffers_terrain_solid vanilla_terrain_basic cull", "shadow_solid vanilla_terrain_basic shadow"), programs.calls,
            "shadow casters are drawn without back-face culling");
    }

    @Test
    void pendingProgramsDrawVanillaUntilTheyAreReady() {
        Programs programs = new Programs();
        programs.next = new ProgramResolution.Pending("variant");
        DrawSubstitution s = new DrawSubstitution(DIM, p -> TERRAIN, programs);
        assertInstanceOf(DrawSubstitution.Decision.Vanilla.class, s.decide(SOLID, false));
        assertInstanceOf(DrawSubstitution.Decision.Vanilla.class, s.decide(SOLID, false));
        programs.next = READY;
        assertInstanceOf(DrawSubstitution.Decision.Pack.class, s.decide(SOLID, false));
        assertEquals(3, programs.calls.size());
    }

    @Test
    void programsThatCannotRunDrawVanillaForGood() {
        Programs programs = new Programs();
        programs.next = new ProgramResolution.Unavailable(List.of("no independent blend"));
        DrawSubstitution s = new DrawSubstitution(DIM, p -> TERRAIN, programs);
        assertInstanceOf(DrawSubstitution.Decision.Vanilla.class, s.decide(SOLID, false));
        programs.next = READY;
        assertInstanceOf(DrawSubstitution.Decision.Vanilla.class, s.decide(SOLID, false));
        assertEquals(1, programs.calls.size());
    }

    @Test
    void drawsOfAKnownSlotBypassTheTable() {
        Programs programs = new Programs();
        programs.next = READY;
        List<RenderPipeline> routed = new ArrayList<>();
        DrawSubstitution s = new DrawSubstitution(DIM, p -> {
            routed.add(p);
            return new PipelineMapping.Vanilla("not in the vanilla table");
        }, programs);
        DrawSubstitution.Decision.Pack generic = assertInstanceOf(DrawSubstitution.Decision.Pack.class,
            s.decideFor(SOLID, GeometryProgram.DH_GENERIC, "dh_generic", false));
        assertEquals(GeometryProgram.DH_GENERIC, generic.routed());
        assertSame(DIM.programFor(GeometryProgram.DH_GENERIC).orElseThrow(), generic.program());
        s.decideFor(SOLID, GeometryProgram.DH_GENERIC, "dh_generic", false);
        assertEquals(List.of("dh_generic dh_generic cull"), programs.calls, "decided once");
        assertEquals(List.of(), routed, "the vanilla table is not consulted");
        assertInstanceOf(DrawSubstitution.Decision.Vanilla.class, s.decide(SOLID, false), "the table's decision is kept apart");
    }

    @Test
    void particlesAndWeatherCastNoFeatureShadows() {
        Programs programs = new Programs();
        PipelineMapping.Mapped particles = new PipelineMapping.Mapped(GeometryProgram.PARTICLES, Optional.of(GeometryProgram.SHADOW), "vanilla_particle");
        PipelineMapping.Mapped entities = new PipelineMapping.Mapped(GeometryProgram.ENTITIES, Optional.of(GeometryProgram.SHADOW_ENTITIES),
            "vanilla_entity");
        assertEquals(false, new DrawSubstitution(DIM, p -> particles, programs).castsFeatureShadow(SOLID));
        assertEquals(true, new DrawSubstitution(DIM, p -> entities, programs).castsFeatureShadow(SOLID));
        assertEquals(true, new DrawSubstitution(DIM, p -> new PipelineMapping.Vanilla("gui"), programs).castsFeatureShadow(SOLID),
            "unmapped pipelines are decided by the shadow pass itself (they draw nothing there)");
    }
}
