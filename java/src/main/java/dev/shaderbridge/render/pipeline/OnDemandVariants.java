package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.GeometryProgram;
import java.util.HashMap;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.Executor;

/**
 * Supplies geometry programs for draw profiles the compiled pack lacks by compiling them on demand
 * (natively, with {@code compileVariant}) on a background executor. Until a variant is compiled
 * its lookup is {@link VariantSource.Lookup.Pending}, and the draw stays vanilla. Every variant is
 * compiled at most once per pack load; a failure is reported as
 * {@link VariantSource.Lookup.Missing} from then on. Variants the pack already contains come from
 * the wrapped source. Render thread only (the compiler runs on the executor).
 */
public final class OnDemandVariants implements VariantSource {
    private final VariantSource compiled;
    private final VariantCompiler compiler;
    private final Executor executor;
    private final Map<Request, CompletableFuture<Lookup>> requests = new HashMap<>();

    /** Compiles one variant. */
    @FunctionalInterface
    public interface VariantCompiler {
        /**
         * @param folder  world folder of the dimension pipeline
         * @param slot    the geometry slot (the compiler follows its fallback chain)
         * @param profile the draw profile
         * @return the compiled program with its blobs
         * @throws Exception if it cannot be compiled
         */
        ProgramVariant compile(String folder, GeometryProgram slot, String profile) throws Exception;
    }

    private record Request(String folder, GeometryProgram slot, String profile) {
    }

    /**
     * @param compiled the variants the pack contains ({@link CompiledVariants})
     * @param compiler compiles the others
     * @param executor runs the compiler off the render thread
     */
    public OnDemandVariants(VariantSource compiled, VariantCompiler compiler, Executor executor) {
        this.compiled = compiled;
        this.compiler = compiler;
        this.executor = executor;
    }

    @Override
    public Lookup find(DimensionPipeline dim, GeometryProgram slot, String profile) {
        Lookup known = compiled.find(dim, slot, profile);
        if (!(known instanceof Lookup.Missing) || !dim.geometry().containsKey(slot)) {
            return known;
        }
        CompletableFuture<Lookup> request = requests.computeIfAbsent(new Request(dim.folder(), slot, profile),
            r -> CompletableFuture.supplyAsync(() -> compile(r), executor)
                .exceptionally(e -> new Lookup.Missing(r.slot().fileName() + " cannot be compiled for draw profile " + r.profile() + ": " + e)));
        return request.isDone() ? request.join() : new Lookup.Pending();
    }

    private Lookup compile(Request r) {
        try {
            ProgramVariant variant = compiler.compile(r.folder(), r.slot(), r.profile());
            if (!r.profile().equals(variant.profile())) {
                return new Lookup.Missing("the compiled variant is for draw profile " + variant.profile() + ", not " + r.profile());
            }
            return new Lookup.Found(variant);
        } catch (Exception e) {
            return new Lookup.Missing(r.slot().fileName() + " cannot be compiled for draw profile " + r.profile() + ": " + e.getMessage());
        }
    }
}
