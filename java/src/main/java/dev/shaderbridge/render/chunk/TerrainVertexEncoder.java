package dev.shaderbridge.render.chunk;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;

/**
 * Fills the extension attributes of the extended chunk vertex ({@link TerrainVertexFormat}) once a
 * mesh is built: per quad, the face normal and tangent from the quad's geometry and atlas
 * coordinates and the centre of its atlas coordinates; per vertex, the block id and render type of
 * the block that emitted the quad and the offset from the vertex to that block's centre.
 *
 * <p>The encodings are those shader packs see under Iris (OptiFine's attributes):
 *
 * <ul>
 *   <li>{@code gl_Normal}: the normalized cross product of the quad's diagonals (counter-clockwise
 *   quads face their viewer, as Minecraft emits them), as signed normalized bytes;</li>
 *   <li>{@code at_tangent}: the direction in which the atlas {@code u} coordinate grows across the
 *   quad, orthogonalized against the normal; {@code w} is +1 when the direction in which {@code v}
 *   grows is {@code cross(tangent, normal)}, -1 when it is the opposite (mirrored mapping), so packs
 *   get the bitangent as {@code cross(at_tangent.xyz, gl_Normal) * at_tangent.w};</li>
 *   <li>{@code mc_midTexCoord}: the average of the quad's four atlas coordinates;</li>
 *   <li>{@code mc_Entity}: {@code (block id, render type)} with the id from the pack's
 *   {@code block.properties} (-1 when unmapped) and render type 0 for block models, 1 for fluids,
 *   as Iris encodes terrain;</li>
 *   <li>{@code at_midBlock}: {@code (block centre - vertex) * 64} per axis (1/64 block units) and the
 *   block's light emission (0..15) in {@code w}.</li>
 * </ul>
 *
 * Quads whose atlas mapping is degenerate get the tangent of a cube face with vanilla texture
 * orientation instead. Stateless and thread-safe.
 */
public final class TerrainVertexEncoder {
    /** {@code mc_Entity.y} of block model quads. */
    public static final int RENDER_TYPE_BLOCK = 0;
    /** {@code mc_Entity.y} of fluid quads. */
    public static final int RENDER_TYPE_FLUID = 1;
    /** {@code mc_Entity.x} of unmapped blocks, and {@code mc_Entity.y} of quads emitted outside any block. */
    public static final int NONE = -1;
    /** Bit of a quad record set when the record names a block (whose centre {@code at_midBlock} points to). */
    private static final long HAS_BLOCK = 1L << 48;
    /** The record of a quad emitted outside any block: no id, no render type, no block centre. */
    public static final long NO_BLOCK = 0xFFFFL | 0xFFFFL << 16;

    /** Vertices per quad. */
    private static final int QUAD = 4;

    /**
     * Byte offsets of the elements the encoder reads ({@code Position}, {@code UV0}) and writes in
     * one vertex.
     *
     * @param stride      bytes per vertex
     * @param position    {@code Position}, three floats
     * @param uv0         {@code UV0}, two floats
     * @param normal      {@code sb_Normal}, four signed normalized bytes
     * @param entity      {@code sb_Entity}, two shorts
     * @param midTexCoord {@code sb_MidTexCoord}, two floats
     * @param tangent     {@code sb_Tangent}, four signed normalized bytes
     * @param midBlock    {@code sb_MidBlock}, four signed bytes
     */
    public record Layout(int stride, int position, int uv0, int normal, int entity, int midTexCoord, int tangent, int midBlock) {
    }

    private TerrainVertexEncoder() {
    }

    // ---------------------------------------------------------------------------------------------
    // Per-quad records
    // ---------------------------------------------------------------------------------------------

    /**
     * Packs what a quad needs from the block that emits it.
     *
     * @param blockId    the block's {@code block.properties} id, -1 if unmapped (kept as a 16-bit
     *                   value, as the {@code RG16_SINT} attribute stores it)
     * @param renderType {@link #RENDER_TYPE_BLOCK} or {@link #RENDER_TYPE_FLUID}
     * @param localX     the block's x within its chunk section, 0..15
     * @param localY     the block's y within its chunk section, 0..15
     * @param localZ     the block's z within its chunk section, 0..15
     * @param emission   the block's light emission, 0..15
     * @return the quad record
     */
    public static long quad(int blockId, int renderType, int localX, int localY, int localZ, int emission) {
        return (blockId & 0xFFFFL) | (renderType & 0xFFFFL) << 16 | (localX & 15L) << 32 | (localY & 15L) << 36 | (localZ & 15L) << 40
            | (Math.clamp(emission, 0, 15) & 15L) << 44 | HAS_BLOCK;
    }

