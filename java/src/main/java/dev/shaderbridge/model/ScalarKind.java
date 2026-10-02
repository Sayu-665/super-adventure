package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/** Scalar component type of a {@link GlslType}. */
public enum ScalarKind implements WireEnum {
    /** 32-bit float. */
    FLOAT,
    /** 32-bit signed integer. */
    INT,
    /** 32-bit unsigned integer. */
    UINT,
    /** Boolean, stored as a 32-bit integer in std140 blocks. */
    BOOL,
    /** 64-bit float. */
    DOUBLE;

    /** @return the size of one component in a std140 block, in bytes */
    public int byteSize() {
        return this == DOUBLE ? 8 : 4;
    }
}
