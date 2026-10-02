package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/** Blend factors packs can express in {@code blend.<prog>}. */
public enum BlendFactor implements WireEnum {
    /** GL_ZERO. */
    ZERO,
    /** GL_ONE. */
    ONE,
    /** GL_SRC_COLOR. */
    SRC_COLOR,
    /** GL_ONE_MINUS_SRC_COLOR. */
    ONE_MINUS_SRC_COLOR,
    /** GL_DST_COLOR. */
    DST_COLOR,
    /** GL_ONE_MINUS_DST_COLOR. */
    ONE_MINUS_DST_COLOR,
    /** GL_SRC_ALPHA. */
    SRC_ALPHA,
    /** GL_ONE_MINUS_SRC_ALPHA. */
    ONE_MINUS_SRC_ALPHA,
    /** GL_DST_ALPHA. */
    DST_ALPHA,
    /** GL_ONE_MINUS_DST_ALPHA. */
    ONE_MINUS_DST_ALPHA,
    /** GL_SRC_ALPHA_SATURATE. */
    SRC_ALPHA_SATURATE
}
