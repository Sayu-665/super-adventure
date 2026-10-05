package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.BindingEntry;
import dev.shaderbridge.model.BindingUse;
import dev.shaderbridge.model.CompiledPack;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.ModelJson;
import dev.shaderbridge.model.Pass;
import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.model.json.ModelParseException;
import dev.shaderbridge.render.targets.TargetPlanner;
import dev.shaderbridge.render.targets.TargetSpec;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Optional;
import java.util.Set;
import java.util.stream.Collectors;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.CsvSource;

/**
 * Runs two frames of real compiled packs through {@link FrameSequencer}, {@link PassAttachments},
 * {@link ColorReads} and the target set of {@link TargetPlanner}, and compares the result line by
 * line with the headless executor's trace of the same model (the reference traces of
 * {@code java/src/test/rust/frame-reftrace}): pass order, implicit geometry, computes (setup only
 * on the first frame), geometry attachments, fullscreen outputs and inputs with their main/alt
 * textures, shadowcomp flips, the copy to the output and the end-of-frame copies. Flips are
 * compared separately against the plan, which applies them.
 */
class FrameSequencerTraceTest {
    @ParameterizedTest
    @CsvSource({
        "frame/ComplementaryReimagined.json, world0, frame/ComplementaryReimagined.trace",
        "frame/photon.json, world0, frame/photon.trace",
        "frame/RethinkingVoxels.json, world0, frame/RethinkingVoxels.trace",
        "glimmer/pack.json, world0, frame/glimmer.trace",
        "tutorial4/pack.json, '', frame/tutorial4.trace",
    })
    void matchesTheHeadlessExecutor(String packJson, String folder, String traceFile) throws Exception {
        DimensionPipeline dim = pack(packJson).dimension(folder).orElseThrow();
        List<String> reference = Arrays.asList(read(traceFile).split("\n"));
        Trace trace = new Trace(dim);
        FrameSequencer sequencer = new FrameSequencer(dim, trace.color::contains, trace.keptShadow);
        for (int frame = 0; frame < 2; frame++) {
            trace.out.add("frame " + frame);
            sequencer.begin(frame == 0);
            assertTrue(sequencer.runUntil(PassGroup.GBUFFERS_OPAQUE, trace));
            trace.geometry("gbuffers_opaque", dim.gbufferAttachments(), sequencer.flips());
            trace.out.add("copy depthtex0->depthtex2,depthtex1 dhDepthTex0->dhDepthTex1");
            assertTrue(sequencer.runUntil(PassGroup.GBUFFERS_TRANSLUCENT, trace));
            trace.geometry("gbuffers_translucent", dim.gbufferAttachments(), sequencer.flips());
            sequencer.finish(trace);
        }
        List<String> withoutFlips = reference.stream().filter(l -> !l.startsWith("flip ")).toList();
        assertEquals(String.join("\n", withoutFlips), String.join("\n", trace.out), traceFile);
        List<String> flips = sequencer.plan().steps().stream().filter(s -> s instanceof FramePlan.Step.Flip)
            .map(s -> "flip " + list(((FramePlan.Step.Flip) s).buffers())).toList();
        List<String> referenceFlips = reference.stream().filter(l -> l.startsWith("flip ")).toList();
        assertEquals(referenceFlips, concat(flips, flips), traceFile + ": flips");
    }

    /** Records the steps in the reference trace format. */
    private static final class Trace implements FrameSteps {
        final DimensionPipeline dim;
        final Set<Integer> color;
        final Set<Integer> shadowColor;
        final List<Integer> keptShadow;
        final List<String> out = new ArrayList<>();

        Trace(DimensionPipeline dim) {
            this.dim = dim;
            this.color = TargetPlanner.colorTargets(dim, 1920, 1080, f -> 16384).stream().map(TargetSpec::index).collect(Collectors.toSet());
            List<TargetSpec> shadow = TargetPlanner.shadowColorTargets(dim, f -> 16384);
            this.shadowColor = shadow.stream().map(TargetSpec::index).collect(Collectors.toSet());
            this.keptShadow = shadow.stream().filter(s -> !s.clear()).map(TargetSpec::index).toList();
        }

        void geometry(String group, List<Integer> shared, FlipState flips) {
            boolean shadow = group.equals("shadow");
            List<AttachmentSlot> slots = PassAttachments.geometry(shared, shadow, flips, t -> shadow ? shadowColor.contains(t) : color.contains(t));
            out.add("geometry " + group + " attachments=" + slots.stream().map(FrameSequencerTraceTest::slot).collect(Collectors.joining(",")));
        }

        @Override
        public void passStarted(Pass pass) {
            out.add("pass " + pass.group().wireName() + " " + pass.index());
        }

