package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/** Shader stages, in pipeline order. */
public enum ShaderStage implements WireEnum {
    /** {@code .vsh}. */
    VERTEX,
    /** {@code .tcs}. */
    TESS_CONTROL,
    /** {@code .tes}. */
    TESS_EVAL,
    /** {@code .gsh}. */
    GEOMETRY,
    /** {@code .fsh}. */
    FRAGMENT,
    /** {@code .csh}. */
    COMPUTE;

    /** @return the pack file extension without the dot: {@code vsh}, {@code tcs}, {@code tes}, {@code gsh}, {@code fsh}, {@code csh} */
    public String packExtension() {
        return switch (this) {
            case VERTEX -> "vsh";
            case TESS_CONTROL -> "tcs";
            case TESS_EVAL -> "tes";
            case GEOMETRY -> "gsh";
            case FRAGMENT -> "fsh";
            case COMPUTE -> "csh";
        };
    }
}