    /** @return the block id of a quad record */
    static short blockId(long quad) {
        return (short) quad;
    }

    /** @return the render type of a quad record */
    static short renderType(long quad) {
        return (short) (quad >>> 16);
    }

    // ---------------------------------------------------------------------------------------------
    // Encoding
    // ---------------------------------------------------------------------------------------------

    /**
     * Writes the extension attributes of a built mesh in place. Vertex {@code i} belongs to quad
     * {@code i / 4}; the vanilla elements are left untouched. Vertices after the last complete quad
     * (never emitted by Minecraft, which meshes quads) get neutral values.
     *
     * @param vertices    the mesh's vertices, starting at the buffer's position (its byte order is
     *                    ignored: vertex data is in native order, as Mojang's builders write it)
     * @param layout      where the elements are
     * @param vertexCount vertices in the mesh
     * @param quads       one record per quad ({@link #quad}, {@link #NO_BLOCK}); quads beyond the
     *                    array length count as {@link #NO_BLOCK}
     */
    public static void encode(ByteBuffer vertices, Layout layout, int vertexCount, long[] quads) {
        ByteBuffer buffer = vertices.duplicate().order(ByteOrder.nativeOrder());
        int origin = vertices.position();
        float[] pos = new float[3 * QUAD];
        float[] uv = new float[2 * QUAD];
        float[] normal = new float[3];
        float[] tangent = new float[4];
        int quadCount = vertexCount / QUAD;
        for (int q = 0; q < quadCount; q++) {
            int base = origin + q * QUAD * layout.stride();
            for (int v = 0; v < QUAD; v++) {
                int at = base + v * layout.stride();
                pos[3 * v] = buffer.getFloat(at + layout.position());
                pos[3 * v + 1] = buffer.getFloat(at + layout.position() + 4);
                pos[3 * v + 2] = buffer.getFloat(at + layout.position() + 8);
                uv[2 * v] = buffer.getFloat(at + layout.uv0());
                uv[2 * v + 1] = buffer.getFloat(at + layout.uv0() + 4);
            }
            faceNormal(pos, normal);
            tangent(pos, uv, normal, tangent);
            float midU = (uv[0] + uv[2] + uv[4] + uv[6]) * 0.25f;
            float midV = (uv[1] + uv[3] + uv[5] + uv[7]) * 0.25f;
            long record = q < quads.length ? quads[q] : NO_BLOCK;
            for (int v = 0; v < QUAD; v++) {
                int at = base + v * layout.stride();
                putSnorm4(buffer, at + layout.normal(), normal[0], normal[1], normal[2], 0f);
                buffer.putShort(at + layout.entity(), blockId(record));
                buffer.putShort(at + layout.entity() + 2, renderType(record));
                buffer.putFloat(at + layout.midTexCoord(), midU);
                buffer.putFloat(at + layout.midTexCoord() + 4, midV);
                putSnorm4(buffer, at + layout.tangent(), tangent[0], tangent[1], tangent[2], tangent[3]);
                putMidBlock(buffer, at + layout.midBlock(), record, pos[3 * v], pos[3 * v + 1], pos[3 * v + 2]);
            }
        }
        encodeNeutral(vertices, layout, quadCount * QUAD, vertexCount);
    }

