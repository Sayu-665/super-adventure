package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.BindingEntry;
import dev.shaderbridge.model.BindingTable;
import dev.shaderbridge.model.BindingUse;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.model.UniformLayout;
import dev.shaderbridge.render.pipeline.SpirvReflection.Descriptor;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;

/**
 * What to bind for every descriptor of a pack pipeline, by name ({@code RenderPass.setUniform}).
 * Every entry must be bound before a draw: the layout lists exactly the declared descriptors.
 *
 * @param bindings one entry per descriptor, in layout order
 */
public record BindingPlan(List<Binding> bindings) {
    /** Name of the per-frame block when a layout has none. */
    public static final String FRAME_BLOCK = "sb_Frame";
    /** Name of the per-draw block when a layout has none. */
    public static final String DRAW_BLOCK = "sb_Draw";

    public BindingPlan {
        bindings = List.copyOf(bindings);
    }

    /**
     * One descriptor.
     *
     * @param name   the name to bind
     * @param source what to bind
     */
    public record Binding(String name, Source source) {
    }

    /** Where a descriptor's value comes from. */
    public sealed interface Source {
        /** ShaderBridge's per-frame uniform buffer ({@code FrameUniforms}). */
        record FrameBlock() implements Source {
        }

        /** ShaderBridge's per-draw uniform slice ({@code DrawUniforms}). */
        record DrawBlock() implements Source {
        }

        /**
         * Bound by the host draw path (Minecraft's own {@code setUniform} calls, the DH or Sodium
         * integration) under the draw profile's name. Vanilla draws do not always bind every host
         * sampler a profile declares (beacon beams bind no lightmap, eyes no overlay): bind the
         * fallback resource first so the host's binding, if any, replaces it.
         *
         * @param fallback what to bind when the host does not, if known
         */
        record Host(Optional<ResourceRef> fallback) implements Source {
        }

        /**
         * A pack resource bound by ShaderBridge. {@link ResourceRef.Atlas} means the draw's albedo:
         * the texture the host binds as {@code Sampler0} for the same draw, else white.
         *
         * @param resource what to bind
         * @param useAlt   for a ping-ponged colortex: read the alt buffer
         */
        record Pack(ResourceRef resource, boolean useAlt) implements Source {
        }

        /** Nobody provides the descriptor: neither ShaderBridge, the binding table nor the draw profile. */
        record Unresolved() implements Source {
        }
    }

    /**
     * Classifies every descriptor of a program.
     *
     * @param iface    the reflected interface
     * @param program  the program (for {@code bindings_used} main/alt choices)
     * @param table    the dimension's binding table
     * @param uniforms the dimension's uniform layout (block names)
     * @param profile  the draw profile the program was translated for, if known to the host
     * @return the plan; descriptors nobody provides are {@link Source.Unresolved}
     */
    public static BindingPlan of(ProgramInterface iface, Program program, BindingTable table, UniformLayout uniforms,
                                 Optional<DrawProfileInfo> profile) {
        String frame = uniforms.frame().name().isEmpty() ? FRAME_BLOCK : uniforms.frame().name();
        String draw = uniforms.draw().name().isEmpty() ? DRAW_BLOCK : uniforms.draw().name();
        List<Binding> out = new ArrayList<>();
        for (Descriptor d : iface.descriptors().values()) {
            String name = d.name();
            Source source;
            if (name.equals(frame)) {
                source = new Source.FrameBlock();
            } else if (name.equals(draw)) {
                source = new Source.DrawBlock();
            } else if (profile.isPresent() && profile.get().blocks().contains(name)) {
                source = new Source.Host(Optional.empty());
            } else if (profile.isPresent() && profile.get().sampler(name).isPresent()) {
                source = new Source.Host(profile.get().sampler(name).get().provides().stream().findFirst()
                    .flatMap(table::get).map(BindingEntry::resource));
            } else {
                source = table.get(name).map(e -> fromTable(e, program)).orElseGet(Source.Unresolved::new);
            }
            out.add(new Binding(name, source));
        }
        return new BindingPlan(out);
    }

    private static Source fromTable(BindingEntry entry, Program program) {
        if (entry.resource() instanceof ResourceRef.UniformBlock) {
            return new Source.Host(Optional.empty());
        }
        boolean alt = program.bindingsUsed().stream().filter(u -> u.name().equals(entry.name())).anyMatch(BindingUse::useAlt);
        return new Source.Pack(entry.resource(), alt);
    }

    /** @return names of descriptors whose source is unknown (the pipeline cannot be drawn) */
    public List<String> unresolved() {
        return bindings.stream().filter(b -> b.source() instanceof Source.Unresolved).map(Binding::name).toList();
    }
}
