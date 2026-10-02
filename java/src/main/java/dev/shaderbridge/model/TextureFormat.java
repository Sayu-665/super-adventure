package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/**
 * Texture formats in Iris/OptiFine spelling ({@code sb_core::format::TextureFormat}). The wire
 * name is the constant name itself, e.g. {@code RGBA16F}.
 */
public enum TextureFormat implements WireEnum {
    RGBA,
    R8,
    RG8,
    RGB8,
    RGBA8,
    R8_SNORM,
    RG8_SNORM,
    RGB8_SNORM,
    RGBA8_SNORM,
    R16,
    RG16,
    RGB16,
    RGBA16,
    R16_SNORM,
    RG16_SNORM,
    RGB16_SNORM,
    RGBA16_SNORM,
    R16F,
    RG16F,
    RGB16F,
    RGBA16F,
    R32F,
    RG32F,
    RGB32F,
    RGBA32F,
    R8I,
    RG8I,
    RGB8I,
    RGBA8I,
    R8UI,
    RG8UI,
    RGB8UI,
    RGBA8UI,
    R16I,
    RG16I,
    RGB16I,
    RGBA16I,
    R16UI,
    RG16UI,
    RGB16UI,
    RGBA16UI,
    R32I,
    RG32I,
    RGB32I,
    RGBA32I,
    R32UI,
    RG32UI,
    RGB32UI,
    RGBA32UI,
    RGBA2,
    RGBA4,
    R3_G3_B2,
    RGB5_A1,
    RGB565,
    RGB10_A2,
    RGB10_A2UI,
    R11F_G11F_B10F,
    RGB9_E5;

    @Override
    public String wireName() {
        return name();
    }
}