        @Override
        public void implicitGeometry(PassGroup group) {
            out.add("pass " + group.wireName() + " implicit");
        }

        @Override
        public void computes(Pass pass, FlipState flips) {
            if (!pass.computes().isEmpty()) {
                out.add("computes " + pass.computes().stream().map(i -> dim.programs().get(i).name()).collect(Collectors.joining(",")));
            }
        }

        @Override
        public void shadow(FlipState flips) {
            if (dim.targets().shadow().enabled()) {
                geometry("shadow", dim.shadowAttachments(), flips);
                out.add("copy shadowtex0->shadowtex1");
            }
        }

        @Override
        public Drawn fullscreen(int index, PassGroup group, FlipState flips) {
            Program program = dim.programs().get(index);
            boolean shadow = group == PassGroup.SHADOW_COMP;
            List<AttachmentSlot> slots = PassAttachments.fullscreen(program, group, flips, t -> shadow ? shadowColor.contains(t) : color.contains(t), 8);
            if (slots.isEmpty()) {
                out.add("draw " + program.name() + " nothing");
                return Drawn.NOTHING;
            }
            List<String> writes = new ArrayList<>();
            for (int s = 0; s < slots.size(); s++) {
                writes.add(slots.get(s) instanceof AttachmentSlot.MainColor ? "output" : s + "=" + slot(slots.get(s)));
            }
            List<String> reads = new ArrayList<>();
            for (BindingUse use : program.bindingsUsed()) {
                Optional<BindingEntry> entry = dim.bindings().get(use.name());
                if (entry.isEmpty()) {
                    continue;
                }
                ResourceRef ref = entry.get().resource();
                boolean alt = ColorReads.alt(program.kind(), ref, use.useAlt(), flips, w -> out.add("warn use_alt " + use.name()));
                switch (ref) {
                    case ResourceRef.ColorTex c when color.contains(c.index()) -> reads.add(use.name() + "=" + c.index() + ":" + image(alt));
                    case ResourceRef.ColorImage c when color.contains(c.index()) -> reads.add(use.name() + "=" + c.index() + ":" + image(alt));
                    case ResourceRef.ShadowColor c when shadowColor.contains(c.index()) -> reads.add(use.name() + "=shadowcolor" + c.index() + ":" + image(alt));
                    case ResourceRef.ShadowColorImage c when shadowColor.contains(c.index()) ->
                        reads.add(use.name() + "=shadowcolor" + c.index() + ":" + image(alt));
                    default -> {
                    }
                }
            }
            out.add("draw " + program.name() + " writes=" + String.join(",", writes) + " reads=" + String.join(",", reads));
            List<Integer> written = targets(slots);
            if (shadow && !written.isEmpty()) {
                out.add("shadow-flip " + list(written));
            }
            return new Drawn(true, shadow ? written : List.of());
        }

        @Override
        public void copyToOutput(FlipState flips) {
            out.add("copy-to-output colortex0:" + image(flips.read(0)));
        }

        @Override
        public void endOfFrame(List<Integer> colorCopies, List<Integer> shadowCopies) {
            colorCopies.forEach(i -> out.add("eof colortex" + i));
            shadowCopies.forEach(i -> out.add("eof shadowcolor" + i));
        }

        @Override
        public void warn(String message) {
            assertTrue(message.contains("flip_state"), message);
            out.add("warn flip_state");
        }
    }

    private static String slot(AttachmentSlot slot) {
        return switch (slot) {
            case AttachmentSlot.Target t -> t.target() + ":" + image(t.alt());
            case AttachmentSlot.MainColor m -> "output";
            case AttachmentSlot.Sink s -> "sink";
        };
    }

    private static String image(boolean alt) {
        return alt ? "alt" : "main";
    }

    private static String list(List<Integer> values) {
        return values.stream().map(String::valueOf).collect(Collectors.joining(","));
    }

    private static <T> List<T> concat(List<T> a, List<T> b) {
        List<T> out = new ArrayList<>(a);
        out.addAll(b);
        return out;
    }

    static CompiledPack pack(String path) throws IOException, ModelParseException {
        return ModelJson.parse(read(path), CompiledPack.class);
    }

    static String read(String path) throws IOException {
        try (InputStream in = FrameSequencerTraceTest.class.getResourceAsStream("/dev/shaderbridge/render/" + path)) {
            assertNotNull(in, path);
            return new String(in.readAllBytes(), StandardCharsets.UTF_8);
        }
    }

    static List<Integer> targets(List<AttachmentSlot> slots) {
        return slots.stream().filter(s -> s instanceof AttachmentSlot.Target).map(s -> ((AttachmentSlot.Target) s).target()).toList();
    }
}