    /**
     * Writes neutral extension attributes (normal up, no block id or render type, the vertex's own
     * atlas coordinates as the centre, tangent +X, no block offset or emission) to a range of
     * vertices that do not form quads.
     *
     * @param vertices the vertices, starting at the buffer's position (native byte order)
     * @param layout   where the elements are
     * @param from     first vertex
     * @param to       end of the range (exclusive)
     */
    public static void encodeNeutral(ByteBuffer vertices, Layout layout, int from, int to) {
        ByteBuffer buffer = vertices.duplicate().order(ByteOrder.nativeOrder());
        int origin = vertices.position();
        for (int i = from; i < to; i++) {
            int at = origin + i * layout.stride();
            putSnorm4(buffer, at + layout.normal(), 0f, 1f, 0f, 0f);
            buffer.putShort(at + layout.entity(), (short) NONE);
            buffer.putShort(at + layout.entity() + 2, (short) NONE);
            buffer.putFloat(at + layout.midTexCoord(), buffer.getFloat(at + layout.uv0()));
            buffer.putFloat(at + layout.midTexCoord() + 4, buffer.getFloat(at + layout.uv0() + 4));
            putSnorm4(buffer, at + layout.tangent(), 1f, 0f, 0f, 1f);
            buffer.putInt(at + layout.midBlock(), 0);
        }
    }

    private static void putSnorm4(ByteBuffer buffer, int at, float x, float y, float z, float w) {
        buffer.put(at, snorm8(x));
        buffer.put(at + 1, snorm8(y));
        buffer.put(at + 2, snorm8(z));
        buffer.put(at + 3, snorm8(w));
    }

    private static void putMidBlock(ByteBuffer buffer, int at, long record, float x, float y, float z) {
        if ((record & HAS_BLOCK) == 0) {
            buffer.putInt(at, 0);
            return;
        }
        buffer.put(at, midBlock((int) (record >>> 32 & 15), x));
        buffer.put(at + 1, midBlock((int) (record >>> 36 & 15), y));
        buffer.put(at + 2, midBlock((int) (record >>> 40 & 15), z));
        buffer.put(at + 3, (byte) (record >>> 44 & 15));
    }

    // ---------------------------------------------------------------------------------------------
    // Math
    // ---------------------------------------------------------------------------------------------

    /**
     * @param value a component in [-1, 1] (clamped)
     * @return its {@code SNORM8} encoding, rounded to the nearest step
     */
    public static byte snorm8(float value) {
        float clamped = Float.isNaN(value) ? 0f : Math.clamp(value, -1f, 1f);
        return (byte) Math.round(clamped * 127f);
    }

    /**
     * @param local  the block's coordinate within its section (0..15)
     * @param vertex the vertex's section-relative coordinate
     * @return {@code (local + 0.5 - vertex) * 64}, rounded and clamped to a signed byte
     */
    public static byte midBlock(int local, float vertex) {
        return (byte) Math.clamp(Math.round((local + 0.5f - vertex) * 64f), -128, 127);
    }

    /**
     * The face normal of a quad: the normalized cross product of its diagonals, which for a
     * counter-clockwise quad points towards its front. A quad whose diagonals are parallel uses its
     * first triangle; a degenerate one faces up.
     *
     * @param pos the four vertices' positions, {@code x, y, z} each
     * @param out receives the normal
     */
    public static void faceNormal(float[] pos, float[] out) {
        float ax = pos[6] - pos[0];
        float ay = pos[7] - pos[1];
        float az = pos[8] - pos[2];
        float bx = pos[9] - pos[3];
        float by = pos[10] - pos[4];
        float bz = pos[11] - pos[5];
        if (!normalized(ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx, out)) {
            ax = pos[3] - pos[0];
            ay = pos[4] - pos[1];
            az = pos[5] - pos[2];
            bx = pos[6] - pos[0];
            by = pos[7] - pos[1];
            bz = pos[8] - pos[2];
            if (!normalized(ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx, out)) {
                out[0] = 0f;
                out[1] = 1f;
                out[2] = 0f;
            }
        }
    }

    /**
     * The tangent of a quad: from its first triangle, or from its second if the first maps no area
     * of the atlas, else the tangent of the cube face closest to the normal with vanilla texture
     * orientation ({@link #axisTangent}).
     *
     * @param pos    the four vertices' positions, {@code x, y, z} each
     * @param uv     the four vertices' atlas coordinates, {@code u, v} each
     * @param normal the quad's unit normal
     * @param out    receives the unit tangent and its handedness ({@code x, y, z, w})
     */
    public static void tangent(float[] pos, float[] uv, float[] normal, float[] out) {
        if (!triangleTangent(pos, uv, normal, 0, 1, 2, out) && !triangleTangent(pos, uv, normal, 2, 3, 0, out)) {
            axisTangent(normal, out);
        }
    }

