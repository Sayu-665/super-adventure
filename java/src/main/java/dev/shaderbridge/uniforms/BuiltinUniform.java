package dev.shaderbridge.uniforms;

import dev.shaderbridge.model.GlslType;

/**
 * A builtin uniform of the sb-uniforms registry with the code that computes its value.
 *
 * @param name     GLSL name, exactly as packs declare it
 * @param type     registry type; block members sourced from it may declare a compatible type
 * @param perDraw  the value lives in {@code sb_Draw} rather than {@code sb_Frame}
 * @param provider computes and writes the value
 */
public record BuiltinUniform(String name, GlslType type, boolean perDraw, Provider provider) {
    /** Computes a builtin value from the frame and draw state. */
    @FunctionalInterface
    public interface Provider {
        /**
         * @param frame the current frame
         * @param draw  the current draw (defaults outside draws)
         * @param out   the destination member
         */
        void write(FrameState frame, DrawState draw, UniformWriter out);
    }
}
