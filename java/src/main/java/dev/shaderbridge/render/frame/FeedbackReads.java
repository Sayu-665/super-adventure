package dev.shaderbridge.render.frame;

import dev.shaderbridge.model.BindingEntry;
import dev.shaderbridge.model.BindingUse;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.ResourceRef;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Optional;
import java.util.Set;

/**
 * The render targets geometry programs sample while their pass draws into them: Minecraft's main
 * depth ({@code depthtex0}) and the shared {@code gbuffer_attachments} in gbuffers passes,
 * {@code shadowtex0} and the {@code shadow_attachments} in the shadow pass. Iris attaches only a
 * program's own draw buffers (and GL tolerates reading the depth being written); ShaderBridge's
 * shared passes attach all of them, and Vulkan leaves reading an attachment that is being written
 * undefined, so these resources are copied before such a pass and the programs sample the copy, as
 * the headless executor does. Which programs draw in a pass is only known while it draws, so every
 * geometry program of the pass's kind counts.
 */
final class FeedbackReads {
    private FeedbackReads() {
    }

    /**
     * @param dim      the dimension pipeline
     * @param shadow   the shadow pass (otherwise the gbuffers passes, including Distant Horizons')
     * @param attached the targets attached to the pass (colortex, or shadowcolor in the shadow pass)
     * @return the attached resources the pass's programs sample, as {@link #key} forms
     */
    static Set<ResourceRef> of(DimensionPipeline dim, boolean shadow, List<Integer> attached) {
        Set<ResourceRef> out = new LinkedHashSet<>();
        for (Program program : dim.programs()) {
            if (!(program.kind() instanceof ProgramKind.Geometry g) || isShadow(g.program()) != shadow) {
                continue;
            }
            for (BindingUse use : program.bindingsUsed()) {
                dim.bindings().get(use.name()).map(BindingEntry::resource).flatMap(FeedbackReads::key)
                    .filter(r -> attached(r, shadow, attached)).ifPresent(out::add);
            }
        }
        return out;
    }

    /**
     * @param resource a sampled resource
     * @return the form copies are kept under ({@code shadowtex0HW} is {@code shadowtex0}), or
     *     empty for resources no pass draws into
     */
    static Optional<ResourceRef> key(ResourceRef resource) {
        return switch (resource) {
            case ResourceRef.ColorTex c -> Optional.of(c);
            case ResourceRef.ShadowColor c -> Optional.of(c);
            case ResourceRef.DepthTex d when d.index() == 0 -> Optional.of(d);
            case ResourceRef.ShadowTex s when s.index() == 0 -> Optional.of(s);
            case ResourceRef.ShadowTexHw s when s.index() == 0 -> Optional.of(new ResourceRef.ShadowTex(0));
            default -> Optional.empty();
        };
    }

    private static boolean attached(ResourceRef key, boolean shadow, List<Integer> attached) {
        return switch (key) {
            case ResourceRef.ColorTex c -> !shadow && attached.contains(c.index());
            case ResourceRef.ShadowColor c -> shadow && attached.contains(c.index());
            case ResourceRef.DepthTex d -> !shadow;
            case ResourceRef.ShadowTex s -> shadow;
            default -> false;
        };
    }

    private static boolean isShadow(GeometryProgram slot) {
        return slot == GeometryProgram.DH_SHADOW || slot.fileName().startsWith("shadow");
    }
}
