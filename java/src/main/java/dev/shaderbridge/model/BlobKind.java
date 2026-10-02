package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/** Payload kind of a blob ({@code sb_core::model::BlobKind}). */
public enum BlobKind implements WireEnum {
    /** SPIR-V words, little-endian. */
    SPIRV,
    /** UTF-8 GLSL source. */
    GLSL,
    /** Raw bytes, e.g. custom texture payloads. */
    BYTES
}
