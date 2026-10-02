package dev.shaderbridge.model;

/**
 * The pack-global uniform blocks (ARCHITECTURE §5.1).
 *
 * @param frame {@code sb_Frame} (set 0, binding 0): per-frame values
 * @param draw  {@code sb_Draw} (set 0, binding 1): per-draw values
 */
public record UniformLayout(BlockLayout frame, BlockLayout draw) {
    public UniformLayout {
        Copies.required(frame, "frame");
        Copies.required(draw, "draw");
    }
}
