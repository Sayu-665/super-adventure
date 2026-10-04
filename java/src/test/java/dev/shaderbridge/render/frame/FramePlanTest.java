package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertEquals;

import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Pass;
import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.render.RenderFixture;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;

/** {@link FramePlan} on hand-made pass lists: the corner cases of {@code record_frame}. */
class FramePlanTest {
    static final DimensionPipeline BASE = RenderFixture.load(RenderFixture.TUTORIAL4).dim();

    static Pass pass(PassGroup group, int index, List<Integer> computes, Integer program, List<Integer> flipsAfter) {
        return new Pass(group, index, computes, program, flipsAfter, List.of());
    }

    static DimensionPipeline withPasses(List<Pass> passes) {
        DimensionPipeline d = BASE;
        return new DimensionPipeline(d.folder(), d.dimensionIds(), d.targets(), d.settings(), d.uniforms(), d.customUniforms(), d.bindings(),
            d.programs(), d.geometry(), passes, d.gbufferAttachments(), d.shadowAttachments(), d.endOfFrameCopies(), d.distantHorizons());
    }

    /** Steps as short strings. */
    static List<String> describe(FramePlan plan) {
        List<String> out = new ArrayList<>();
        for (FramePlan.Step step : plan.steps()) {
            out.add(switch (step) {
                case FramePlan.Step.Begin b -> "begin " + b.pass().group().wireName() + b.pass().index() + (b.firstOfGroup() ? " first" : "");
                case FramePlan.Step.Geometry g -> "geometry " + g.group().wireName() + (g.pass() == null ? " implicit" : "");
                case FramePlan.Step.Computes c -> "computes " + c.pass().computes() + (c.firstFrameOnly() ? " once" : "");
                case FramePlan.Step.Fullscreen f -> "draw " + f.program() + " " + f.group().wireName();
                case FramePlan.Step.Flip f -> "flip " + f.buffers();
            });
        }
        return out;
    }

    @Test
    void geometryGroupsMissingFromThePassListAreInsertedImplicitly() {
        FramePlan plan = FramePlan.of(withPasses(List.of(
            pass(PassGroup.PREPARE, 0, List.of(), 7, List.of(1)),
            pass(PassGroup.COMPOSITE, 0, List.of(), 8, List.of(0)),
            pass(PassGroup.FINAL, 0, List.of(), 9, List.of()))));
        assertEquals(List.of(
            "geometry shadow implicit",
            "begin prepare0 first", "draw 7 prepare", "flip [1]",
            "geometry gbuffers_opaque implicit", "geometry gbuffers_translucent implicit",
            "begin composite0 first", "draw 8 composite", "flip [0]",
            "begin final0 first", "draw 9 final"), describe(plan));
    }

    @Test
    void geometryWithoutLaterPassesComesAtTheEnd() {
        FramePlan plan = FramePlan.of(withPasses(List.of(pass(PassGroup.BEGIN, 0, List.of(), 3, List.of()))));
        assertEquals(List.of("begin begin0 first", "draw 3 begin", "geometry shadow implicit", "geometry gbuffers_opaque implicit",
            "geometry gbuffers_translucent implicit"), describe(plan));
    }

    @Test
    void aSecondPassOfAGeometryGroupOnlyDispatchesItsComputes() {
        FramePlan plan = FramePlan.of(withPasses(List.of(
            pass(PassGroup.SHADOW, 0, List.of(4), null, List.of()),
            pass(PassGroup.SHADOW, 1, List.of(5), null, List.of()),
            pass(PassGroup.SHADOW, 2, List.of(), null, List.of()))));
        assertEquals(List.of(
            "begin shadow0 first", "geometry shadow",
            "begin shadow1", "computes [5]",
            "begin shadow2",
            "geometry gbuffers_opaque implicit", "geometry gbuffers_translucent implicit"), describe(plan));
    }

    @Test
    void setupComputesRunOnceAndShadowcompFlipsAreNotScheduled() {
        FramePlan plan = FramePlan.of(withPasses(List.of(
            pass(PassGroup.SETUP, 0, List.of(1, 2), null, List.of(3)),
            pass(PassGroup.SHADOW_COMP, 0, List.of(), 6, List.of(0)),
            pass(PassGroup.SHADOW_COMP, 1, List.of(), 7, List.of()))));
        assertEquals(List.of(
            "begin setup0 first", "computes [1, 2] once", "flip [3]",
            "geometry shadow implicit",
            "begin shadow_comp0 first", "draw 6 shadow_comp",
            "begin shadow_comp1", "draw 7 shadow_comp",
            "geometry gbuffers_opaque implicit", "geometry gbuffers_translucent implicit"), describe(plan));
    }

    @Test
    void everyCompiledPackHasEachGeometryGroupExactlyOnce() {
        for (String fixture : List.of(RenderFixture.TUTORIAL4, RenderFixture.GLIMMER)) {
            FramePlan plan = FramePlan.of(RenderFixture.load(fixture).dim());
            for (PassGroup g : FramePlan.GEOMETRY_GROUPS) {
                assertEquals(1, plan.steps().stream().filter(s -> s instanceof FramePlan.Step.Geometry geo && geo.group() == g).count(), fixture + " " + g);
            }
        }
    }
}
