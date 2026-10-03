package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.GeometrySlot;
import dev.shaderbridge.model.Program;
import java.util.Objects;

/**
 * The variants the compiled pack already contains: the slot's program itself when it was
 * translated for the requested profile, else another entry of {@code programs} with the same
 * name and kind translated for it (the pipeline compiler emits one entry per program and profile
 * any slot uses). Anything else is {@link VariantSource.Lookup.Missing}; an on-demand compiler
 * wraps this source.
 */
public final class CompiledVariants implements VariantSource {
    private final Blobs blobs;

    /** @param blobs the blob table of the compiled pack */
    public CompiledVariants(Blobs blobs) {
        this.blobs = blobs;
    }

    @Override
    public Lookup find(DimensionPipeline dim, GeometryProgram slot, String profile) {
        GeometrySlot geometry = dim.geometry().get(slot);
        if (geometry == null) {
            return new Lookup.Missing("the pack has no program for " + slot.fileName());
        }
        Program resolved = dim.programs().get(geometry.program());
        return dim.programs().stream()
            .filter(p -> p == resolved || (p.name().equals(resolved.name()) && p.kind().equals(resolved.kind())))
            .filter(p -> Objects.equals(p.drawProfile(), profile))
            .findFirst()
            .<Lookup>map(p -> new Lookup.Found(new ProgramVariant(dim.folder(), p, blobs)))
            .orElseGet(() -> new Lookup.Missing(resolved.name() + " was not compiled for draw profile " + profile));
    }
}
