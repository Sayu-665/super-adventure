package dev.shaderbridge.render.shadow;

import dev.shaderbridge.model.ShadowSettings;
import java.util.ArrayList;
import java.util.List;

/**
 * What the shadow pass draws, from the pack's shadow directives, in the headless executor's order:
 * opaque casters, the {@code shadowtex0} to {@code shadowtex1} copy (so {@code shadowtex1} holds
 * only opaque casters), then translucent casters.
 *
 * @param steps the steps, empty when the pack has no shadow pass
 * @param notes casters the pack asks for that ShaderBridge does not render into the shadow map
 */
public record ShadowPlan(List<Step> steps, List<String> notes) {
    public ShadowPlan {
        steps = List.copyOf(steps);
        notes = List.copyOf(notes);
    }

    /** One step of the shadow pass. */
    public enum Step {
        /** Solid and cutout terrain ({@code shadow_solid}, {@code shadow_cutout}, {@code shadow}). */
        OPAQUE_TERRAIN,
        /** Copy {@code shadowtex0} into {@code shadowtex1}. */
        COPY_DEPTH,
        /** Translucent terrain ({@code shadow_water}). */
        TRANSLUCENT_TERRAIN
    }

    /**
     * @param shadow the pack's shadow settings
     * @return the plan
     */
    public static ShadowPlan of(ShadowSettings shadow) {
        if (!shadow.enabled()) {
            return new ShadowPlan(List.of(), List.of());
        }
        List<Step> steps = new ArrayList<>();
        List<String> notes = new ArrayList<>();
        if (shadow.renderTerrain()) {
            steps.add(Step.OPAQUE_TERRAIN);
        }
        steps.add(Step.COPY_DEPTH);
        if (shadow.renderTranslucent()) {
            steps.add(Step.TRANSLUCENT_TERRAIN);
        }
        if (shadow.renderEntities() || shadow.renderPlayer() || shadow.renderBlockEntities()) {
            notes.add("entities, the player and block entities do not cast shadows: Minecraft prepares their draws for the camera view only");
        }
        return new ShadowPlan(steps, notes);
    }
}
