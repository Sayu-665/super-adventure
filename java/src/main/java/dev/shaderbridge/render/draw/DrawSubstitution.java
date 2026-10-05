package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.render.mapping.PipelineMapping;
import dev.shaderbridge.render.pipeline.PipelineShape;
import dev.shaderbridge.render.pipeline.ProgramResolution;
import dev.shaderbridge.render.pipeline.ProgramResolver;
import java.util.EnumSet;
import java.util.HashMap;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.function.Function;

/**
 * Decides which pack program draws a vanilla pipeline's geometry: the vanilla pipeline location is
 * routed to a geometry slot and draw profile ({@code PipelineRouter}), the slot's program for that
 * profile is resolved along its fallback chain ({@link ProgramResolver#geometry}), and the draw
 * uses the pack pipeline when it is compiled. Shadow-pass pipelines cull no faces by default. Draws whose pipeline is not mapped, whose program is
 * missing or cannot run, or whose pipeline is still compiling fall back to vanilla. Final decisions
 * (compiled, or never possible) are cached per vanilla pipeline and pass; pending ones are asked
 * again on every draw. Render thread only.
 */
public final class DrawSubstitution {
    /** Resolves the program of a geometry slot ({@code ProgramResolver::geometry}). */
    @FunctionalInterface
    public interface GeometryPrograms {
        /**
         * @param slot       the geometry slot
         * @param profile    the draw profile of the vanilla vertex data
         * @param shape      the vanilla draw's shape
         * @param shadowPass the draw is in the shadow pass
         * @return the program of the slot's chain that draws, and how
         */
        ProgramResolver.GeometryResolution geometry(GeometryProgram slot, String profile, PipelineShape shape, boolean shadowPass);
    }

    /** Outcome of {@link #decide}. */
    public sealed interface Decision {
        /**
         * Draw with a pack pipeline.
         *
         * @param routed     the slot the vanilla pipeline maps to (render stage)
         * @param program    the program that draws (after fallback)
         * @param resolution its compiled pipeline and binding plan
         */
        record Pack(GeometryProgram routed, Program program, ProgramResolution.Renderpearl resolution) implements Decision {
        }

        /** Draw the vanilla pipeline adapted to the pass. */
        record Vanilla() implements Decision {
        }
    }

    private record Key(RenderPipeline vanilla, boolean shadow) {
    }

    /** Feature geometry Iris keeps out of the shadow map. */
    private static final Set<GeometryProgram> NO_FEATURE_SHADOW = EnumSet.of(GeometryProgram.PARTICLES, GeometryProgram.PARTICLES_TRANSLUCENT,
        GeometryProgram.WEATHER);

    private final DimensionPipeline dim;
    private final Function<RenderPipeline, PipelineMapping> router;
    private final GeometryPrograms programs;
    private final Map<Key, Decision> settled = new HashMap<>();

    /**
     * @param dim      the dimension pipeline
     * @param router   maps vanilla pipelines to slots ({@code PipelineRouter::route})
     * @param programs resolves slot programs
     */
    public DrawSubstitution(DimensionPipeline dim, Function<RenderPipeline, PipelineMapping> router, GeometryPrograms programs) {
        this.dim = dim;
        this.router = router;
        this.programs = programs;
    }

    /**
     * @param vanilla the pipeline vanilla code binds
     * @param shadow  the draw is in the shadow pass
     * @return how to draw it
     */
    public Decision decide(RenderPipeline vanilla, boolean shadow) {
        Key key = new Key(vanilla, shadow);
        Decision known = settled.get(key);
        if (known != null) {
            return known;
        }
        Optional<Decision> decision = resolve(vanilla, shadow);
        decision.ifPresent(d -> settled.put(key, d));
        return decision.orElseGet(Decision.Vanilla::new);
    }

    /**
     * Whether a vanilla pipeline drawn among the frame's prepared features casts a shadow when the
     * shadow pass draws those features again: everything the table gives a shadow program,
     * except particles and weather, which Iris does not draw into the shadow map (their camera-facing
     * quads would cast camera-dependent shadows).
     *
     * @param vanilla a vanilla pipeline
     * @return false when the draw must leave no trace in the shadow map
     */
    public boolean castsFeatureShadow(RenderPipeline vanilla) {
        return !(router.apply(vanilla) instanceof PipelineMapping.Mapped mapped) || !NO_FEATURE_SHADOW.contains(mapped.gbuffers());
    }

    /** @return the decision, or empty while it may still change (a variant or pipeline is compiling) */
    private Optional<Decision> resolve(RenderPipeline vanilla, boolean shadow) {
        if (!(router.apply(vanilla) instanceof PipelineMapping.Mapped mapped)) {
            return Optional.of(new Decision.Vanilla());
        }
        Optional<GeometryProgram> slot = mapped.program(shadow);
        if (slot.isEmpty()) {
            return Optional.of(new Decision.Vanilla());
        }
        PipelineShape shape = shadow ? PipelineShape.of(vanilla).withCull(false) : PipelineShape.of(vanilla);
        ProgramResolver.GeometryResolution r = programs.geometry(slot.get(), mapped.profile(), shape, shadow);
        return switch (r.resolution()) {
            case ProgramResolution.Renderpearl rp -> dim.programFor(r.program())
                .<Decision>map(p -> new Decision.Pack(mapped.gbuffers(), p, rp))
                .or(() -> Optional.of(new Decision.Vanilla()));
            case ProgramResolution.Pending p -> Optional.empty();
            case ProgramResolution.Raw raw -> Optional.of(new Decision.Vanilla());
            case ProgramResolution.Unavailable u -> Optional.of(new Decision.Vanilla());
        };
    }
}