    /**
     * The tangent of one triangle of a quad. The {@code u} and {@code v} directions solve
     * {@code e1 = du1 * T + dv1 * B}, {@code e2 = du2 * T + dv2 * B} over the triangle's edges; only
     * their directions matter, so the determinant's sign replaces the division by it.
     *
     * @return whether the triangle maps an area of the atlas and its {@code u} direction is not
     *     along the normal
     */
    private static boolean triangleTangent(float[] pos, float[] uv, float[] n, int i0, int i1, int i2, float[] out) {
        float e1x = pos[3 * i1] - pos[3 * i0];
        float e1y = pos[3 * i1 + 1] - pos[3 * i0 + 1];
        float e1z = pos[3 * i1 + 2] - pos[3 * i0 + 2];
        float e2x = pos[3 * i2] - pos[3 * i0];
        float e2y = pos[3 * i2 + 1] - pos[3 * i0 + 1];
        float e2z = pos[3 * i2 + 2] - pos[3 * i0 + 2];
        float du1 = uv[2 * i1] - uv[2 * i0];
        float dv1 = uv[2 * i1 + 1] - uv[2 * i0 + 1];
        float du2 = uv[2 * i2] - uv[2 * i0];
        float dv2 = uv[2 * i2 + 1] - uv[2 * i0 + 1];
        float det = du1 * dv2 - du2 * dv1;
        if (det == 0f || !Float.isFinite(det)) {
            return false;
        }
        float sign = Math.signum(det);
        float tx = sign * (dv2 * e1x - dv1 * e2x);
        float ty = sign * (dv2 * e1y - dv1 * e2y);
        float tz = sign * (dv2 * e1z - dv1 * e2z);
        float bx = sign * (du1 * e2x - du2 * e1x);
        float by = sign * (du1 * e2y - du2 * e1y);
        float bz = sign * (du1 * e2z - du2 * e1z);
        // Gram-Schmidt: keep the tangent in the quad's plane (fluid quads need not be planar).
        float d = tx * n[0] + ty * n[1] + tz * n[2];
        if (!normalized(tx - d * n[0], ty - d * n[1], tz - d * n[2], out)) {
            return false;
        }
        // Handedness: does v grow along cross(tangent, normal) or against it?
        float cx = out[1] * n[2] - out[2] * n[1];
        float cy = out[2] * n[0] - out[0] * n[2];
        float cz = out[0] * n[1] - out[1] * n[0];
        out[3] = bx * cx + by * cy + bz * cz < 0f ? -1f : 1f;
        return true;
    }

    /**
     * The tangent of the axis-aligned cube face closest to a normal, with vanilla texture
     * orientation ({@code u} to the right of a viewer facing the face, {@code v} down on side
     * faces): up, down and south faces +X, north -X, west +Z, east -Z; handedness +1.
     *
     * @param normal a unit normal
     * @param out    receives the tangent, orthogonalized against the normal
     */
    public static void axisTangent(float[] normal, float[] out) {
        float ax = Math.abs(normal[0]);
        float ay = Math.abs(normal[1]);
        float az = Math.abs(normal[2]);
        float tx;
        float tz;
        if (ay >= ax && ay >= az) {
            tx = 1f;
            tz = 0f;
        } else if (az >= ax) {
            tx = normal[2] < 0f ? -1f : 1f;
            tz = 0f;
        } else {
            tx = 0f;
            tz = normal[0] < 0f ? 1f : -1f;
        }
        float d = tx * normal[0] + tz * normal[2];
        if (!normalized(tx - d * normal[0], -d * normal[1], tz - d * normal[2], out)) {
            out[0] = 1f;
            out[1] = 0f;
            out[2] = 0f;
        }
        out[3] = 1f;
    }

    /** Writes the normalized vector to {@code out[0..2]}; false (out untouched) if it has no direction. */
    private static boolean normalized(float x, float y, float z, float[] out) {
        double length = Math.sqrt((double) x * x + (double) y * y + (double) z * z);
        if (!(length > 1e-12) || !Double.isFinite(length)) {
            return false;
        }
        out[0] = (float) (x / length);
        out[1] = (float) (y / length);
        out[2] = (float) (z / length);
        return true;
    }
}
