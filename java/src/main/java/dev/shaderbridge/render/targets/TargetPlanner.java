package dev.shaderbridge.render.targets;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.BindingEntry;
import dev.shaderbridge.model.ColorTarget;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.model.ShadowSettings;
import dev.shaderbridge.model.TargetSize;
import dev.shaderbridge.model.TextureFormat;
import dev.shaderbridge.render.pipeline.TextureFormats;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.SortedSet;
import java.util.TreeSet;
import java.util.function.ToIntFunction;

/**
 * Decides which color targets a dimension pipeline needs and how big they are, with the rules of
 * the headless executor ({@code sb-runtime}): a {@code colortex} exists when the pack marks it
 * used, a gbuffers or composite program writes it, a binding refers to it, it is in the shared
 * gbuffers attachments, or it is {@code colortex0}; shadow color targets likewise from the shadow
 * programs, {@code shadowcomp} and the bindings. Sizes follow {@link TargetSize} (shadow targets
 * are the shadow map resolution) clamped to the device limit; mipmapped targets get a full chain.
 */
public final class TargetPlanner {
    /** {@code colortex0..31}. */
    public static final int MAX_COLOR_TEX = 32;
    /** {@code shadowcolor0..7}. */
    public static final int MAX_SHADOW_COLOR = 8;
    /** Smallest shadow map resolution. */
    public static final int MIN_SHADOW_RESOLUTION = 16;
    /** Largest shadow map resolution. */
    public static final int MAX_SHADOW_RESOLUTION = 8192;

    private TargetPlanner() {
    }

    /**
     * @param dim     a dimension pipeline
     * @param width   screen width in pixels
     * @param height  screen height in pixels
     * @param maxSize largest texture extent the device supports for a format
     * @return the colortex targets, by index
     */
    public static List<TargetSpec> colorTargets(DimensionPipeline dim, int width, int height, ToIntFunction<GpuFormat> maxSize) {
        SortedSet<Integer> used = new TreeSet<>();
        dim.targets().colortex().stream().filter(ColorTarget::used).forEach(t -> used.add(t.index()));
        used.addAll(dim.gbufferAttachments());
        used.add(0);
        for (Program p : dim.programs()) {
            if (!writesShadowTargets(p)) {
                used.addAll(p.drawBuffers());
            }
        }
        for (BindingEntry e : dim.bindings().entries()) {
            switch (e.resource()) {
                case ResourceRef.ColorTex t -> used.add(t.index());
                case ResourceRef.ColorImage t -> used.add(t.index());
                default -> {
                }
            }
        }
        List<TargetSpec> out = new ArrayList<>();
        for (int index : used.headSet(MAX_COLOR_TEX)) {
            ColorTarget t = target(dim.targets().colortex(), index);
            GpuFormat format = TextureFormats.renderable(t.format());
            int[] size = t.size().resolve(width, height);
            int limit = maxSize.applyAsInt(format);
            int w = Math.min(size[0], limit);
            int h = Math.min(size[1], limit);
            boolean mipped = !t.mipmapPrograms().isEmpty() || dim.programs().stream().anyMatch(p -> p.mipmapTargets().contains(index));
            out.add(new TargetSpec(index, false, format, w, h, mipped ? fullMipChain(w, h) : 1, t.clear(), clearColor(t)));
        }
        return out;
    }

    /**
     * @param dim     a dimension pipeline
     * @param maxSize largest texture extent the device supports for a format
     * @return the shadowcolor targets, by index, at the shadow map resolution
     */
    public static List<TargetSpec> shadowColorTargets(DimensionPipeline dim, ToIntFunction<GpuFormat> maxSize) {
        SortedSet<Integer> used = new TreeSet<>();
        dim.targets().shadowcolor().stream().filter(ColorTarget::used).forEach(t -> used.add(t.index()));
        used.addAll(dim.shadowAttachments());
        for (Program p : dim.programs()) {
            if (writesShadowTargets(p)) {
                used.addAll(p.drawBuffers());
            }
        }
        for (BindingEntry e : dim.bindings().entries()) {
            switch (e.resource()) {
                case ResourceRef.ShadowColor t -> used.add(t.index());
                case ResourceRef.ShadowColorImage t -> used.add(t.index());
                default -> {
                }
            }
        }
        ShadowSettings shadow = dim.targets().shadow();
        int resolution = shadowResolution(shadow);
        List<TargetSpec> out = new ArrayList<>();
        for (int index : used.headSet(MAX_SHADOW_COLOR)) {
            ColorTarget t = target(dim.targets().shadowcolor(), index);
            GpuFormat format = TextureFormats.renderable(t.format());
            int extent = Math.min(resolution, maxSize.applyAsInt(format));
            boolean mipped = index < shadow.colorMipmap().size() && shadow.colorMipmap().get(index);
            out.add(new TargetSpec(index, true, format, extent, extent, mipped ? fullMipChain(extent, extent) : 1, t.clear(), clearColor(t)));
        }
        return out;
    }

    /**
     * @param shadow the pack's shadow settings
     * @return the shadow map extent: the configured resolution clamped to
     *     [{@value #MIN_SHADOW_RESOLUTION}, {@value #MAX_SHADOW_RESOLUTION}], or 1 without a shadow pass
     */
    public static int shadowResolution(ShadowSettings shadow) {
        return shadow.enabled() ? Math.clamp(shadow.resolution(), MIN_SHADOW_RESOLUTION, MAX_SHADOW_RESOLUTION) : 1;
    }

    /**
     * @param width  base level width
     * @param height base level height
     * @return the number of levels of a full mip chain
     */
    public static int fullMipChain(int width, int height) {
        return 32 - Integer.numberOfLeadingZeros(Math.max(1, Math.max(width, height)));
    }

    /**
     * Shadow geometry programs ({@code shadow*}, {@code dh_shadow}) and {@code shadowcomp} passes
     * write shadow color targets; every other program writes colortex targets.
     */
    static boolean writesShadowTargets(Program program) {
        return switch (program.kind()) {
            case ProgramKind.Geometry g -> isShadowProgram(g.program());
            case ProgramKind.Composite c -> c.group() == PassGroup.SHADOW_COMP;
            default -> false;
        };
    }

    private static boolean isShadowProgram(GeometryProgram program) {
        return switch (program) {
            case SHADOW, SHADOW_SOLID, SHADOW_CUTOUT, SHADOW_WATER, SHADOW_ENTITIES, SHADOW_LIGHTNING, SHADOW_BLOCK, DH_SHADOW -> true;
            default -> false;
        };
    }

    /** The pack's configuration of a target, or Iris' defaults (RGBA8, cleared, screen-sized). */
    private static ColorTarget target(List<ColorTarget> targets, int index) {
        return targets.stream().filter(t -> t.index() == index).findFirst()
            .orElseGet(() -> new ColorTarget(index, TextureFormat.RGBA8, true, null, List.of(), new TargetSize.Relative(1, 1), true));
    }

    private static Optional<Rgba> clearColor(ColorTarget t) {
        return t.clearColor() == null ? Optional.empty() : Optional.of(Rgba.of(t.clearColor()));
    }
}
