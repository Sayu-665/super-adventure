package dev.shaderbridge.render.draw;

import static org.junit.jupiter.api.Assertions.assertEquals;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.render.frame.FlipState;
import dev.shaderbridge.render.pipeline.BindingPlan;
import dev.shaderbridge.render.pipeline.BindingPlan.Source;
import dev.shaderbridge.render.targets.TextureBinding;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.Set;
import org.junit.jupiter.api.Test;

/** {@link UniformBinder}: what is bound for each kind of descriptor, and main or alt per program kind. */
class UniformBinderTest {
    private static final RenderFixture FIXTURE = RenderFixture.load(RenderFixture.TUTORIAL4);
    private static final Program COMPOSITE = FIXTURE.dim().programs().stream().filter(p -> p.name().endsWith("composite")).findFirst().orElseThrow();
    private static final Program TERRAIN = FIXTURE.dim().programFor(GeometryProgram.TERRAIN_SOLID).orElseThrow();
    private static final GpuBufferSlice FRAME = new GpuBufferSlice(null, 0, 64);
    private static final GpuBufferSlice DRAW = new GpuBufferSlice(null, 256, 64);
    private static final BindingPlan PLAN = new BindingPlan(List.of(
        new BindingPlan.Binding("sb_Frame", new Source.FrameBlock()),
        new BindingPlan.Binding("sb_Draw", new Source.DrawBlock()),
        new BindingPlan.Binding("Sampler2", new Source.Host(Optional.of(new ResourceRef.Lightmap()))),
        new BindingPlan.Binding("DynamicTransforms", new Source.Host(Optional.empty())),
        new BindingPlan.Binding("colortex4", new Source.Pack(new ResourceRef.ColorTex(4), true)),
        new BindingPlan.Binding("shadowcolor0", new Source.Pack(new ResourceRef.ShadowColor(0), false)),
        new BindingPlan.Binding("mystery", new Source.Unresolved())));

    /** Records binds; names in {@code bound} already have a value. */
    private record Target(Set<String> bound, List<String> binds) implements UniformTarget {
        @Override
        public boolean isBound(String name) {
            return bound.contains(name);
        }

        @Override
        public Optional<TextureBinding> boundTexture(String name) {
            return Optional.empty();
        }

        @Override
        public void bind(String name, GpuBufferSlice slice) {
            binds.add(name + "=" + (slice == FRAME ? "frame" : slice == DRAW ? "draw" : "?"));
        }

        @Override
        public void bind(String name, TextureBinding texture) {
            binds.add(name + "=texture");
        }
    }

    private static List<String> bind(Program program, Set<String> bound, FlipState flips, List<String> resolved, List<String> warnings) {
        Target target = new Target(bound, new ArrayList<>());
        UniformBinder binder = new UniformBinder((resource, alt, p, host) -> {
            resolved.add(resource.getClass().getSimpleName() + (alt ? " alt" : " main"));
            return new TextureBinding(null, null);
        }, warnings::add);
        binder.bind(target, PLAN, program, flips, FRAME, DRAW, null);
        return target.binds();
    }

    @Test
    void compositeProgramsReadPerUseAltAndGetEveryPackDescriptor() {
        FlipState flips = new FlipState();
        flips.flip(List.of(4));
        flips.flipShadow(List.of(0));
        List<String> resolved = new ArrayList<>();
        List<String> warnings = new ArrayList<>();
        assertEquals(List.of("sb_Frame=frame", "sb_Draw=draw", "Sampler2=texture", "colortex4=texture", "shadowcolor0=texture"),
            bind(COMPOSITE, Set.of(), flips, resolved, warnings));
        assertEquals(List.of("Lightmap main", "ColorTex alt", "ShadowColor alt"), resolved);
        assertEquals(List.of(), warnings);
    }

    @Test
    void hostFallbacksNeverReplaceWhatTheHostBound() {
        List<String> resolved = new ArrayList<>();
        assertEquals(List.of("sb_Frame=frame", "sb_Draw=draw", "colortex4=texture", "shadowcolor0=texture"),
            bind(COMPOSITE, Set.of("Sampler2"), new FlipState(), resolved, new ArrayList<>()));
        assertEquals(List.of("ColorTex alt", "ShadowColor main"), resolved);
    }

    @Test
    void geometryProgramsReadTheCurrentTextureAndDisagreementsAreReported() {
        List<String> resolved = new ArrayList<>();
        List<String> warnings = new ArrayList<>();
        bind(TERRAIN, Set.of(), new FlipState(), resolved, warnings);
        assertEquals(List.of("Lightmap main", "ColorTex main", "ShadowColor main"), resolved, "use_alt is ignored for geometry");
        assertEquals(List.of(), warnings);
        resolved.clear();
        bind(COMPOSITE, Set.of(), new FlipState(), resolved, warnings);
        assertEquals("ColorTex alt", resolved.get(1), "the model's use_alt wins");
        assertEquals(1, warnings.size());
    }
}
