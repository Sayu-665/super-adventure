package dev.shaderbridge.dh;

import java.util.ArrayList;
import java.util.List;
import org.joml.FrustumIntersection;
import org.joml.Matrix4fc;

/**
 * Chooses the LOD buffers one pass draws. Distant Horizons' own frustum culling is turned off
 * while a pack renders (so that the shadow pass sees every section), so each pass culls the
 * sections against its own view: the camera projection for the gbuffers passes, the shadow
 * projection for the shadow pass. Sections are tested as boxes over their XZ footprint and the
 * level's height, in camera-relative coordinates (the space the pass's matrix maps from).
 *
 * @param clip        projection times model-view of the pass (GL convention), for
 *                    camera-relative positions; null draws every section (no culling)
 * @param cameraX     camera X
 * @param cameraY     camera Y
 * @param cameraZ     camera Z
 * @param minY        the level's minimum Y
 * @param maxY        the level's maximum Y (exclusive)
 * @param vanillaArea radius in blocks around the camera within which a section lying completely
 *                    inside is skipped (vanilla terrain covers it), or 0 to keep all sections
 */
public record LodSelection(Matrix4fc clip, double cameraX, double cameraY, double cameraZ, int minY, int maxY, double vanillaArea) {
    /**
     * The radius vanilla terrain surely covers for a render distance: one chunk less than the
     * render distance, which absorbs the chunk alignment and the rounded corners of Minecraft's
     * visible area.
     *
     * @param renderDistanceChunks Minecraft's effective render distance in chunks
     * @return the radius in blocks (0 for render distances of one chunk or less)
     */
    public static double vanillaRadius(int renderDistanceChunks) {
        return Math.max(0, renderDistanceChunks - 1) * 16.0;
    }

    /**
     * @param buffers candidate buffers
     * @return the buffers the pass draws, in their order
     */
    public List<LodBuffer> select(List<LodBuffer> buffers) {
        FrustumIntersection frustum = clip == null ? null : new FrustumIntersection(clip, false);
        List<LodBuffer> out = new ArrayList<>(buffers.size());
        for (LodBuffer b : buffers) {
            if (vanillaArea > 0 && insideVanillaArea(b)) {
                continue;
            }
            if (frustum != null && !frustum.testAab((float) (b.minX() - cameraX), (float) (minY - cameraY), (float) (b.minZ() - cameraZ),
                (float) (b.minX() + b.width() - cameraX), (float) (maxY - cameraY), (float) (b.minZ() + b.width() - cameraZ))) {
                continue;
            }
            out.add(b);
        }
        return out;
    }

    /** Whether the section's whole XZ footprint lies within {@link #vanillaArea} of the camera. */
    private boolean insideVanillaArea(LodBuffer b) {
        double dx = Math.max(Math.abs(b.minX() - cameraX), Math.abs(b.minX() + b.width() - cameraX));
        double dz = Math.max(Math.abs(b.minZ() - cameraZ), Math.abs(b.minZ() + b.width() - cameraZ));
        return dx * dx + dz * dz < vanillaArea * vanillaArea;
    }
}
