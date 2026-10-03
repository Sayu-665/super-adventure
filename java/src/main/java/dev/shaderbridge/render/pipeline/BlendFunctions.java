package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.pipeline.BlendFactor;
import com.mojang.renderpearl.api.pipeline.BlendFunction;
import dev.shaderbridge.model.BlendMode;

/** Pack blend modes ({@code blend.<program>}) as Mojang blend functions (both equations ADD). */
public final class BlendFunctions {
    private BlendFunctions() {
    }

    /**
     * @param mode a pack blend mode
     * @return the equivalent blend function
     */
    public static BlendFunction of(BlendMode mode) {
        return new BlendFunction(factor(mode.srcColor()), factor(mode.dstColor()), factor(mode.srcAlpha()), factor(mode.dstAlpha()));
    }

    static BlendFactor factor(dev.shaderbridge.model.BlendFactor factor) {
        return switch (factor) {
            case ZERO -> BlendFactor.ZERO;
            case ONE -> BlendFactor.ONE;
            case SRC_COLOR -> BlendFactor.SRC_COLOR;
            case ONE_MINUS_SRC_COLOR -> BlendFactor.ONE_MINUS_SRC_COLOR;
            case DST_COLOR -> BlendFactor.DST_COLOR;
            case ONE_MINUS_DST_COLOR -> BlendFactor.ONE_MINUS_DST_COLOR;
            case SRC_ALPHA -> BlendFactor.SRC_ALPHA;
            case ONE_MINUS_SRC_ALPHA -> BlendFactor.ONE_MINUS_SRC_ALPHA;
            case DST_ALPHA -> BlendFactor.DST_ALPHA;
            case ONE_MINUS_DST_ALPHA -> BlendFactor.ONE_MINUS_DST_ALPHA;
            case SRC_ALPHA_SATURATE -> BlendFactor.SRC_ALPHA_SATURATE;
        };
    }
}
