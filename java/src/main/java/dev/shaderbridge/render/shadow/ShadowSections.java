package dev.shaderbridge.render.shadow;

import it.unimi.dsi.fastutil.objects.ObjectArrayList;
import java.util.ArrayList;
import java.util.List;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.renderer.LevelRenderer;
import net.minecraft.client.renderer.ViewArea;
import net.minecraft.client.renderer.chunk.ChunkSectionsToRender;
import net.minecraft.client.renderer.chunk.SectionMesh;
import net.minecraft.client.renderer.chunk.SectionRenderDispatcher;
import net.minecraft.core.BlockPos;
import net.minecraft.world.phys.AABB;
import org.joml.FrustumIntersection;
import org.joml.Matrix4f;

/**
 * Which terrain sections the shadow pass draws: the culling hook of the shadow pass.
 *
 * <ul>
 *   <li>{@link #SHADOW_FRUSTUM}: every section with geometry within the shadow distance whose box
 *   lies in the shadow camera's view volume ({@link ShadowCulling}), whether the player's camera
 *   sees it or not, so that terrain behind or above the camera still casts shadows into view, as
 *   with Iris' shadow frustum;</li>
 *   <li>{@link #CAMERA_VISIBLE}: the sections Minecraft found visible from the camera this frame
 *   (casters outside the camera's view throw no shadow).</li>
 * </ul>
 *
 * Both draw through Minecraft's own path ({@code prepareChunkRenders}, multi-draw-indirect or not,
 * as the main pass); the shadow frustum hands it its section list in place of the camera's for
 * the duration of the call. With Sodium, Minecraft's own sections are empty: ShaderBridge's Sodium
 * integration answers {@code prepareChunkRenders} with the sections Sodium draws for the camera,
 * so with Sodium both draw those.
 */
@FunctionalInterface
public interface ShadowSections {
    /**
     * The shadow camera of the frame.
     *
     * @param modelView     the shadow model-view (camera-relative world positions in)
     * @param projection    the shadow projection
     * @param cameraX       the player camera's world position
     * @param cameraY       the player camera's world position
     * @param cameraZ       the player camera's world position
     * @param distance      the shadow render distance in blocks ({@link ShadowCulling#renderDistance})
     * @param frustumCulled whether casters outside the shadow camera's view volume are culled
     *                      ({@code shadow.culling} other than {@code false}); without, every section
     *                      within the distance casts shadows
     */
    record ShadowView(Matrix4f modelView, Matrix4f projection, double cameraX, double cameraY, double cameraZ, double distance,
                      boolean frustumCulled) {
        /**
         * @param culling the pack's {@code shadow.culling} as the model spells it ({@code default},
         *                {@code distance}, {@code advanced} or {@code safe_zone})
         * @return whether sections are frustum culled: all but {@code distance} ({@code safe_zone}
         *         culls like {@code advanced})
         */
        public static boolean culls(String culling) {
            return !"distance".equals(culling);
        }
    }

    /** The camera-visible sections, drawn through the same path (multi-draw-indirect or not) as the main pass. */
    ShadowSections CAMERA_VISIBLE = (level, view) -> prepareVisible(level, view);

    /**
     * The sections in the shadow camera's view within the shadow distance; the camera-visible ones
     * when the level has no view area yet or reading it fails. Only sections Minecraft has built
     * are drawn: Minecraft builds the sections it sees, so terrain never yet in the camera's view
     * casts no shadow until it is built.
     */
    ShadowSections SHADOW_FRUSTUM = (level, view) -> {
        if (sodiumTerrain()) {
            return prepareVisible(level, view);
        }
        ObjectArrayList<SectionRenderDispatcher.RenderSection> visible = level.visibleSections();
        List<SectionRenderDispatcher.RenderSection> casters;
        try {
            casters = casters(level.viewArea(), view);
        } catch (RuntimeException e) {
            casters = null;
        }
        if (casters == null || visible == null) {
            return prepareVisible(level, view);
        }
        List<SectionRenderDispatcher.RenderSection> camera = new ArrayList<>(visible);
        visible.clear();
        visible.addAll(casters);
        try {
            return prepareVisible(level, view);
        } finally {
            visible.clear();
            visible.addAll(camera);
        }
    };

    /**
     * Prepares the shadow pass's terrain draws. Called on the render thread outside any render
     * pass (it uploads per-frame draw data).
     *
     * @param level the level renderer
     * @param view  the shadow camera (its model-view is written to the terrain block; shadow
     *              programs take theirs from {@code shadowModelView})
     * @return the sections to draw
     */
    ChunkSectionsToRender prepare(LevelRenderer level, ShadowView view);

    private static ChunkSectionsToRender prepareVisible(LevelRenderer level, ShadowView view) {
        // With Sodium, ShaderBridge's Sodium integration answers prepareChunkRenders (only) with Sodium's sections.
        return !sodiumTerrain() && level.isChunkRenderingUsingMultiDrawIndirect()
            ? level.prepareChunkRendersIndirect(view.modelView(), true)
            : level.prepareChunkRenders(view.modelView(), true);
    }

    /** @return whether Sodium renders the terrain (Minecraft's own sections are then empty) */
    private static boolean sodiumTerrain() {
        return FabricLoader.getInstance().isModLoaded("sodium");
    }

    /** @return the sections with geometry in the shadow view, or null without a view area */
    private static List<SectionRenderDispatcher.RenderSection> casters(ViewArea area, ShadowView view) {
        if (area == null) {
            return null;
        }
        FrustumIntersection frustum = view.frustumCulled() ? new FrustumIntersection(new Matrix4f(view.projection()).mul(view.modelView())) : null;
        int radius = ShadowCulling.sectionRadius(view.distance(), area.getViewDistance());
        int cx = Math.floorDiv((int) Math.floor(view.cameraX()), 16);
        int cz = Math.floorDiv((int) Math.floor(view.cameraZ()), 16);
        List<SectionRenderDispatcher.RenderSection> out = new ArrayList<>();
        BlockPos.MutableBlockPos pos = new BlockPos.MutableBlockPos();
        for (int sx = cx - radius; sx <= cx + radius; sx++) {
            for (int sz = cz - radius; sz <= cz + radius; sz++) {
                if (!ShadowCulling.columnInRange(sx - cx, sz - cz, radius)) {
                    continue;
                }
                for (int sy = area.minSectionY(); sy <= area.maxSectionY(); sy++) {
                    SectionRenderDispatcher.RenderSection section = area.getRenderSectionAt(pos.set(sx * 16, sy * 16, sz * 16));
                    if (section == null) {
                        continue;
                    }
                    SectionMesh mesh = section.getSectionMesh();
                    if (mesh == null || !mesh.hasRenderableLayers()) {
                        continue;
                    }
                    AABB box = section.getBoundingBox();
                    if (ShadowCulling.visible(frustum, box.minX - view.cameraX(), box.minY - view.cameraY(), box.minZ - view.cameraZ(),
                        box.maxX - view.cameraX(), box.maxY - view.cameraY(), box.maxZ - view.cameraZ())) {
                        out.add(section);
                    }
                }
            }
        }
        return out;
    }
}
