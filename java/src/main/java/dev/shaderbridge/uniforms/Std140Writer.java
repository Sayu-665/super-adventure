package dev.shaderbridge.uniforms;

import dev.shaderbridge.model.GlslType;
import dev.shaderbridge.model.ScalarKind;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.List;
import org.joml.Matrix3fc;
import org.joml.Matrix4fc;

/**
 * Writes values into a std140 uniform block at explicit member offsets, little-endian, with the
 * same layout rules as {@code sb_core::GlslType}: vectors of 3 and 4 components align to 16
 * bytes, every matrix column (and every array element) occupies a multiple of 16 bytes, and
 * {@code bool} is a 4-byte integer.
 */
public final class Std140Writer {
    private final ByteBuffer buffer;

    /** @param buffer the block storage; written with absolute puts, its position is ignored */
    public Std140Writer(ByteBuffer buffer) {
        this.buffer = buffer.duplicate().order(ByteOrder.LITTLE_ENDIAN);
    }

    /** @return the block storage (little-endian view) */
    public ByteBuffer buffer() {
        return buffer;
    }

    // ---------------------------------------------------------------------------------------
    // Layout
    // ---------------------------------------------------------------------------------------

    /**
     * @param type a GLSL type
     * @return its std140 base alignment in bytes
     */
    public static int alignment(GlslType type) {
        int element = type.isMatrix() ? columnStride(type) : vectorAlignment(type.scalar().byteSize(), type.rows());
        return type.array() != null ? roundUp(element, 16) : element;
    }

    /**
     * @param type a GLSL type
     * @return its std140 size in bytes (stride times length for arrays), saturating at
     *     {@link Integer#MAX_VALUE} for absurd array lengths, as {@code sb_core} saturates
     */
    public static int size(GlslType type) {
        if (type.array() == null) {
            return elementSize(type);
        }
        long size = (long) arrayStride(type) * Math.max(1, type.array());
        return (int) Math.min(Integer.MAX_VALUE, size);
    }

    /**
     * @param type a GLSL type
     * @return the distance between array elements: the element size rounded up to 16
     */
    public static int arrayStride(GlslType type) {
        return roundUp(Math.max(elementSize(type), alignment(type.element())), 16);
    }

    /**
     * @param type a matrix type
     * @return the distance between matrix columns: a column vector rounded up to 16
     */
    public static int columnStride(GlslType type) {
        return roundUp(vectorAlignment(type.scalar().byteSize(), type.rows()), 16);
    }

    private static int elementSize(GlslType type) {
        return type.isMatrix() ? columnStride(type) * type.cols() : type.scalar().byteSize() * type.rows();
    }

    private static int vectorAlignment(int scalarSize, int rows) {
        return switch (rows) {
            case 1 -> scalarSize;
            case 2 -> 2 * scalarSize;
            default -> 4 * scalarSize;
        };
    }

    private static int roundUp(int value, int alignment) {
        return (value + alignment - 1) / alignment * alignment;
    }

    // ---------------------------------------------------------------------------------------
    // Scalars
    // ---------------------------------------------------------------------------------------

    /**
     * Writes one component converted to the given scalar kind (float to int truncates, any
     * non-zero value is {@code true}).
     *
     * @param offset byte offset
     * @param kind   destination component type
     * @param value  the value
     */
    public void putComponent(int offset, ScalarKind kind, double value) {
        switch (kind) {
            case FLOAT -> buffer.putFloat(offset, (float) value);
            case INT, UINT -> buffer.putInt(offset, (int) (long) value);
            case BOOL -> buffer.putInt(offset, value != 0.0 ? 1 : 0);
            case DOUBLE -> buffer.putDouble(offset, value);
        }
    }

    /**
     * Writes one integer component converted to the given scalar kind.
     *
     * @param offset byte offset
     * @param kind   destination component type
     * @param value  the value
     */
    public void putComponent(int offset, ScalarKind kind, int value) {
        switch (kind) {
            case FLOAT -> buffer.putFloat(offset, value);
            case INT, UINT -> buffer.putInt(offset, value);
            case BOOL -> buffer.putInt(offset, value != 0 ? 1 : 0);
            case DOUBLE -> buffer.putDouble(offset, value);
        }
    }

    // ---------------------------------------------------------------------------------------
    // Matrices
    // ---------------------------------------------------------------------------------------

    /**
     * Writes a matrix column by column, padding every column to {@link #columnStride}. A smaller
     * declared matrix receives the upper-left part.
     *
     * @param offset byte offset of the member
     * @param type   declared matrix type
     * @param m      the value
     */
    public void putMatrix(int offset, GlslType type, Matrix4fc m) {
        int stride = columnStride(type);
        int size = type.scalar().byteSize();
        for (int c = 0; c < Math.min(4, type.cols()); c++) {
            for (int r = 0; r < Math.min(4, type.rows()); r++) {
                putComponent(offset + c * stride + r * size, type.scalar(), m.get(c, r));
            }
        }
    }

    /**
     * Writes a 3x3 matrix, padding every column to {@link #columnStride}.
     *
     * @param offset byte offset of the member
     * @param type   declared matrix type
     * @param m      the value
     */
    public void putMatrix(int offset, GlslType type, Matrix3fc m) {
        int stride = columnStride(type);
        int size = type.scalar().byteSize();
        for (int c = 0; c < Math.min(3, type.cols()); c++) {
            for (int r = 0; r < Math.min(3, type.rows()); r++) {
                putComponent(offset + c * stride + r * size, type.scalar(), m.get(c, r));
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Constant initializers
    // ---------------------------------------------------------------------------------------

    /**
     * Writes a {@code uniform T x = init;} default: unpadded components in GLSL constructor order
     * (column-major for matrices, element after element for arrays) are spread to their std140
     * positions.
     *
     * @param offset byte offset of the member
     * @param type   declared type
     * @param values {@code rows * cols * arrayLength} component values
     */
    public void putDefault(int offset, GlslType type, List<Float> values) {
        GlslType element = type.element();
        int perElement = element.rows() * element.cols();
        int stride = type.array() != null ? arrayStride(type) : 0;
        int columnStride = element.isMatrix() ? columnStride(element) : 0;
        int scalar = element.scalar().byteSize();
        long components = Math.min(values.size(), (long) perElement * type.arrayLength());
        for (int i = 0; i < components; i++) {
            int e = i / perElement;
            int column = (i % perElement) / element.rows();
            int row = (i % perElement) % element.rows();
            // serde writes a non-finite f32 as null.
            Float value = values.get(i);
            putComponent(offset + e * stride + column * columnStride + row * scalar, element.scalar(), value == null ? Double.NaN : value);
        }
    }
}
