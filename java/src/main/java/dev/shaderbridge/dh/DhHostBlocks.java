package dev.shaderbridge.dh;

import dev.shaderbridge.model.GlslType;
import dev.shaderbridge.model.ScalarKind;
import dev.shaderbridge.uniforms.Std140Writer;
import java.util.ArrayList;
import java.util.List;
import org.joml.Matrix4fc;

/**
 * The Distant Horizons host uniform blocks of the {@code dh_terrain} draw profile
 * ({@code crates/sb-transform/profiles/dh_terrain.toml}), which ShaderBridge fills itself because
 * it draws the LODs instead of Distant Horizons. They use a camera-relative convention for
 * precision far from the origin: {@code uModelOffset} is the buffer's minimum corner minus the
 * camera position, computed in double precision, and {@code uCameraPos} is zero (the profile
 * positions vertices at {@code vPosition + uModelOffset - uCameraPos}). Distant Horizons itself
 * stores absolute float coordinates there, which lose a block of precision 16 million blocks out.
 */
public final class DhHostBlocks {
    /** The per-buffer block. */
    public static final String UNIQUE_BLOCK = "vertUniqueUniformBlock";
    /** The per-pass block. */
    public static final String SHARED_BLOCK = "vertSharedUniformBlock";
    /** {@code uMircoOffset}: the size of Distant Horizons' micro offsets in blocks. */
    static final float MICRO_OFFSET = 0.01f;
    /** Earth radius in blocks at a curvature ratio of 1 ({@code BlazeDhTerrainRenderer}). */
    static final float EARTH_RADIUS = 6_371_000f;

    /**
     * One member of a host block.
     *
     * @param name   GLSL name
     * @param type   GLSL type
     * @param offset std140 byte offset
     */
    public record Member(String name, GlslType type, int offset) {
    }

    /** {@code vertUniqueUniformBlock}, in declaration order. */
    public static final List<Member> UNIQUE = layout(new String[] {"uModelOffset"}, new GlslType[] {GlslType.VEC3});
    /** {@code vertSharedUniformBlock}, in declaration order. */
    public static final List<Member> SHARED = layout(
        new String[] {"uIsWhiteWorld", "uWorldYOffset", "uMircoOffset", "uEarthRadius", "uFrameMod8", "uViewWidth", "uViewHeight", "uCameraPos",
            "uCombinedMatrix"},
        new GlslType[] {GlslType.BOOL, GlslType.FLOAT, GlslType.FLOAT, GlslType.FLOAT, GlslType.FLOAT, GlslType.FLOAT, GlslType.FLOAT, GlslType.VEC3,
            GlslType.MAT4});
    /** Size of {@code vertUniqueUniformBlock} in bytes (std140, rounded to 16). */
    public static final int UNIQUE_SIZE = blockSize(UNIQUE);
    /** Size of {@code vertSharedUniformBlock} in bytes (std140, rounded to 16). */
    public static final int SHARED_SIZE = blockSize(SHARED);

    /**
     * The values of {@code vertSharedUniformBlock} for one pass.
     *
     * @param worldYOffset the level's minimum Y ({@code uWorldYOffset})
     * @param earthRadius  {@link #earthRadius} of Distant Horizons' curvature setting
     * @param frameMod8    {@code uFrameMod8}
     * @param viewWidth    width of the render target
     * @param viewHeight   height of the render target
     * @param combined     {@code uCombinedMatrix}: projection times model-view of the pass, for camera-relative positions
     */
    public record Shared(float worldYOffset, float earthRadius, float frameMod8, float viewWidth, float viewHeight, Matrix4fc combined) {
    }

    private DhHostBlocks() {
    }

    /**
     * Writes {@code vertSharedUniformBlock}; {@code uIsWhiteWorld} is false and {@code uCameraPos}
     * is zero.
     *
     * @param out    destination
     * @param base   byte offset of the block in {@code out}
     * @param values the pass's values
     */
    public static void writeShared(Std140Writer out, int base, Shared values) {
        float[] scalars = {0, values.worldYOffset(), MICRO_OFFSET, values.earthRadius(), values.frameMod8(), values.viewWidth(), values.viewHeight()};
        for (int i = 0; i < scalars.length; i++) {
            Member m = SHARED.get(i);
            out.putComponent(base + m.offset(), m.type().scalar(), scalars[i]);
        }
        Member camera = SHARED.get(7);
        for (int c = 0; c < 3; c++) {
            out.putComponent(base + camera.offset() + c * 4, ScalarKind.FLOAT, 0.0);
        }
        Member combined = SHARED.get(8);
        out.putMatrix(base + combined.offset(), combined.type(), values.combined());
    }

    /**
     * Writes {@code vertUniqueUniformBlock} of one LOD buffer.
     *
     * @param out     destination
     * @param base    byte offset of the block in {@code out}
     * @param offset  {@link #modelOffset} of the buffer
     */
    public static void writeUnique(Std140Writer out, int base, float[] offset) {
        Member m = UNIQUE.getFirst();
        for (int c = 0; c < 3; c++) {
            out.putComponent(base + m.offset() + c * 4, ScalarKind.FLOAT, offset[c]);
        }
    }

    /**
     * {@code uModelOffset} of a LOD buffer: its minimum corner relative to the camera, subtracted
     * in double precision so that the float result keeps sub-block precision at any distance from
     * the world origin.
     *
     * @param minX   minimum corner X (block)
     * @param minY   minimum corner Y (block)
     * @param minZ   minimum corner Z (block)
     * @param cameraX camera X
     * @param cameraY camera Y
     * @param cameraZ camera Z
     * @return the offset
     */
    public static float[] modelOffset(int minX, int minY, int minZ, double cameraX, double cameraY, double cameraZ) {
        return new float[] {(float) (minX - cameraX), (float) (minY - cameraY), (float) (minZ - cameraZ)};
    }

    /**
     * {@code uEarthRadius} as Distant Horizons derives it from its {@code earthCurvatureRatio}
     * setting: 0 (no curvature) for ratios in [-1, 1], else the earth's radius divided by the ratio.
     *
     * @param curvatureRatio the setting
     * @return the radius in blocks, or 0
     */
    public static float earthRadius(int curvatureRatio) {
        return curvatureRatio >= -1 && curvatureRatio <= 1 ? 0 : EARTH_RADIUS / curvatureRatio;
    }

    private static List<Member> layout(String[] names, GlslType[] types) {
        List<Member> out = new ArrayList<>();
        int offset = 0;
        for (int i = 0; i < names.length; i++) {
            int align = Std140Writer.alignment(types[i]);
            offset = (offset + align - 1) / align * align;
            out.add(new Member(names[i], types[i], offset));
            offset += Std140Writer.size(types[i]);
        }
        return List.copyOf(out);
    }

    private static int blockSize(List<Member> members) {
        Member last = members.getLast();
        int end = last.offset() + Std140Writer.size(last.type());
        return (end + 15) / 16 * 16;
    }
}
