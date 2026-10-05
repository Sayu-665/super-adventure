package dev.shaderbridge.dh;

/**
 * The near and far planes of the Distant Horizons projection that packs see as
 * {@code dhNearPlane} / {@code dhFarPlane} and that ShaderBridge draws LODs with. The values
 * follow what Iris reports for Distant Horizons 3.3 (and what the headless executor uses):
 *
 * <ul>
 *   <li>far: {@code (DH render distance in chunks * 16 + 512) * sqrt(2)}, the LOD distance plus
 *   one region, widened so that the corners of the square LOD area are not clipped;</li>
 *   <li>near: Distant Horizons' {@code RenderUtil.getNearClipPlaneInBlocks} while a shader pack is
 *   in use: the overdraw prevention fraction of the vanilla render distance (0.2 in automatic
 *   mode, since packs hide LODs near the camera themselves), at least one block, moved to the
 *   distance of the frustum corner of a fixed 70° field of view; 0.5 in Distant Horizons'
 *   LOD-only debug mode; the height above the world when the player flies more than 1000 blocks
 *   above it.</li>
 * </ul>
 *
 * Distant Horizons also brings the near plane closer while the camera moves fast
 * ({@code reduceOverdrawWithFastMovement}, not part of its API); that is not reproduced.
 */
public final class DhPlanes {
    /** Minecraft's near plane ({@code Camera.setupPerspective(0.05F, ...)}). */
    public static final float MINECRAFT_NEAR = 0.05f;
    /** Width of a Distant Horizons region in blocks ({@code LodUtil.REGION_WIDTH}). */
    static final int REGION_WIDTH = 512;
    /** Overdraw prevention Distant Horizons uses in automatic mode while a shader pack is in use. */
    static final float SHADER_PACK_OVERDRAW = 0.2f;
    /** Overdraw prevention bounds Distant Horizons clamps a configured value to. */
    static final float MIN_OVERDRAW = 0.05f;
    /** Near plane of Distant Horizons' LOD-only debug mode. */
    static final float LOD_ONLY_NEAR = 0.5f;
    /** The fixed field of view Distant Horizons computes its near plane with, in degrees. */
    static final double NEAR_PLANE_FOV = 70;
    /** Height above the level's height (in blocks) from which the near plane follows the player up. */
    static final int HEIGHT_OVERRIDE_MARGIN = 1000;

    private DhPlanes() {
    }

    /**
     * @param lodChunks Distant Horizons' render distance in chunks ({@code chunkRenderDistance})
     * @return {@code dhFarPlane}
     */
    public static float farPlane(int lodChunks) {
        return (float) ((lodChunks * 16.0 + REGION_WIDTH) * Math.sqrt(2));
    }

    /**
     * {@code dhNearPlane}.
     *
     * @param vanillaChunks  Minecraft's effective render distance in chunks
     * @param aspect         width / height of Minecraft's main render target
     * @param overdraw       Distant Horizons' {@code overdrawPreventionRadius} setting (negative: automatic)
     * @param lodOnly        Distant Horizons' LOD-only debug mode
     * @param heightOverride {@link #heightOverride}, or a negative value for none
     * @return the near plane distance in blocks
     */
    public static float nearPlane(int vanillaChunks, double aspect, float overdraw, boolean lodOnly, float heightOverride) {
        float fraction = overdraw < 0 ? SHADER_PACK_OVERDRAW : Math.clamp(overdraw, MIN_OVERDRAW, 1.0f);
        float near = lodOnly ? LOD_ONLY_NEAR : Math.max(vanillaChunks * 16 * fraction, 1.0f);
        if (heightOverride >= 0) {
            near = heightOverride;
        }
        double a = Double.isFinite(aspect) && aspect > 0 ? aspect : 1.0;
        double tan = Math.tan(Math.toRadians(NEAR_PLANE_FOV / 2));
        return (float) (near / Math.sqrt(1 + tan * tan * (a * a + 1)));
    }

    /**
     * Distant Horizons' height-based near plane ({@code getHeightBasedNearClipOverrideBlockDistance}):
     * far above the world, depth precision needs a farther near plane.
     *
     * @param playerBlockY the player's block Y
     * @param levelHeight  the level's height in blocks ({@code Level.getHeight()}, what Distant
     *                     Horizons calls its max height)
     * @return the near plane to use, or -1 when the player is not that high
     */
    public static float heightOverride(int playerBlockY, int levelHeight) {
        int threshold = levelHeight + HEIGHT_OVERRIDE_MARGIN;
        return playerBlockY > threshold ? playerBlockY - threshold : -1;
    }
}
