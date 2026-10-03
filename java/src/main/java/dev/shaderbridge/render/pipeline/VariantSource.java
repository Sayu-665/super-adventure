package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.GeometryProgram;

/**
 * Supplies a geometry slot's program compiled for a draw profile. The compiled pack contains each
 * slot's program for the slot's default profile (and for every other profile some slot needed);
 * the pipeline substitution may need others ({@code vanilla_block} for moving blocks drawn with
 * {@code gbuffers_block}, ...), which an implementation compiles on demand with the native
 * {@code compileVariant} and reports as {@link Lookup.Pending} until done.
 */
@FunctionalInterface
public interface VariantSource {
    /**
     * @param dim     a dimension pipeline
     * @param slot    a geometry slot present in {@code dim.geometry()}
     * @param profile a draw profile
     * @return the slot's program compiled for the profile, or whether it may still come
     */
    Lookup find(DimensionPipeline dim, GeometryProgram slot, String profile);

    /** Outcome of {@link #find}. */
    sealed interface Lookup {
        /** @param variant the program compiled for the profile */
        record Found(ProgramVariant variant) implements Lookup {
        }

        /** The variant is being compiled; ask again later. */
        record Pending() implements Lookup {
        }

        /** @param reason why the variant does not exist and never will (for this pack load) */
        record Missing(String reason) implements Lookup {
        }
    }
}
