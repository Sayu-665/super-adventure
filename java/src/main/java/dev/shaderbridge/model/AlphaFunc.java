package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/** Alpha test comparison ({@code alphaTest.<prog>}). The test passes when {@code alpha <op> reference}. */
public enum AlphaFunc implements WireEnum {
    /** Never passes. */
    NEVER,
    /** {@code <}. */
    LESS,
    /** {@code ==}. */
    EQUAL,
    /** {@code <=} (serde {@code LEqual}). */
    L_EQUAL,
    /** {@code >}. */
    GREATER,
    /** {@code !=}. */
    NOT_EQUAL,
    /** {@code >=} (serde {@code GEqual}). */
    G_EQUAL,
    /** Always passes. */
    ALWAYS
}
