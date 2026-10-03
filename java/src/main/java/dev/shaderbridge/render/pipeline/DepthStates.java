package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.pipeline.CompareOp;
import com.mojang.renderpearl.api.pipeline.DepthStencilState;
import dev.shaderbridge.model.DepthMode;

/**
 * Depth test state per {@link DepthMode} (ARCHITECTURE §4). Minecraft 26.3 renders reversed-Z
 * ({@code GEQUAL}, cleared to 0): with {@link DepthMode#REVERSED_ZERO_TO_ONE} pack pipelines keep
 * the vanilla depth state; the forward modes mirror every ordering comparison and depth bias.
 */
public final class DepthStates {
    private DepthStates() {
    }

    /**
     * @param mode a depth convention
     * @return whether near maps to 1
     */
    public static boolean reversed(DepthMode mode) {
        return mode == DepthMode.REVERSED_ZERO_TO_ONE;
    }

    /**
     * @param mode a depth convention
     * @return the value depth targets are cleared to (the far plane)
     */
    public static double clearValue(DepthMode mode) {
        return reversed(mode) ? 0.0 : 1.0;
    }

    /**
     * Depth state of pack geometry drawn without a vanilla pipeline to follow (shadow pass, LODs):
     * test nearer-or-equal and write.
     *
     * @param mode the pack's depth convention
     * @return the state
     */
    public static DepthStencilState standard(DepthMode mode) {
        return new DepthStencilState(reversed(mode) ? CompareOp.GREATER_THAN_OR_EQUAL : CompareOp.LESS_THAN_OR_EQUAL, true);
    }

    /**
     * Adapts the depth state of a vanilla (reversed-Z) pipeline to the pack's convention.
     *
     * @param mode    the pack's depth convention
     * @param vanilla the vanilla pipeline's state, or null for no depth test
     * @return the state for the pack pipeline, or null for no depth test
     */
    public static DepthStencilState forPack(DepthMode mode, DepthStencilState vanilla) {
        if (vanilla == null || reversed(mode)) {
            return vanilla;
        }
        return new DepthStencilState(mirror(vanilla.depthTest()), vanilla.writeDepth(), -vanilla.depthBiasScaleFactor(),
            -vanilla.depthBiasConstant());
    }

    /**
     * @param op a comparison
     * @return the comparison with its ordering reversed (equality and constant tests unchanged)
     */
    static CompareOp mirror(CompareOp op) {
        return switch (op) {
            case LESS_THAN -> CompareOp.GREATER_THAN;
            case LESS_THAN_OR_EQUAL -> CompareOp.GREATER_THAN_OR_EQUAL;
            case GREATER_THAN -> CompareOp.LESS_THAN;
            case GREATER_THAN_OR_EQUAL -> CompareOp.LESS_THAN_OR_EQUAL;
            case ALWAYS_PASS, EQUAL, NOT_EQUAL, NEVER_PASS -> op;
        };
    }
}
