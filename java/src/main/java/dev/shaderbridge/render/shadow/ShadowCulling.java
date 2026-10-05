package dev.shaderbridge.render.shadow;

import org.joml.FrustumIntersection;

/**
 * The pure parts of the shadow pass's terrain culling ({@link ShadowSections#SHADOW_FRUSTUM}): how
 * far shadow casters are drawn and whether a section box is in the shadow camera's view.
 */
public final class ShadowCulling {
    private ShadowCulling() {
    }

    /**
     * The distance within which terrain casts shadows, following {@code shadowDistanceRenderMul}:
     * negative (the default) or zero draws casters as far as the world is rendered; a positive
     * multiplier limits them to {@code shadowDistance * shadowDistanceRenderMul}, never beyond the
     * render distance.
     *
     * @param shadowDistance       {@code shadowDistance}
     * @param distanceRenderMul    {@code shadowDistanceRenderMul}
     * @param renderDistanceBlocks the world's render distance in blocks
     * @return the shadow render distance in blocks
     */
    public static double renderDistance(double shadowDistance, double distanceRenderMul, double renderDistanceBlocks) {
        if (!(distanceRenderMul > 0) || !(shadowDistance > 0)) {
            return renderDistanceBlocks;
        }
        double limited = shadowDistance * distanceRenderMul;
        return Double.isFinite(limited) ? Math.min(limited, renderDistanceBlocks) : renderDistanceBlocks;
    }

    /**
     * @param distanceBlocks       the shadow render distance in blocks ({@link #renderDistance})
     * @param renderDistanceChunks the view area's render distance in chunks
     * @return the radius, in sections around the camera's section, of the columns that may cast
     *         shadows: the distance rounded up, at most the render distance, at least 0
     */
    public static int sectionRadius(double distanceBlocks, int renderDistanceChunks) {
        int max = Math.max(renderDistanceChunks, 0);
        if (!(distanceBlocks > 0)) {
            return Double.isNaN(distanceBlocks) ? max : 0;
        }
        double sections = Math.ceil(distanceBlocks / 16.0);
        return sections >= max ? max : (int) sections;
    }

    /**
     * Whether a column of sections lies within the shadow render distance (a cylinder around the
     * camera's column; a section's corner may be up to one section further than its index).
     *
     * @param dx     the column's section x minus the camera's
     * @param dz     the column's section z minus the camera's
     * @param radius {@link #sectionRadius}
     * @return whether the column may cast shadows
     */
    public static boolean columnInRange(int dx, int dz, int radius) {
        if (radius < 0) {
            return false;
        }
        long reach = (long) radius + 1;
        return (long) dx * dx + (long) dz * dz <= reach * reach;
    }

    /**
     * @param frustum the shadow camera's frustum ({@code shadowProjection * shadowModelView})
     * @param minX    the box relative to the camera
     * @param minY    the box relative to the camera
     * @param minZ    the box relative to the camera
     * @param maxX    the box relative to the camera
     * @param maxY    the box relative to the camera
     * @param maxZ    the box relative to the camera
     * @return whether any part of the box may be inside the shadow camera's view; null frustum:
     *         every box is
     */
    public static boolean visible(FrustumIntersection frustum, double minX, double minY, double minZ, double maxX, double maxY, double maxZ) {
        return frustum == null || frustum.testAab((float) minX, (float) minY, (float) minZ, (float) maxX, (float) maxY, (float) maxZ);
    }
}
