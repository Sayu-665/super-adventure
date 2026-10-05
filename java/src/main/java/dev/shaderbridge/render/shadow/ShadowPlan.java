package dev.shaderbridge.render.shadow;

import dev.shaderbridge.model.ShadowSettings;
import java.util.ArrayList;
import java.util.List;

/**
 * What the shadow pass draws, from the pack's shadow directives, in the headless executor's order:
 * opaque casters (terrain, then entities and block entities, then Distant Horizons LODs), the
 * {@code shadowtex0} to {@code shadowtex1} copy (so {@code shadowtex1} holds only opaque
 * casters), then translucent casters.
 *
 * @param steps the steps, empty when the pack has no shadow pass
 * @param notes casters the pack asks for that ShaderBridge renders differently from Iris, or not at all
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
        /**
         * The opaque features Minecraft prepared for the frame (entities, block entities, items),
         * drawn again with the shadow camera's model-view ({@code shadow_entities}, ...).
         */
        ENTITIES,
        /** Distant Horizons LODs ({@code dh_shadow}). */
        DISTANT_TERRAIN,
        /** Copy {@code shadowtex0} into {@code shadowtex1}. */
        COPY_DEPTH,
        /** Translucent terrain ({@code shadow_water}). */
        TRANSLUCENT_TERRAIN
    }

    /**
     * @param shadow         the pack's shadow settings
     * @param distantCasters Distant Horizons LODs cast shadows ({@code DhMode.castsShadows})
     * @return the plan
     */
    public static ShadowPlan of(ShadowSettings shadow, boolean distantCasters) {
        if (!shadow.enabled()) {
            return new ShadowPlan(List.of(), List.of());
        }
        List<Step> steps = new ArrayList<>();
        List<String> notes = new ArrayList<>();
        if (shadow.renderTerrain()) {
            steps.add(Step.OPAQUE_TERRAIN);
        }
        boolean entities = shadow.renderEntities() || shadow.renderBlockEntities() || shadow.renderLightBlockEntities();
        if (entities) {
            steps.add(Step.ENTITIES);
            notes.add("entities, block entities and items cast shadows together (Minecraft prepares them in one batch, so shadowEntities, "
                + "shadowBlockEntities and shadowLightBlockEntities cannot be told apart); only what Minecraft prepared for the camera "
                + "casts shadows (nothing outside its view, and not the first-person player), and translucent entities and particles do not");
        } else if (shadow.renderPlayer()) {
            notes.add("shadowPlayer without shadowEntities: the player casts no shadow (Minecraft prepares it in one batch with the other entities)");
        }
        if (distantCasters) {
            steps.add(Step.DISTANT_TERRAIN);
        }
        steps.add(Step.COPY_DEPTH);
        if (shadow.renderTranslucent()) {
            steps.add(Step.TRANSLUCENT_TERRAIN);
        }
        return new ShadowPlan(steps, notes);
    }
}
