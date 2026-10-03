package dev.shaderbridge.render.mapping;

import dev.shaderbridge.model.GeometryProgram;
import java.util.Objects;
import java.util.Optional;

/** What happens to a vanilla draw while a shader pack is active. */
public sealed interface PipelineMapping {
    /**
     * The draw is substituted with a pack program.
     *
     * @param gbuffers the program of the main (gbuffers) pass
     * @param shadow   the program of the shadow pass, if this geometry casts shadows
     * @param profile  the draw profile the vanilla vertex data and host bindings match; the program
     *                 must be compiled for it
     */
    record Mapped(GeometryProgram gbuffers, Optional<GeometryProgram> shadow, String profile) implements PipelineMapping {
        public Mapped {
            Objects.requireNonNull(gbuffers, "gbuffers");
            Objects.requireNonNull(shadow, "shadow");
            Objects.requireNonNull(profile, "profile");
        }

        /**
         * @param shadowPass whether the draw happens in the shadow pass
         * @return the program for that pass, if the geometry is drawn there
         */
        public Optional<GeometryProgram> program(boolean shadowPass) {
            return shadowPass ? shadow : Optional.of(gbuffers);
        }
    }

    /**
     * The draw keeps its vanilla pipeline (writing the pack's {@code fallback_tex} in the main pass,
     * nothing in the shadow pass).
     *
     * @param reason why it is not substituted
     */
    record Vanilla(String reason) implements PipelineMapping {
        public Vanilla {
            Objects.requireNonNull(reason, "reason");
        }
    }
}
