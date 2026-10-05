package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.GeometrySlot;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * Decides how each program of one dimension pipeline runs: as a renderpearl pipeline
 * ({@link PackPipelineFactory} + {@link PackPipelineCache}), on the {@link RawPath}, or not at
 * all, in which case a geometry slot falls back along its {@link GeometryChain}. Decisions are
 * made once per program and draw shape and reported to the {@link PipelineDiagnostics}; the
 * states they lead to (compiling, ready, failed) are read on every call. Render thread only.
 */
public final class ProgramResolver implements AutoCloseable {
    private final DimensionPipeline dim;
    private final Blobs blobs;
    private final DepthMode depthMode;
    private final PackPipelineFactory factory;
    private final PackPipelineCache cache;
    private final RawPath raw;
    private final VariantSource variants;
    private final PipelineDiagnostics diagnostics;
    private final Map<PipelineKey, Decision> decisions = new HashMap<>();

    /** How a program was decided to run. */
    private sealed interface Decision {
        record Pipeline(PackPipeline pipeline) implements Decision {
        }

        record OnRawPath(RawProgram program) implements Decision {
        }

        record Rejected(List<String> reasons) implements Decision {
        }
    }

    /**
     * Where a geometry slot's draws go.
     *
     * @param program    the program of the chain that draws them (the slot itself unless it fell back)
     * @param resolution how that program runs
     */
    public record GeometryResolution(GeometryProgram program, ProgramResolution resolution) {
    }

    /**
     * @param dim         the dimension pipeline
     * @param blobs       the pack's blob table (fullscreen and compute programs)
     * @param depthMode   the depth convention the pack was compiled for
     * @param factory     builds renderpearl pipelines
     * @param cache       compiles them; the caller polls it once per frame
     * @param raw         runs what renderpearl cannot
     * @param variants    supplies geometry programs compiled for other draw profiles
     * @param diagnostics receives the reasons programs are skipped
     */
    public ProgramResolver(DimensionPipeline dim, Blobs blobs, DepthMode depthMode, PackPipelineFactory factory, PackPipelineCache cache,
                           RawPath raw, VariantSource variants, PipelineDiagnostics diagnostics) {
        this.dim = dim;
        this.blobs = blobs;
        this.depthMode = depthMode;
        this.factory = factory;
        this.cache = cache;
        this.raw = raw;
        this.variants = variants;
        this.diagnostics = diagnostics;
    }

    /**
     * Resolves a composite-style (fullscreen) program or a compute program.
     *
     * @param index  index into {@code dim.programs()}
     * @param layout the attachments of its pass (ignored for compute programs)
     * @return how it runs; compute programs only ever run on the raw path
     */
    public ProgramResolution program(int index, AttachmentLayout layout) {
        Program program = dim.programs().get(index);
        ProgramVariant variant = new ProgramVariant(dim.folder(), program, blobs);
        boolean compute = program.kind() instanceof ProgramKind.Compute || program.kind() instanceof ProgramKind.GeometryCompute;
        return compute ? resolveCompute(variant) : resolve(variant, PipelineShape.fullscreen(), layout);
    }

    /**
     * Resolves the program of a geometry slot for a draw, walking the slot's fallback chain past
     * programs that cannot run.
     *
     * @param slot       the geometry slot ({@code TERRAIN_SOLID}, {@code ENTITIES}, ...)
     * @param profile    the draw profile of the draw's vertex data
     * @param shape      the draw's shape
     * @param shadowPass the draw is in the shadow pass (shadow attachments)
     * @return the program and how it runs; {@link ProgramResolution.Unavailable} when nothing in
     *     the chain can run (draw vanilla)
     */
    public GeometryResolution geometry(GeometryProgram slot, String profile, PipelineShape shape, boolean shadowPass) {
        List<String> reasons = new ArrayList<>();
        Set<Integer> tried = new HashSet<>();
        // The blend of a program that inherits it depends on the slot it draws: the slot is part
        // of the pipeline's identity.
        PipelineShape slotShape = shape.forSlot(slot.wireName());
        for (GeometryProgram candidate : GeometryChain.chain(slot)) {
            GeometrySlot resolved = dim.geometry().get(candidate);
            if (resolved == null || !tried.add(resolved.program())) {
                continue;
            }
            ProgramResolution resolution = switch (variants.find(dim, candidate, profile)) {
                case VariantSource.Lookup.Found found -> {
                    ProgramVariant drawn = new ProgramVariant(found.variant().folder(), drawn(slot, found.variant().program()), found.variant().blobs());
                    yield resolve(drawn, slotShape, AttachmentLayout.geometry(dim, drawn.program(), shadowPass));
                }
                case VariantSource.Lookup.Pending pending -> new ProgramResolution.Pending(candidate.fileName() + " for draw profile " + profile);
                case VariantSource.Lookup.Missing missing -> new ProgramResolution.Unavailable(List.of(missing.reason()));
            };
            if (!(resolution instanceof ProgramResolution.Unavailable unavailable)) {
                return new GeometryResolution(candidate, resolution);
            }
            unavailable.reasons().forEach(r -> reasons.add(candidate.fileName() + ": " + r));
        }
        if (reasons.isEmpty()) {
            reasons.add("the pack has no program for " + slot.fileName());
        }
        return new GeometryResolution(slot, new ProgramResolution.Unavailable(reasons));
    }

