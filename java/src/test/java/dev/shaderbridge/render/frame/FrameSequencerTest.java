package dev.shaderbridge.render.frame;

import static dev.shaderbridge.render.frame.FramePlanTest.pass;
import static dev.shaderbridge.render.frame.FramePlanTest.withPasses;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Pass;
import dev.shaderbridge.model.PassGroup;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;
import org.junit.jupiter.api.Test;

/** {@link FrameSequencer}: the split frame, final/output handling, shadowcomp flips and warnings. */
class FrameSequencerTest {
    /** Records calls; fullscreen draws succeed unless their program is listed as failing. */
    private static final class Recorder implements FrameSteps {
        final List<String> calls = new ArrayList<>();
        Set<Integer> failing = Set.of();
        List<Integer> shadowWritten = List.of();

        @Override
        public void passStarted(Pass pass) {
            calls.add("pass " + pass.group().wireName() + pass.index());
        }

        @Override
        public void implicitGeometry(PassGroup group) {
            calls.add("implicit " + group.wireName());
        }

        @Override
        public void computes(Pass pass, FlipState flips) {
            calls.add("computes " + pass.computes());
        }

        @Override
        public void shadow(FlipState flips) {
            calls.add("shadow");
        }

        @Override
        public Drawn fullscreen(int program, PassGroup group, FlipState flips) {
            calls.add("draw " + program + " reads0=" + (flips.read(0) ? "alt" : "main") + " shadow0=" + (flips.shadowRead(0) ? "alt" : "main"));
            return failing.contains(program) ? Drawn.NOTHING : new Drawn(true, shadowWritten);
        }

        @Override
        public void copyToOutput(FlipState flips) {
            calls.add("output " + (flips.read(0) ? "alt" : "main"));
        }

        @Override
        public void endOfFrame(List<Integer> color, List<Integer> shadow) {
            calls.add("eof " + color + " " + shadow);
        }

        @Override
        public void warn(String message) {
            calls.add("warn");
        }
    }

    private static DimensionPipeline frame(List<Pass> passes, List<Integer> endOfFrameCopies) {
        DimensionPipeline d = withPasses(passes);
        return new DimensionPipeline(d.folder(), d.dimensionIds(), d.targets(), d.settings(), d.uniforms(), d.customUniforms(), d.bindings(),
            d.programs(), d.geometry(), d.passes(), d.gbufferAttachments(), d.shadowAttachments(), endOfFrameCopies, d.distantHorizons());
    }

    @Test
    void framesSplitAtTheGeometryMinecraftDraws() {
        DimensionPipeline dim = frame(List.of(
            pass(PassGroup.SETUP, 0, List.of(1), null, List.of()),
            pass(PassGroup.SHADOW, 0, List.of(2), null, List.of()),
            pass(PassGroup.GBUFFERS_OPAQUE, 0, List.of(3), null, List.of()),
            pass(PassGroup.DEFERRED, 0, List.of(), 4, List.of(0)),
            pass(PassGroup.COMPOSITE, 0, List.of(), 5, List.of(0)),
            pass(PassGroup.FINAL, 0, List.of(), 6, List.of())), List.of(0));
        FrameSequencer seq = new FrameSequencer(dim, i -> true, List.of());
        Recorder r = new Recorder();
        for (int frame = 0; frame < 2; frame++) {
            seq.begin(frame == 0);
            assertTrue(seq.runUntil(PassGroup.GBUFFERS_OPAQUE, r));
            r.calls.add("| opaque");
            assertTrue(seq.runUntil(PassGroup.GBUFFERS_TRANSLUCENT, r));
            r.calls.add("| translucent");
            assertTrue(seq.inFrame());
            seq.finish(r);
            assertFalse(seq.inFrame());
        }
        List<String> frame = List.of("pass shadow0", "computes [2]", "shadow", "pass gbuffers_opaque0", "computes [3]", "| opaque",
            "pass deferred0", "draw 4 reads0=main shadow0=main", "implicit gbuffers_translucent", "| translucent",
            "pass composite0", "draw 5 reads0=alt shadow0=main", "pass final0", "draw 6 reads0=main shadow0=main", "eof [] []");
        List<String> expected = new ArrayList<>(List.of("pass setup0", "computes [1]"));
        expected.addAll(frame);
        expected.add("pass setup0");
        expected.addAll(frame);
        assertEquals(expected, r.calls);
    }

