package dev.shaderbridge.render.frame;

import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.ResourceRef;
import java.util.function.Consumer;

/**
 * Which texture of a ping-ponged target a program samples, with the headless executor's rules
 * ({@code color_read} in {@code sb-runtime/src/descriptors.rs}):
 *
 * <ul>
 *   <li>composite-style programs and the computes of composite-style passes read colortex
 *   {@code BindingUse.use_alt}, the model's static flip schedule (a disagreement with the tracked
 *   state is reported and the model wins);</li>
 *   <li>geometry programs and geometry-pass computes read the current texture
 *   ({@link FlipState#read}), which is what they also draw into;</li>
 *   <li>shadowcolor reads follow the shadowcomp flips ({@link FlipState#shadowRead}).</li>
 * </ul>
 */
public final class ColorReads {
    private ColorReads() {
    }

    /**
     * @param kind     the sampling program's kind
     * @param resource the sampled resource
     * @param useAlt   the binding's {@code use_alt}
     * @param flips    the current flip state
     * @param warnings receives disagreements between {@code use_alt} and the flip state
     * @return whether to bind the alternate texture (false for resources that are not ping-ponged)
     */
    public static boolean alt(ProgramKind kind, ResourceRef resource, boolean useAlt, FlipState flips, Consumer<String> warnings) {
        return switch (resource) {
            case ResourceRef.ColorTex c -> color(kind, c.index(), useAlt, flips, warnings);
            case ResourceRef.ColorImage c -> color(kind, c.index(), useAlt, flips, warnings);
            case ResourceRef.ShadowColor s -> flips.shadowRead(s.index());
            case ResourceRef.ShadowColorImage s -> flips.shadowRead(s.index());
            default -> false;
        };
    }

    /**
     * @param kind a program kind
     * @return whether the program reads targets in their current texture rather than per
     *     {@code use_alt}: geometry programs and computes of geometry passes
     */
    public static boolean followsCurrentState(ProgramKind kind) {
        return kind instanceof ProgramKind.Geometry || kind instanceof ProgramKind.GeometryCompute;
    }

    private static boolean color(ProgramKind kind, int index, boolean useAlt, FlipState flips, Consumer<String> warnings) {
        boolean current = flips.read(index);
        if (followsCurrentState(kind)) {
            return current;
        }
        if (useAlt != current) {
            warnings.accept("colortex" + index + ": binding use_alt=" + useAlt + " disagrees with the pass flip state; following use_alt");
        }
        return useAlt;
    }
}
