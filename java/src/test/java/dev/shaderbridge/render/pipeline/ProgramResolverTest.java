package dev.shaderbridge.render.pipeline;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.render.RenderFixture;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.client.renderer.RenderPipelines;
import org.junit.jupiter.api.Test;

class ProgramResolverTest {
    private static final RenderFixture TUTORIAL = RenderFixture.load(RenderFixture.TUTORIAL4);
    private static final RenderFixture GLIMMER = RenderFixture.load(RenderFixture.GLIMMER);
    private static final PipelineCapabilities INDEPENDENT = new PipelineCapabilities(true, PipelineCapabilities.DEFAULT_MAX_DESCRIPTORS);

    /** Everything a resolver needs, over a fake device. */
    private static final class Harness {
        final SpirvModules modules = new SpirvModules();
        final FakeDevice fake = new FakeDevice(modules, true);
        final PackPipelineCache cache = new PackPipelineCache(fake.device(), new PackShaderSource(modules), Runnable::run, Runnable::run, modules);
        final PipelineDiagnostics diagnostics = new PipelineDiagnostics();
        final ProgramResolver resolver;

        Harness(RenderFixture fixture, PipelineCapabilities caps, RawPath raw, VariantSource variants) {
            PackPipelineFactory factory = new PackPipelineFactory(fixture.pack().info().sourceHash(), modules, caps, DrawProfiles.get());
            resolver = new ProgramResolver(fixture.dim(), fixture.blobs(), DepthMode.REVERSED_ZERO_TO_ONE, factory, cache, raw, variants, diagnostics);
        }

        void compile() {
            fake.completeAll();
            cache.poll();
        }
    }

    /** A raw path that accepts every program and prepares it instantly. */
    private static final class AcceptingRawPath implements RawPath {
        final List<ProgramVariant> admitted = new ArrayList<>();

        @Override
        public Admission admit(DimensionPipeline dim, ProgramVariant program, List<String> renderpearlProblems) {
            admitted.add(program);
            return new Admission.Accepted(new RawProgram() {
                @Override
                public ProgramVariant program() {
                    return program;
                }

                @Override
                public State state() {
                    return new State.Ready();
                }

                @Override
                public void close() {
                }
            });
        }
    }

    @Test
    void fullscreenProgramIsPendingUntilCompiled() {
        Harness h = new Harness(TUTORIAL, PipelineCapabilities.baseline(), RawPath.NONE, new CompiledVariants(TUTORIAL.blobs()));
        Program composite = TUTORIAL.program("composite", "fullscreen");
        int index = TUTORIAL.indexOf(composite);
        AttachmentLayout layout = AttachmentLayout.fullscreen(TUTORIAL.dim(), composite);
        assertInstanceOf(ProgramResolution.Pending.class, h.resolver.program(index, layout));
        h.compile();
        ProgramResolution.Renderpearl ready = assertInstanceOf(ProgramResolution.Renderpearl.class, h.resolver.program(index, layout));
        assertSame(h.fake.compiled.getFirst(), ready.compiled());
        assertEquals(composite.name(), ready.pipeline().key().program());
    }

    @Test
    void geometryRunsTheSlotProgramForTheDrawProfile() {
        Harness h = new Harness(TUTORIAL, INDEPENDENT, RawPath.NONE, new CompiledVariants(TUTORIAL.blobs()));
        PipelineShape shape = PipelineShape.of(RenderPipelines.ENTITY_CUTOUT);
        ProgramResolver.GeometryResolution first = h.resolver.geometry(GeometryProgram.ENTITIES, "vanilla_entity", shape, false);
        assertEquals(GeometryProgram.ENTITIES, first.program());
        assertInstanceOf(ProgramResolution.Pending.class, first.resolution());
        h.compile();
        ProgramResolution.Renderpearl ready = assertInstanceOf(ProgramResolution.Renderpearl.class,
            h.resolver.geometry(GeometryProgram.ENTITIES, "vanilla_entity", shape, false).resolution());
        assertEquals("gbuffers_entities", ready.pipeline().key().program());
        assertEquals(List.of(), h.diagnostics.messages());
    }