    @Test
    void withoutADrawnFinalColortex0GoesToTheOutputAndOddFlipsAreCopiedBack() {
        DimensionPipeline dim = frame(List.of(
            pass(PassGroup.COMPOSITE, 0, List.of(), 5, List.of(0, 3)),
            pass(PassGroup.FINAL, 0, List.of(), 6, List.of())), List.of(0, 3, 7));
        FrameSequencer seq = new FrameSequencer(dim, i -> i != 3, List.of());
        Recorder r = new Recorder();
        r.failing = Set.of(6);
        seq.begin(true);
        seq.runUntil(PassGroup.GBUFFERS_OPAQUE, r);
        seq.runUntil(PassGroup.GBUFFERS_TRANSLUCENT, r);
        seq.finish(r);
        assertEquals(List.of("implicit shadow", "shadow", "implicit gbuffers_opaque", "implicit gbuffers_translucent", "pass composite0",
            "draw 5 reads0=main shadow0=main", "pass final0", "draw 6 reads0=alt shadow0=main", "output alt", "eof [0] []"), r.calls);
    }

    @Test
    void shadowcompFlipsFollowTheWrittenTargets() {
        DimensionPipeline dim = frame(List.of(
            pass(PassGroup.SHADOW_COMP, 0, List.of(), 5, List.of(0)),
            pass(PassGroup.SHADOW_COMP, 1, List.of(), 6, List.of()),
            pass(PassGroup.FINAL, 0, List.of(), 7, List.of())), List.of());
        FrameSequencer seq = new FrameSequencer(dim, i -> true, List.of(0, 1));
        Recorder r = new Recorder();
        r.shadowWritten = List.of(0);
        seq.begin(true);
        seq.runUntil(PassGroup.GBUFFERS_OPAQUE, r);
        assertTrue(r.calls.contains("draw 5 reads0=main shadow0=main"));
        assertTrue(r.calls.contains("draw 6 reads0=main shadow0=alt"), r.calls.toString());
        seq.runUntil(PassGroup.GBUFFERS_TRANSLUCENT, r);
        r.shadowWritten = List.of();
        seq.finish(r);
        assertEquals("eof [] []", r.calls.getLast(), "shadowcolor0 flipped twice ends in main");
    }

    @Test
    void flipStateDisagreementsAreReportedExceptAtTheStartOfAGroup() {
        DimensionPipeline dim = frame(List.of(
            new Pass(PassGroup.COMPOSITE, 0, List.of(), 5, List.of(), List.of(true)),
            new Pass(PassGroup.COMPOSITE, 1, List.of(), 6, List.of(), List.of(false)),
            new Pass(PassGroup.FINAL, 0, List.of(), 7, List.of(), List.of(false))), List.of());
        FrameSequencer seq = new FrameSequencer(dim, i -> true, List.of());
        Recorder r = new Recorder();
        seq.begin(true);
        seq.runUntil(PassGroup.GBUFFERS_OPAQUE, r);
        seq.runUntil(PassGroup.GBUFFERS_TRANSLUCENT, r);
        seq.finish(r);
        assertEquals(1, r.calls.stream().filter("warn"::equals).count(), r.calls.toString());
        assertTrue(r.calls.contains("draw 5 reads0=alt shadow0=main"), "the model's flip state wins");
    }

    @Test
    void geometryPassedOverIsNotDueAndFramesMustBegin() {
        DimensionPipeline dim = frame(List.of(pass(PassGroup.FINAL, 0, List.of(), 7, List.of())), List.of());
        FrameSequencer seq = new FrameSequencer(dim, i -> true, List.of());
        Recorder r = new Recorder();
        assertThrows(IllegalStateException.class, () -> seq.runUntil(PassGroup.GBUFFERS_OPAQUE, r));
        seq.begin(true);
        assertTrue(seq.runUntil(PassGroup.GBUFFERS_TRANSLUCENT, r));
        assertFalse(seq.runUntil(PassGroup.GBUFFERS_OPAQUE, r));
        seq.abandon();
        assertFalse(seq.inFrame());
        assertThrows(IllegalStateException.class, () -> seq.finish(r));
    }
}
