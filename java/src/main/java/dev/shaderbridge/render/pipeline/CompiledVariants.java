package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.GeometrySlot;
import dev.shaderbridge.model.Program;
import java.util.Objects;

/**
 * The variants the compiled pack already contains for a geometry slot: the slot's program itself
 * when it was translated for the requested draw profile, else the entry of the slot's
 * {@link GeometrySlot#variants() variants} map for that profile. The pipeline compiler lists there
 * every other profile it translated the slot's program for, each with the {@code use_alt} of the
 * slot's own pass, so the program found reads the right main/alt textures for this slot; another
 * entry of {@code programs} with the same name may have been translated for a slot of another
 * pass and is never used in its place. Anything else is {@link VariantSource.Lookup.Missing}; an
 * on-demand compiler wraps this source.
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
        if (Objects.equals(resolved.drawProfile(), profile)) {
            return found(dim, resolved);
        }
        Integer variant = geometry.variants().get(profile);
        if (variant == null) {
            return new Lookup.Missing(resolved.name() + " was not compiled for draw profile " + profile);
        }
        if (variant < 0 || variant >= dim.programs().size()) {
            return new Lookup.Missing(resolved.name() + ": the variant for draw profile " + profile + " is program " + variant
                + ", which does not exist");
        }
        Program program = dim.programs().get(variant);
        if (!Objects.equals(program.drawProfile(), profile)) {
            return new Lookup.Missing(resolved.name() + ": the variant listed for draw profile " + profile + " was translated for "
                + program.drawProfile());
        }
        return found(dim, program);
    }

    private Lookup found(DimensionPipeline dim, Program program) {
        return new Lookup.Found(new ProgramVariant(dim.folder(), program, blobs));
    }
}