    /**
     * The program to draw a slot's geometry with: {@code program} (the slot's program, a variant or
     * a fallback) with the slot's blend and alpha test ({@link GeometrySlot#drawn}). Blend and
     * alpha test belong to the geometry, not to the program file: one {@code gbuffers_terrain}
     * draws solid terrain unblended without an alpha test, cutout terrain at 0.5 and water blended,
     * as in Iris.
     *
     * @param slot    the geometry slot drawn
     * @param program a program drawing it
     * @return the program with the slot's draw state (itself when the model has no slot entry)
     */
    public Program drawn(GeometryProgram slot, Program program) {
        GeometrySlot state = dim.geometry().get(slot);
        return state == null ? program : state.drawn(program);
    }

    private ProgramResolution resolveCompute(ProgramVariant variant) {
        PipelineKey key = new PipelineKey(variant.folder(), variant.program().name(), variant.profile(), "compute", "none");
        Decision decision = decisions.computeIfAbsent(key, k -> offerToRawPath(variant, List.of("it is a compute program")));
        return current(decision, key);
    }

    private ProgramResolution resolve(ProgramVariant variant, PipelineShape shape, AttachmentLayout layout) {
        PipelineKey key = new PipelineKey(variant.folder(), variant.program().name(), variant.profile(), shape.id(), layout.id());
        Decision decision = decisions.get(key);
        if (decision == null) {
            decision = switch (factory.build(dim, variant, shape, layout, depthMode)) {
                case PackPipelineFactory.Result.Built built -> {
                    cache.request(built.pipeline());
                    yield new Decision.Pipeline(built.pipeline());
                }
                case PackPipelineFactory.Result.Ineligible ineligible -> offerToRawPath(variant, ineligible.reasons());
            };
            decisions.put(key, decision);
        }
        return current(decision, key);
    }

    private Decision offerToRawPath(ProgramVariant variant, List<String> renderpearlProblems) {
        return switch (raw.admit(dim, variant, renderpearlProblems)) {
            case RawPath.Admission.Accepted accepted -> new Decision.OnRawPath(accepted.program());
            case RawPath.Admission.Rejected rejected -> {
                List<String> reasons = new ArrayList<>(renderpearlProblems);
                reasons.add(rejected.reason());
                diagnostics.report(variant.program().name() + " [" + variant.profile() + "] cannot run: " + String.join("; ", reasons));
                yield new Decision.Rejected(reasons);
            }
        };
    }

    private ProgramResolution current(Decision decision, PipelineKey key) {
        return switch (decision) {
            case Decision.Pipeline p -> switch (cache.state(key)) {
                case PackPipelineCache.State.Ready ready -> new ProgramResolution.Renderpearl(p.pipeline(), ready.pipeline());
                case PackPipelineCache.State.Failed failed -> new ProgramResolution.Unavailable(List.of(failed.reason()));
                case PackPipelineCache.State.Compiling compiling -> new ProgramResolution.Pending("pipeline " + key.program());
                case PackPipelineCache.State.Missing missing -> new ProgramResolution.Pending("pipeline " + key.program());
            };
            case Decision.OnRawPath r -> switch (r.program().state()) {
                case RawProgram.State.Ready ready -> new ProgramResolution.Raw(r.program());
                case RawProgram.State.Preparing preparing -> new ProgramResolution.Pending("raw program " + key.program());
                case RawProgram.State.Failed failed -> new ProgramResolution.Unavailable(List.of(failed.reason()));
            };
            case Decision.Rejected rejected -> new ProgramResolution.Unavailable(rejected.reasons());
        };
    }

    /** Closes the raw-path programs; renderpearl pipelines belong to the {@link PackPipelineCache}. */
    @Override
    public void close() {
        for (Decision decision : decisions.values()) {
            if (decision instanceof Decision.OnRawPath r) {
                r.program().close();
            }
        }
        decisions.clear();
    }
}
