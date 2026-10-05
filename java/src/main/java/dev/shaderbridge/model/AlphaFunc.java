package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;
import java.util.Optional;

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
    ALWAYS;

    /**
     * A reference every finite alpha passes with this comparison: how a host turns off a compiled
     * test per draw (as {@code sb_core::program::AlphaFunc::always_passing_reference}).
     *
     * @return the reference, empty for comparisons no reference always passes
     */
    public Optional<Float> alwaysPassingReference() {
        return switch (this) {
            case GREATER, G_EQUAL -> Optional.of(-Float.MAX_VALUE);
            case LESS, L_EQUAL -> Optional.of(Float.MAX_VALUE);
            default -> Optional.empty();
        };
    }
}