    @Test
    void geometryThatCannotRunFallsBackAlongItsChainAndIsReported() {
        // Without independentBlend no program writing a subset of the shared attachments can run,
        // and nothing in ENTITIES' chain was compiled for vanilla_entity besides gbuffers_entities.
        Harness h = new Harness(TUTORIAL, PipelineCapabilities.baseline(), RawPath.NONE, new CompiledVariants(TUTORIAL.blobs()));
        ProgramResolver.GeometryResolution r = h.resolver.geometry(GeometryProgram.ENTITIES, "vanilla_entity",
            PipelineShape.of(RenderPipelines.ENTITY_CUTOUT), false);
        ProgramResolution.Unavailable unavailable = assertInstanceOf(ProgramResolution.Unavailable.class, r.resolution());
        assertTrue(unavailable.reasons().stream().anyMatch(s -> s.startsWith("gbuffers_entities: ") && s.contains("independentBlend")),
            unavailable.reasons().toString());
        assertTrue(unavailable.reasons().stream().anyMatch(s -> s.contains("gbuffers_textured_lit was not compiled for draw profile vanilla_entity")),
            unavailable.reasons().toString());
        assertEquals(1, h.diagnostics.messages().size(), h.diagnostics.messages().toString());
        assertTrue(h.diagnostics.messages().getFirst().startsWith("gbuffers_entities [vanilla_entity] cannot run"));
        // The decision is cached: asking again reports nothing new.
        h.resolver.geometry(GeometryProgram.ENTITIES, "vanilla_entity", PipelineShape.of(RenderPipelines.ENTITY_CUTOUT), false);
        assertEquals(1, h.diagnostics.messages().size());
    }

    @Test
    void fallsBackToTheNextProgramThatCanRun() {
        // TERRAIN_SOLID resolves to gbuffers_terrain. Mark it raw-only: the chain continues to
        // gbuffers_textured_lit, compiled for vanilla_position_tex_color like the requested profile.
        DimensionPipeline dim = TUTORIAL.dim();
        List<Program> programs = new ArrayList<>(dim.programs());
        int terrain = dim.geometry().get(GeometryProgram.TERRAIN_SOLID).program();
        programs.set(terrain, Programs.rawOnly(programs.get(terrain)));
        Program lit = dim.programs().get(dim.geometry().get(GeometryProgram.TEXTURED_LIT).program());
        String profile = lit.drawProfile();
        VariantSource variants = (d, slot, p) -> slot == GeometryProgram.TERRAIN_SOLID
            ? new VariantSource.Lookup.Found(new ProgramVariant("", programs.get(terrain), TUTORIAL.blobs()))
            : new CompiledVariants(TUTORIAL.blobs()).find(d, slot, p);
        Harness h = new Harness(TUTORIAL, INDEPENDENT, RawPath.NONE, variants);
        PipelineShape shape = PipelineShape.of(RenderPipelines.GUI_TEXTURED);
        ProgramResolver.GeometryResolution r = h.resolver.geometry(GeometryProgram.TERRAIN_SOLID, profile, shape, false);
        assertEquals(GeometryProgram.TEXTURED_LIT, r.program());
        assertInstanceOf(ProgramResolution.Pending.class, r.resolution());
        assertTrue(h.diagnostics.messages().getFirst().contains("raw-Vulkan only"), h.diagnostics.messages().toString());
    }

    @Test
    void pendingVariantsAreWaitedFor() {
        Harness h = new Harness(TUTORIAL, INDEPENDENT, RawPath.NONE, (d, slot, p) -> new VariantSource.Lookup.Pending());
        ProgramResolver.GeometryResolution r = h.resolver.geometry(GeometryProgram.BLOCK, "vanilla_block", PipelineShape.of(RenderPipelines.SOLID_BLOCK),
            false);
        assertEquals(GeometryProgram.BLOCK, r.program());
        assertInstanceOf(ProgramResolution.Pending.class, r.resolution());
    }

    @Test
    void computeAndRawOnlyProgramsGoToTheRawPath() {
        AcceptingRawPath raw = new AcceptingRawPath();
        Harness h = new Harness(GLIMMER, INDEPENDENT, raw, new CompiledVariants(GLIMMER.blobs()));
        for (int i = 0; i < GLIMMER.dim().programs().size(); i++) {
            Program p = GLIMMER.dim().programs().get(i);
            if (p.kind() instanceof ProgramKind.Compute || (p.kind() instanceof ProgramKind.Composite && p.requiresRawVulkan())) {
                ProgramResolution r = h.resolver.program(i, AttachmentLayout.fullscreen(GLIMMER.dim(), p));
                assertInstanceOf(ProgramResolution.Raw.class, r, p.name());
            }
        }
        assertTrue(raw.admitted.size() >= 10, "admitted " + raw.admitted.size());
        Harness none = new Harness(GLIMMER, INDEPENDENT, RawPath.NONE, new CompiledVariants(GLIMMER.blobs()));
        Program setup = GLIMMER.program("world0/setup.csh", null);
        ProgramResolution.Unavailable unavailable = assertInstanceOf(ProgramResolution.Unavailable.class,
            none.resolver.program(GLIMMER.indexOf(setup), AttachmentLayout.fullscreen(GLIMMER.dim(), setup)));
        assertTrue(unavailable.reasons().contains("the raw Vulkan path is not available"), unavailable.reasons().toString());
        h.resolver.close();
    }
}
