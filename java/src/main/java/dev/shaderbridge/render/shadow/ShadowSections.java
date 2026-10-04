package dev.shaderbridge.render.shadow;

import net.minecraft.client.renderer.LevelRenderer;
import net.minecraft.client.renderer.chunk.ChunkSectionsToRender;
import org.joml.Matrix4fc;

/**
 * Which terrain sections the shadow pass draws: the culling hook of the shadow pass. Iris culls
 * shadow casters against a separate shadow frustum (sections behind the camera still cast shadows
 * into view); {@link #CAMERA_VISIBLE} reuses the sections Minecraft found visible from the camera
 * this frame, so casters outside the view throw no shadow. A shadow-frustum implementation builds
 * its own section list and hands it to the same vanilla draw path.
 */
@FunctionalInterface
public interface ShadowSections {
    /** The camera-visible sections, drawn through the same path (multi-draw-indirect or not) as the main pass. */
    ShadowSections CAMERA_VISIBLE = (level, shadowModelView) -> level.isChunkRenderingUsingMultiDrawIndirect()
        ? level.prepareChunkRendersIndirect(shadowModelView, true)
        : level.prepareChunkRenders(shadowModelView, true);

    /**
     * Prepares the shadow pass's terrain draws. Called on the render thread outside any render
     * pass (it uploads per-frame draw data).
     *
     * @param level           the level renderer
     * @param shadowModelView the shadow camera's model-view (written to the terrain block; shadow
     *                        programs take theirs from {@code shadowModelView})
     * @return the sections to draw
     */
    ChunkSectionsToRender prepare(LevelRenderer level, Matrix4fc shadowModelView);
}
