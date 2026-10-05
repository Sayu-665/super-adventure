package dev.shaderbridge.render.shadow;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.textures.FilterMode;
import com.mojang.renderpearl.api.textures.GpuSampler;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.LevelRenderer;
import net.minecraft.client.renderer.chunk.ChunkSectionLayerGroup;
import net.minecraft.client.renderer.chunk.ChunkSectionsToRender;
import net.minecraft.data.AtlasIds;
import org.joml.Matrix4fc;

/**
 * Renders the shadow pass by re-running Minecraft's terrain draw path into a ShaderBridge pass on
 * the shadow targets: the chunk sections ({@link ShadowSections}) are prepared again for the
 * shadow camera, and {@code ChunkSectionsToRender.renderGroup} binds the vanilla terrain pipelines,
 * which the pipeline substitution replaces with the pack's shadow programs (vanilla pipelines
 * without a shadow program draw nothing there). The frame's prepared features (entities, block
 * entities) are drawn again the same way ({@link ShadowTargets#drawFeatures}). Render thread only,
 * outside any render pass.
 */
public final class ShadowRenderer {
    /** The shadow targets, provided by the frame orchestration. */
    public interface ShadowTargets {
        /**
         * Opens a render pass on the shadow attachments and {@code shadowtex0}, registered so that
         * its vanilla draws are substituted with shadow programs.
         *
         * @param label debug label
         * @return the open pass
         */
        RenderPass open(String label);

        /**
         * A pass from {@link #open} was closed.
         *
         * @param pass the pass
         */
        void closed(RenderPass pass);

        /** Copies {@code shadowtex0} into {@code shadowtex1}. */
        void copyDepth();

        /** Draws the Distant Horizons LODs that cast shadows ({@code dh_shadow}), in passes of its own. */
        void drawDistant();

        /**
         * Draws the frame's prepared opaque features (entities, block entities) with the shadow
         * camera, in a pass of its own on the shadow attachments.
         */
        void drawFeatures();
    }

    private final ShadowPlan plan;
    private final ShadowSections sections;

    /**
     * @param plan     what the pack's shadow pass draws
     * @param sections which terrain sections cast shadows
     */
    public ShadowRenderer(ShadowPlan plan, ShadowSections sections) {
        this.plan = plan;
        this.sections = sections;
    }

    /**
     * @param level           the level renderer of the frame
     * @param shadowModelView the shadow camera's model-view
     * @param targets         the shadow targets
     */
    public void render(LevelRenderer level, Matrix4fc shadowModelView, ShadowTargets targets) {
        if (plan.steps().isEmpty()) {
            return;
        }
        ChunkSectionsToRender chunks = null;
        if (needsTerrain()) {
            chunks = sections.prepare(level, shadowModelView);
            // Preparing only records how many quad indices the sections need; Minecraft grows its
            // shared index buffer later in the frame (prepareTranslucents), after this pass.
            RenderSystem.resizeAllAutoStorageIndexBuffers();
        }
        for (ShadowPlan.Step step : plan.steps()) {
            switch (step) {
                case OPAQUE_TERRAIN -> draw(chunks, ChunkSectionLayerGroup.OPAQUE, targets);
                case ENTITIES -> targets.drawFeatures();
                case DISTANT_TERRAIN -> targets.drawDistant();
                case COPY_DEPTH -> targets.copyDepth();
                case TRANSLUCENT_TERRAIN -> draw(chunks, ChunkSectionLayerGroup.TRANSLUCENT, targets);
            }
        }
    }

    /** @return whether the shadow pass draws the frame's prepared features ({@link ShadowPlan.Step#ENTITIES}) */
    public boolean drawsFeatures() {
        return plan.steps().contains(ShadowPlan.Step.ENTITIES);
    }

    private boolean needsTerrain() {
        return plan.steps().contains(ShadowPlan.Step.OPAQUE_TERRAIN) || plan.steps().contains(ShadowPlan.Step.TRANSLUCENT_TERRAIN);
    }

    private static void draw(ChunkSectionsToRender chunks, ChunkSectionLayerGroup group, ShadowTargets targets) {
        GpuTextureView atlas = Minecraft.getInstance().getAtlasManager().getAtlasOrThrow(AtlasIds.BLOCKS).getTextureView();
        GpuSampler sampler = RenderSystem.getSamplerCache().getClampToEdge(FilterMode.LINEAR, true);
        RenderPass pass = targets.open("ShaderBridge shadow " + group.label());
        try (pass) {
            chunks.renderGroup(group, pass, sampler, atlas, false);
        } finally {
            targets.closed(pass);
        }
    }
}
