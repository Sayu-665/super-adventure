package dev.shaderbridge.model;

import dev.shaderbridge.model.json.OmitIfNull;

/**
 * A non-opaque GLSL type: scalar, vector or matrix, optionally an array.
 *
 * @param scalar component type
 * @param rows   rows, or vector components (1..4)
 * @param cols   columns (1 for scalars and vectors, 2..4 for matrices)
 * @param array  array length, or null if not an array
 */
public record GlslType(ScalarKind scalar, int rows, int cols, @OmitIfNull Integer array) {
    /** {@code float}. */
    public static final GlslType FLOAT = new GlslType(ScalarKind.FLOAT, 1, 1, null);
    /** {@code int}. */
    public static final GlslType INT = new GlslType(ScalarKind.INT, 1, 1, null);
    /** {@code bool}. */
    public static final GlslType BOOL = new GlslType(ScalarKind.BOOL, 1, 1, null);
    /** {@code vec3}. */
    public static final GlslType VEC3 = new GlslType(ScalarKind.FLOAT, 3, 1, null);
    /** {@code vec4}. */
    public static final GlslType VEC4 = new GlslType(ScalarKind.FLOAT, 4, 1, null);
    /** {@code mat4}. */
    public static final GlslType MAT4 = new GlslType(ScalarKind.FLOAT, 4, 4, null);

    public GlslType {
        Copies.required(scalar, "scalar");
        if (rows < 1 || rows > 4 || cols < 1 || cols > 4) {
            throw new IllegalArgumentException("invalid GLSL type shape " + cols + "x" + rows);
        }
        if (array != null && array < 0) {
            throw new IllegalArgumentException("negative array length " + array);
        }
    }

    /**
     * @param kind component type
     * @param n    component count (1..4)
     * @return the scalar ({@code n == 1}) or vector type
     */
    public static GlslType vector(ScalarKind kind, int n) {
        return new GlslType(kind, n, 1, null);
    }

    /**
     * @param cols columns
     * @param rows rows
     * @return the float matrix type {@code mat<cols>x<rows>}
     */
    public static GlslType matrix(int cols, int rows) {
        return new GlslType(ScalarKind.FLOAT, rows, cols, null);
    }

    /**
     * @param length array length
     * @return this type as an array
     */
    public GlslType withArray(int length) {
        return new GlslType(scalar, rows, cols, length);
    }

    /** @return the element type of an array (this type without the array) */
    public GlslType element() {
        return array == null ? this : new GlslType(scalar, rows, cols, null);
    }

    /** @return true for matrices */
    public boolean isMatrix() {
        return cols > 1;
    }

    /** @return the number of array elements (1 for non-arrays) */
    public int arrayLength() {
        return array == null ? 1 : array;
    }

    /** @return GLSL spelling of the element type, e.g. {@code vec3}, {@code mat3x4} */
    public String glslName() {
        if (cols > 1) {
            String prefix = scalar == ScalarKind.DOUBLE ? "dmat" : "mat";
            return cols == rows ? prefix + cols : prefix + cols + "x" + rows;
        }
        if (rows == 1) {
            return scalar.wireName();
        }
        String prefix = switch (scalar) {
            case FLOAT -> "vec";
            case INT -> "ivec";
            case UINT -> "uvec";
            case BOOL -> "bvec";
            case DOUBLE -> "dvec";
        };
        return prefix + rows;
    }

    @Override
    public String toString() {
        return array == null ? glslName() : glslName() + "[" + array + "]";
    }
}
