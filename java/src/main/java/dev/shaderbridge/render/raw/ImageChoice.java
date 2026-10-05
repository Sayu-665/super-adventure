package dev.shaderbridge.render.raw;

import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.ResourceRef;
import java.util.List;
import java.util.function.Consumer;

/**
 * Which texture of a ping-ponged target a raw program binds, sampled or as a storage image, with
 * the headless executor's rules ({@code color_read} in {@code sb-runtime/src/descriptors.rs}, as
 * {@code ColorReads} applies them to renderpearl draws): computes of composite-style passes and
 * fullscreen programs follow {@code BindingUse.use_alt} (reporting a disagreement with the frame's
 * flip state), computes of geometry passes the current state; shadowcolor targets follow the
 * shadowcomp flips.
 */
public final class ImageChoice {
    private ImageChoice() {
    }

    /**
     * @param kind           the program's kind
     * @param resource       the bound resource
     * @param useAlt         the binding's {@code use_alt}
     * @param colorAlt       per colortex index: the current contents are in the alternate texture
     * @param shadowColorAlt per shadowcolor index: the current contents are in the alternate texture
     * @param warnings       receives disagreements between {@code use_alt} and the flip state
     * @return whether to bind the alternate texture (false for resources that are not ping-ponged)
     */
    public static boolean alt(ProgramKind kind, ResourceRef resource, boolean useAlt, List<Boolean> colorAlt, List<Boolean> shadowColorAlt,
                              Consumer<String> warnings) {
        return switch (resource) {
            case ResourceRef.ColorTex c -> color(kind, c.index(), useAlt, colorAlt, warnings);
            case ResourceRef.ColorImage c -> color(kind, c.index(), useAlt, colorAlt, warnings);
            case ResourceRef.ShadowColor s -> state(shadowColorAlt, s.index());
            case ResourceRef.ShadowColorImage s -> state(shadowColorAlt, s.index());
            // A sampler the pack did not name reads colortex0 in its current texture outside geometry programs.
            case ResourceRef.Unknown u -> !(kind instanceof ProgramKind.Geometry) && state(colorAlt, 0);
            default -> false;
        };
    }

    private static boolean color(ProgramKind kind, int index, boolean useAlt, List<Boolean> colorAlt, Consumer<String> warnings) {
        boolean current = state(colorAlt, index);
        if (kind instanceof ProgramKind.Geometry || kind instanceof ProgramKind.GeometryCompute) {
            return current;
        }
        if (useAlt != current) {
            warnings.accept("colortex" + index + ": binding use_alt=" + useAlt + " disagrees with the pass flip state; following use_alt");
        }
        return useAlt;
    }

    private static boolean state(List<Boolean> alt, int index) {
        return index >= 0 && index < alt.size() && alt.get(index);
    }
}
