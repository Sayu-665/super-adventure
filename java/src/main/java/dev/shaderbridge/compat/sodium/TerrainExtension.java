package dev.shaderbridge.compat.sodium;

/**
 * The values of the extension attributes of the extended terrain vertex
 * ({@link TerrainVertexLayout}), computed from a quad and the block it belongs to, in the
 * encodings the {@value SodiumPipelines#PROFILE} draw profile decodes (the conventions Iris's
 * extended Sodium vertex uses, so packs see the same values):
 *
 * <ul>
 *   <li>{@code sb_Entity}: {@code ((block id + 1) << 1) | is fluid}, so an unmapped block (id -1)
 *   is 0; the profile decodes {@code mc_Entity = (id, is fluid, 0, 1)}.</li>
 *   <li>{@code sb_Normal}: the quad's face normal from its winding (vertex 0 to 2 crossed with
 *   vertex 1 to 3, which is the outward normal of Minecraft's counter-clockwise quads, and of the
 *   triangles Sodium stores as quads with a repeated vertex), as signed bytes {@code round(n *
 *   127)}.</li>
 *   <li>{@code sb_MidTexCoord}: the mean texture coordinate of the quad's four vertices, times
 *   32768, in two 16-bit halves.</li>
 *   <li>{@code sb_MidBlock}: per vertex, {@code (block centre - vertex position) * 64} rounded
 *   and clamped to {@code [-127, 127]} (the range {@code RGBA8_SNORM} decodes without loss), in
 *   section-local block units, and the block's light emission (0 to 15) in the fourth byte; the
 *   same values ShaderBridge's extended vanilla chunk vertex carries (Iris truncates instead of
 *   rounding, a difference below 1/64 block).</li>
 * </ul>
 *
 * <p>A <em>block reference</em> packs the section-local position of the block a quad belongs to
 * and its light emission into one int ({@link #block}); {@link #NO_BLOCK} means the quad belongs
 * to no block (geometry added by other mods outside Sodium's block loop), for which
 * {@code sb_MidBlock} is zero.
 *
 * <p>Pure functions; thread-safe.
 */
public final class TerrainExtension {
    /** Block reference of quads that belong to no block. */
    public static final int NO_BLOCK = 0;
    /** {@code sb_Normal} of degenerate quads: straight up. */
    public static final int UP = packNormal(0.0f, 1.0f, 0.0f);

    private static final int VALID = 1 << 31;
    private static final float MID_TEX_COORD_SCALE = 32768.0f;
    private static final float MID_BLOCK_SCALE = 64.0f;

    private TerrainExtension() {
    }

    /**
     * @param blockId the pack's {@code block.properties} id of the block, -1 when unmapped
     * @param fluid   whether the quad belongs to a fluid
     * @return the {@code sb_Entity} value
     */
    public static int entity(int blockId, boolean fluid) {
        return ((Math.max(blockId, -1) + 1) << 1) | (fluid ? 1 : 0);
    }

    /**
     * The face normal of a quad.
     *
     * @param positions the quad's vertex positions, {@code x0, y0, z0, ..., x3, y3, z3}
     * @return the {@code sb_Normal} value ({@link #UP} for degenerate quads)
     */
    public static int normal(float[] positions) {
        float ax = positions[6] - positions[0];
        float ay = positions[7] - positions[1];
        float az = positions[8] - positions[2];
        float bx = positions[9] - positions[3];
        float by = positions[10] - positions[4];
        float bz = positions[11] - positions[5];
        float nx = ay * bz - az * by;
        float ny = az * bx - ax * bz;
        float nz = ax * by - ay * bx;
        float length = (float) Math.sqrt(nx * nx + ny * ny + nz * nz);
        if (!(length > 1.0e-12f) || Float.isInfinite(length)) {
            return UP;
        }
        return packNormal(nx / length, ny / length, nz / length);
    }

    /**
     * @param x normal x
     * @param y normal y
     * @param z normal z
     * @return the normal as {@code RGBA8_SNORM} bytes, w = 0
     */
    static int packNormal(float x, float y, float z) {
        return snorm(x) | snorm(y) << 8 | snorm(z) << 16;
    }

    private static int snorm(float value) {
        return Math.round(Math.clamp(value, -1.0f, 1.0f) * 127.0f) & 0xFF;
    }

    /**
     * @param u mean texture coordinate u
     * @param v mean texture coordinate v
     * @return the {@code sb_MidTexCoord} value
     */
    public static int midTexCoord(float u, float v) {
        return unorm16(u) | unorm16(v) << 16;
    }

    private static int unorm16(float coordinate) {
        return (int) Math.clamp((long) Math.round(coordinate * MID_TEX_COORD_SCALE), 0L, 0xFFFFL);
    }

    /**
     * @param localX   the block's x within its section (0 to 15)
     * @param localY   the block's y within its section
     * @param localZ   the block's z within its section
     * @param emission the block's light emission (0 to 15)
     * @return the block reference
     */
    public static int block(int localX, int localY, int localZ, int emission) {
        return VALID | (localX & 0xF) | (localY & 0xF) << 4 | (localZ & 0xF) << 8 | Math.clamp(emission, 0, 15) << 12;
    }

    /**
     * @param block a block reference ({@link #block} or {@link #NO_BLOCK})
     * @param x     the vertex's section-local x
     * @param y     the vertex's section-local y
     * @param z     the vertex's section-local z
     * @return the {@code sb_MidBlock} value of the vertex
     */
    public static int midBlock(int block, float x, float y, float z) {
        if ((block & VALID) == 0) {
            return 0;
        }
        int dx = offset((block & 0xF) + 0.5f - x);
        int dy = offset((block >>> 4 & 0xF) + 0.5f - y);
        int dz = offset((block >>> 8 & 0xF) + 0.5f - z);
        int emission = block >>> 12 & 0xF;
        return (dx & 0xFF) | (dy & 0xFF) << 8 | (dz & 0xFF) << 16 | emission << 24;
    }

    private static int offset(float blocks) {
        float scaled = blocks * MID_BLOCK_SCALE;
        if (Float.isNaN(scaled)) {
            return 0;
        }
        return Math.round(Math.clamp(scaled, -127.0f, 127.0f));
    }
}
