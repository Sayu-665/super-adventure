package dev.shaderbridge.render.frame;

import dev.shaderbridge.model.BlendFactor;
import dev.shaderbridge.model.BlendMode;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.uniforms.DrawState;
import dev.shaderbridge.uniforms.FrameState;
import java.util.List;

/**
 * The {@code sb_Draw} values of one kind of draw: what Iris would set per draw that ShaderBridge
 * can know from the pipeline alone. Entity, block entity and item ids are not known per draw
 * (Minecraft batches feature draws) and stay {@code -1}.
 *
 * @param pipeline     identity of the pack pipeline (distinguishes the keys of different programs)
 * @param renderStage  {@code renderStage} ({@link RenderStages})
 * @param shadow       a shadow-pass draw: the shadow model-view and projection
 * @param alphaTestRef {@code alphaTestRef} of the program
 * @param blendFunc    {@code blendFunc} as GL enums (srcRGB, dstRGB, srcAlpha, dstAlpha), zeros without blending
 */
public record DrawKey(String pipeline, int renderStage, boolean shadow, float alphaTestRef, List<Integer> blendFunc) {
    public DrawKey {
        blendFunc = List.copyOf(blendFunc);
    }

    /**
     * @param pipeline    identity of the pack pipeline
     * @param program     the program it runs
     * @param renderStage the draw's render stage
     * @param shadow      a shadow-pass draw
     * @return the key
     */
    public static DrawKey of(String pipeline, Program program, int renderStage, boolean shadow) {
        float alpha = program.alphaTest() == null ? 0 : program.alphaTest().reference();
        return new DrawKey(pipeline, renderStage, shadow, alpha, blendFunc(program.blend()));
    }

    /**
     * Writes the key's values into a draw state.
     *
     * @param frame the frame
     * @param draw  destination; reset to the frame's defaults first
     */
    public void apply(FrameState frame, DrawState draw) {
        draw.reset(frame);
        if (shadow) {
            draw.modelViewMatrix.set(frame.shadowModelView());
            draw.projectionMatrix.set(frame.shadowProjection());
        }
        draw.renderStage = renderStage;
        draw.alphaTestRef = alphaTestRef;
        for (int i = 0; i < 4; i++) {
            draw.blendFunc[i] = blendFunc.get(i);
        }
    }

    /**
     * @param blend a program's blend mode, or null
     * @return its factors as GL enums, zeros when blending is off
     */
    static List<Integer> blendFunc(BlendMode blend) {
        if (blend == null) {
            return List.of(0, 0, 0, 0);
        }
        return List.of(gl(blend.srcColor()), gl(blend.dstColor()), gl(blend.srcAlpha()), gl(blend.dstAlpha()));
    }

    private static int gl(BlendFactor f) {
        return switch (f) {
            case ZERO -> 0;
            case ONE -> 1;
            case SRC_COLOR -> 0x0300;
            case ONE_MINUS_SRC_COLOR -> 0x0301;
            case SRC_ALPHA -> 0x0302;
            case ONE_MINUS_SRC_ALPHA -> 0x0303;
            case DST_ALPHA -> 0x0304;
            case ONE_MINUS_DST_ALPHA -> 0x0305;
            case DST_COLOR -> 0x0306;
            case ONE_MINUS_DST_COLOR -> 0x0307;
            case SRC_ALPHA_SATURATE -> 0x0308;
        };
    }
}
