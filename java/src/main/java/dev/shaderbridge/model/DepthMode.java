package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/** Depth convention translated shaders are generated for (ARCHITECTURE §4). */
public enum DepthMode implements WireEnum {
    /** NDC z in [0,1], near maps to 0. Host uses LESS/LEQUAL and clears to 1. */
    FORWARD_ZERO_TO_ONE,
    /** NDC z in [0,1], near maps to 1 (Minecraft 26.2+, DH 3.3+). Host uses GEQUAL and clears to 0. */
    REVERSED_ZERO_TO_ONE,
    /** GL default clip control, NDC z in [-1,1]; no remap. */
    GL_NEG_ONE_TO_ONE
}
