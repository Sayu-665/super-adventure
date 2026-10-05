package dev.shaderbridge.render.raw;

import dev.shaderbridge.model.BindingEntry;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.ResourceKind;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.render.targets.ColorPair;
import dev.shaderbridge.render.targets.TargetSpec;
import java.util.Set;
import java.util.TreeSet;

/**
 * The render targets a dimension pipeline binds as storage images ({@code colorimgN},
 * {@code shadowcolorimgN}, or a target's sampler name declared as an image), which need
 * {@code VK_IMAGE_USAGE_STORAGE_BIT}: both textures of each, by the labels {@link ColorPair}
 * creates them with. Other targets keep Minecraft's usage, as storage usage can cost
 * render-target compression on some GPUs.
 */
public final class StorageTargets {
    private StorageTargets() {
    }

    /**
     * @param dim a dimension pipeline
     * @return the labels of the textures that need the storage bit
     */
    public static Set<String> labels(DimensionPipeline dim) {
        Set<String> labels = new TreeSet<>();
        for (BindingEntry entry : dim.bindings().entries()) {
            if (!(entry.kind() instanceof ResourceKind.StorageImage)) {
                continue;
            }
            String name = switch (entry.resource()) {
                case ResourceRef.ColorImage c -> TargetSpec.name(false, c.index());
                case ResourceRef.ColorTex c -> TargetSpec.name(false, c.index());
                case ResourceRef.ShadowColorImage s -> TargetSpec.name(true, s.index());
                case ResourceRef.ShadowColor s -> TargetSpec.name(true, s.index());
                default -> null;
            };
            if (name != null) {
                labels.add(ColorPair.label(name, false));
                labels.add(ColorPair.label(name, true));
            }
        }
        return labels;
    }
}
