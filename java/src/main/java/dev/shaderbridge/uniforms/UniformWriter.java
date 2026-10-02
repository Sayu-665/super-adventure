package dev.shaderbridge.uniforms;

import dev.shaderbridge.model.GlslType;
import org.joml.Matrix3fc;
import org.joml.Matrix4fc;
import org.joml.Vector3dc;
import org.joml.Vector3fc;
import org.joml.Vector4fc;

/**
 * The destination of one builtin value: a block member with its declared type. Providers write
 * the value in the builtin's natural type and the writer converts it to the declared type the way
 * GL's {@code glUniform*} would: {@code int} and {@code bool} scalars are interchangeable, extra
 * components are dropped, missing ones stay zero. Builtins are never arrays; an array declaration
 * receives the value in its first element.
 */
public final class UniformWriter {
    private Std140Writer target;
    private int offset;
    private GlslType type;

    /**
     * Points the writer at a member.
     *
     * @param target the block
     * @param offset member offset in bytes
     * @param type   declared member type
     * @return this writer
     */
    public UniformWriter bind(Std140Writer target, int offset, GlslType type) {
        this.target = target;
        this.offset = offset;
        this.type = type.element();
        return this;
    }

    /** @param value a {@code float} */
    public void putFloat(float value) {
        component(0, value);
    }

    /** @param value an {@code int} */
    public void putInt(int value) {
        intComponent(0, value);
    }

    /** @param value a {@code bool} */
    public void putBool(boolean value) {
        intComponent(0, value ? 1 : 0);
    }

    /**
     * @param x first component
     * @param y second component
     */
    public void putVec2(float x, float y) {
        component(0, x);
        component(1, y);
    }

    /**
     * @param x first component
     * @param y second component
     * @param z third component
     */
    public void putVec3(double x, double y, double z) {
        component(0, x);
        component(1, y);
        component(2, z);
    }

    /** @param v a {@code vec3} */
    public void putVec3(Vector3fc v) {
        putVec3(v.x(), v.y(), v.z());
    }

    /** @param v a {@code vec3} given in double precision (written as float) */
    public void putVec3(Vector3dc v) {
        putVec3(v.x(), v.y(), v.z());
    }

    /** @param v a {@code vec4} */
    public void putVec4(Vector4fc v) {
        putVec4(v.x(), v.y(), v.z(), v.w());
    }

    /**
     * @param x first component
     * @param y second component
     * @param z third component
     * @param w fourth component
     */
    public void putVec4(float x, float y, float z, float w) {
        component(0, x);
        component(1, y);
        component(2, z);
        component(3, w);
    }

    /**
     * @param x first component
     * @param y second component
     */
    public void putIvec2(int x, int y) {
        intComponent(0, x);
        intComponent(1, y);
    }

    /**
     * @param x first component
     * @param y second component
     * @param z third component
     */
    public void putIvec3(int x, int y, int z) {
        intComponent(0, x);
        intComponent(1, y);
        intComponent(2, z);
    }

    /**
     * @param x first component
     * @param y second component
     * @param z third component
     * @param w fourth component
     */
    public void putIvec4(int x, int y, int z, int w) {
        intComponent(0, x);
        intComponent(1, y);
        intComponent(2, z);
        intComponent(3, w);
    }

    /** @param m a {@code mat4} */
    public void putMat4(Matrix4fc m) {
        if (type.isMatrix()) {
            target.putMatrix(offset, type, m);
        }
    }

    /** @param m a {@code mat3} */
    public void putMat3(Matrix3fc m) {
        if (type.isMatrix()) {
            target.putMatrix(offset, type, m);
        }
    }

    private void component(int index, double value) {
        if (!type.isMatrix() && index < type.rows()) {
            target.putComponent(offset + index * type.scalar().byteSize(), type.scalar(), value);
        }
    }

    private void intComponent(int index, int value) {
        if (!type.isMatrix() && index < type.rows()) {
            target.putComponent(offset + index * type.scalar().byteSize(), type.scalar(), value);
        }
    }
}
