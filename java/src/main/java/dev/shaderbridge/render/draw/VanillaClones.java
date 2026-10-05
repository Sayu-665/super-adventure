package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.api.pipeline.ColorTargetState;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import dev.shaderbridge.render.pipeline.AttachmentLayout;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.function.Function;
import java.util.function.Supplier;
import net.minecraft.resources.Identifier;

/**
 * Vanilla pipelines adapted to the attachments of ShaderBridge's render passes, for draws no pack
 * program replaces. Mojang's render passes only accept pipelines with one color target state per
 * attachment, each of the attachment's format, so a vanilla draw inside the shared gbuffers pass
 * needs a {@link ClonedPipeline} with the pass's layout:
 *
 * <ul>
 *   <li>{@linkplain #fallback fallback}: the vanilla color output (location 0) goes to slot 0
 *   when slot 0 holds the pack's {@code fallback_tex}, with the vanilla blend and write mask;
 *   every other slot is write-masked. Without {@code independentBlend} the attachments of a
 *   pipeline must share one write mask, so with more than one attachment the clone writes depth
 *   only. The vanilla depth state is kept.</li>
 *   <li>{@linkplain #discard discard}: no color writes, no depth test or writes, for draws that
 *   must leave no trace (vanilla geometry in the shadow pass).</li>
 * </ul>
 *
 * Clones are compiled by Mojang's pipeline cache with the vanilla shader source (the compiler
 * function is {@code RenderSystem::getCompiledPipelineNullable}), which recompiles them after a
 * resource reload. That cache keeps every pipeline it compiled, by identity, until the next
 * resource reload, so the clones themselves are kept for the life of the process: rebuilding a
 * pack's resources (another pack, dimension or reload) reuses them instead of compiling another
 * set. Render thread only.
 */
public final class VanillaClones {
    /** Path prefix of clone locations (namespace {@code shaderbridge}). */
    static final String PATH_PREFIX = "clone/";

    /** Every clone made, shared by all instances (see the class description). */
    private static final Map<Key, RenderPipeline> CLONES = new HashMap<>();

    private final Function<RenderPipeline, CompiledRenderPipeline> compiler;
    private final boolean independentBlend;

    private enum Mode {
        FALLBACK, DISCARD
    }

    private record Key(RenderPipeline vanilla, AttachmentLayout layout, Mode mode, boolean independentBlend) {
    }

    /**
     * @param compiler         compiles (and caches) a pipeline with Minecraft's shader source;
     *                         returns null if it fails
     * @param independentBlend the device allows per-attachment write masks
     */
    public VanillaClones(Function<RenderPipeline, CompiledRenderPipeline> compiler, boolean independentBlend) {
        this.compiler = compiler;
        this.independentBlend = independentBlend;
    }

    /**
     * @param vanilla        a vanilla pipeline
     * @param layout         the pass's attachments
     * @param fallbackTarget the pack's {@code fallback_tex}
     * @return the vanilla draw adapted to the pass, or empty if it does not compile
     */
    public Optional<CompiledRenderPipeline> fallback(RenderPipeline vanilla, AttachmentLayout layout, int fallbackTarget) {
        Key key = new Key(vanilla, layout, Mode.FALLBACK, independentBlend);
        return compile(key, () -> fallbackPipeline(vanilla, layout, fallbackTarget, independentBlend));
    }

    /**
     * @param vanilla a vanilla pipeline
     * @param layout  the pass's attachments
     * @return the vanilla draw with every write disabled, or empty if it does not compile
     */
    public Optional<CompiledRenderPipeline> discard(RenderPipeline vanilla, AttachmentLayout layout) {
        return compile(new Key(vanilla, layout, Mode.DISCARD, false), () -> discardPipeline(vanilla, layout));
    }

    private Optional<CompiledRenderPipeline> compile(Key key, Supplier<RenderPipeline> build) {
        RenderPipeline clone = CLONES.computeIfAbsent(key, k -> build.get());
        return Optional.ofNullable(compiler.apply(clone));
    }

    /**
     * @param states the color target states of a pipeline
     * @param layout a pass's attachments
     * @return whether the pipeline can be bound in the pass as it is: one state per attachment,
     *     of the attachment's format (Mojang's render passes check exactly this)
     */
    public static boolean fits(List<ColorTargetState> states, AttachmentLayout layout) {
        if (states.size() != layout.attachments().size()) {
            return false;
        }
        for (int slot = 0; slot < states.size(); slot++) {
            if (states.get(slot) == null || states.get(slot).format() != layout.attachments().get(slot).format()) {
                return false;
            }
        }
        return true;
    }

    /**
     * @param vanilla          a vanilla pipeline
     * @param layout           the pass's attachments
     * @param fallbackTarget   the pack's {@code fallback_tex}
     * @param independentBlend per-attachment write masks are allowed
     * @return the fallback clone (see the class description)
     */
    static RenderPipeline fallbackPipeline(RenderPipeline vanilla, AttachmentLayout layout, int fallbackTarget, boolean independentBlend) {
        List<ColorTargetState> vanillaStates = vanilla.getColorTargetStates();
        ColorTargetState color = vanillaStates.isEmpty() ? null : vanillaStates.getFirst();
        boolean writes = color != null && !layout.attachments().isEmpty() && layout.attachments().getFirst().target() == fallbackTarget
            && (independentBlend || layout.attachments().size() == 1);
        List<ColorTargetState> states = new ArrayList<>();
        for (int slot = 0; slot < layout.attachments().size(); slot++) {
            AttachmentLayout.Attachment a = layout.attachments().get(slot);
            states.add(slot == 0 && writes ? new ColorTargetState(color.blendFunction(), a.format(), color.writeMask()) : masked(a));
        }
        return new ClonedPipeline(location(vanilla, layout, Mode.FALLBACK), vanilla, states, vanilla.getDepthStencilState());
    }

    /**
     * @param vanilla a vanilla pipeline
     * @param layout  the pass's attachments
     * @return the discard clone (see the class description)
     */
    static RenderPipeline discardPipeline(RenderPipeline vanilla, AttachmentLayout layout) {
        List<ColorTargetState> states = layout.attachments().stream().map(VanillaClones::masked).toList();
        return new ClonedPipeline(location(vanilla, layout, Mode.DISCARD), vanilla, new ArrayList<>(states), null);
    }

    private static ColorTargetState masked(AttachmentLayout.Attachment a) {
        return new ColorTargetState(Optional.empty(), a.format(), ColorTargetState.WRITE_NONE);
    }

    private static Identifier location(RenderPipeline vanilla, AttachmentLayout layout, Mode mode) {
        Identifier v = vanilla.getLocation();
        return Identifier.fromNamespaceAndPath("shaderbridge",
            PATH_PREFIX + v.getNamespace() + "/" + v.getPath() + "/" + layout.id() + "/" + mode.name().toLowerCase(Locale.ROOT));
    }
}
