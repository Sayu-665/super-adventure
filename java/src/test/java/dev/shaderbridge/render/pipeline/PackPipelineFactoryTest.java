package dev.shaderbridge.render.pipeline;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.blaze3d.vertex.DefaultVertexFormat;
import com.mojang.renderpearl.api.pipeline.BindGroupLayout;
import com.mojang.renderpearl.api.pipeline.ColorTargetState;
import com.mojang.renderpearl.api.pipeline.CompareOp;
import com.mojang.renderpearl.api.pipeline.PrimitiveTopology;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import com.mojang.renderpearl.api.pipeline.ShaderType;
import com.mojang.renderpearl.api.pipeline.UniformType;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.render.RenderFixture;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.stream.Collectors;
import net.minecraft.client.renderer.RenderPipelines;
import org.junit.jupiter.api.Test;

/**
 * Builds renderpearl pipelines from a real compiled pack (no device needed: a
 * {@link RenderPipeline} is a description) and checks every part of them.
 */
class PackPipelineFactoryTest {
    private static final RenderFixture TUTORIAL = RenderFixture.load(RenderFixture.TUTORIAL4);
    private static final PipelineCapabilities INDEPENDENT = new PipelineCapabilities(true, PipelineCapabilities.DEFAULT_MAX_DESCRIPTORS);

    private static PackPipelineFactory factory(SpirvModules modules, PipelineCapabilities caps) {
        return new PackPipelineFactory(TUTORIAL.pack().info().sourceHash(), modules, caps, DrawProfiles.get());
    }

    private static ProgramVariant variant(Program program) {
        return new ProgramVariant(TUTORIAL.dim().folder(), program, TUTORIAL.blobs());
    }

    private static PackPipeline built(PackPipelineFactory.Result result) {
        if (result instanceof PackPipelineFactory.Result.Ineligible ineligible) {
            throw new AssertionError("ineligible: " + ineligible.reasons());
        }
        return ((PackPipelineFactory.Result.Built) result).pipeline();
    }

    @Test
    void compositeProgramBecomesAFullscreenPipeline() {
        SpirvModules modules = new SpirvModules();
        Program composite = TUTORIAL.program("composite", "fullscreen");
        PackPipeline p = built(factory(modules, PipelineCapabilities.baseline()).build(TUTORIAL.dim(), variant(composite),
            PipelineShape.fullscreen(), AttachmentLayout.fullscreen(TUTORIAL.dim(), composite), DepthMode.REVERSED_ZERO_TO_ONE));
        RenderPipeline pipeline = p.pipeline();
        assertEquals("shaderbridge", pipeline.getLocation().getNamespace());
        assertEquals(TUTORIAL.pack().info().sourceHash().substring(0, 16) + "/root/composite/fullscreen/fullscreen/fullscreen",
            pipeline.getLocation().getPath());
        assertEquals(PrimitiveTopology.TRIANGLES, pipeline.getPrimitiveTopology());
        assertNull(pipeline.getDepthStencilState());
        assertTrue(pipeline.getVertexFormatBindings().stream().allMatch(f -> f == null));
        assertEquals(1, pipeline.getColorTargetStates().size());
        assertEquals(ColorTargetState.WRITE_ALL, pipeline.getColorTargetStates().getFirst().writeMask());
        assertEquals(p.modules(), List.of(pipeline.getShaders().get(ShaderType.VERTEX), pipeline.getShaders().get(ShaderType.FRAGMENT)));
        p.modules().forEach(id -> {
            assertTrue(modules.contains(id));
            assertTrue(SpirvModules.isModuleId(id.toString()));
        });
        // Every descriptor is bound from a known source.
        assertEquals(List.of(), p.bindings().unresolved());
        BindingPlan.Source colortex0 = p.bindings().bindings().stream().filter(b -> b.name().equals("colortex0")).findFirst().orElseThrow().source();
        assertEquals(new BindingPlan.Source.Pack(new ResourceRef.ColorTex(0), false), colortex0);
    }

    @Test
    void entityProgramReplacesAVanillaEntityPipeline() {
        Program entities = TUTORIAL.program("gbuffers_entities", "vanilla_entity");
        RenderPipeline vanilla = RenderPipelines.ENTITY_CUTOUT;
        PipelineShape shape = PipelineShape.of(vanilla);
        AttachmentLayout layout = AttachmentLayout.geometry(TUTORIAL.dim(), entities, false);
        PackPipelineFactory.Result baseline = factory(new SpirvModules(), PipelineCapabilities.baseline()).build(TUTORIAL.dim(), variant(entities),
            shape, layout, DepthMode.REVERSED_ZERO_TO_ONE);
        assertInstanceOf(PackPipelineFactory.Result.Ineligible.class, baseline, "a subset of the shared attachments needs independentBlend");

        PackPipeline p = built(factory(new SpirvModules(), INDEPENDENT).build(TUTORIAL.dim(), variant(entities), shape, layout,
            DepthMode.REVERSED_ZERO_TO_ONE));
        RenderPipeline pipeline = p.pipeline();
        assertEquals(DefaultVertexFormat.ENTITY, pipeline.getVertexFormatBinding(0));
        assertEquals(PrimitiveTopology.QUADS, pipeline.getPrimitiveTopology());
        assertEquals(vanilla.isCull(), pipeline.isCull());
        assertEquals(CompareOp.GREATER_THAN_OR_EQUAL, pipeline.getDepthStencilState().depthTest());
        assertEquals(List.of(ColorTargetState.WRITE_ALL, ColorTargetState.WRITE_NONE, ColorTargetState.WRITE_NONE),
            pipeline.getColorTargetStates().stream().map(ColorTargetState::writeMask).toList());
        assertEquals(List.of(), p.bindings().unresolved());

        PackPipeline forward = built(factory(new SpirvModules(), INDEPENDENT).build(TUTORIAL.dim(), variant(entities), shape, layout,
            DepthMode.FORWARD_ZERO_TO_ONE));
        assertEquals(CompareOp.LESS_THAN_OR_EQUAL, forward.pipeline().getDepthStencilState().depthTest());
    }

    @Test
    void vertexDataWithoutTheProgramsInputsIsIneligible() {
        Program entities = TUTORIAL.program("gbuffers_entities", "vanilla_entity");
        PackPipelineFactory.Result result = factory(new SpirvModules(), INDEPENDENT).build(TUTORIAL.dim(), variant(entities),
            PipelineShape.of(RenderPipelines.SKY), AttachmentLayout.geometry(TUTORIAL.dim(), entities, false), DepthMode.REVERSED_ZERO_TO_ONE);
        PackPipelineFactory.Result.Ineligible ineligible = assertInstanceOf(PackPipelineFactory.Result.Ineligible.class, result);
        assertTrue(ineligible.reasons().stream().anyMatch(r -> r.contains("no matching vertex format element")), ineligible.reasons().toString());
    }

    @Test
    void terrainProgramDrawsFromItsProfileLayout() {
        Program terrain = TUTORIAL.program("gbuffers_terrain", "vanilla_terrain");
        PipelineShape shape = PipelineShape.ofProfile("vanilla_terrain", ProfileVertexFormats.get(), PrimitiveTopology.QUADS,
            DepthStates.standard(DepthMode.REVERSED_ZERO_TO_ONE), true).orElseThrow();
        PackPipeline p = built(factory(new SpirvModules(), PipelineCapabilities.baseline()).build(TUTORIAL.dim(), variant(terrain), shape,
            AttachmentLayout.geometry(TUTORIAL.dim(), terrain, false), DepthMode.REVERSED_ZERO_TO_ONE));
        assertEquals(ProfileVertexFormats.get().bindings("vanilla_terrain").orElseThrow(), p.pipeline().getVertexFormatBindings().subList(0, 2));
        assertEquals(1, p.pipeline().getVertexFormatBinding(1).getStepRate());
    }

    @Test
    void rawOnlyAndComputeProgramsAreIneligible() {
        RenderFixture glimmer = RenderFixture.load(RenderFixture.GLIMMER);
        PackPipelineFactory f = new PackPipelineFactory(glimmer.pack().info().sourceHash(), new SpirvModules(), INDEPENDENT, DrawProfiles.get());
        for (Program program : glimmer.dim().programs()) {
            boolean compute = program.kind() instanceof ProgramKind.Compute;
            AttachmentLayout layout = compute || !(program.kind() instanceof ProgramKind.Geometry)
                ? AttachmentLayout.fullscreen(glimmer.dim(), program) : AttachmentLayout.geometry(glimmer.dim(), program, false);
            PipelineShape shape = program.kind() instanceof ProgramKind.Geometry
                ? PipelineShape.ofProfile(program.drawProfile(), ProfileVertexFormats.get(), PrimitiveTopology.QUADS, null, true).orElseThrow()
                : PipelineShape.fullscreen();
            PackPipelineFactory.Result result = f.build(glimmer.dim(), new ProgramVariant(glimmer.dim().folder(), program, glimmer.blobs()), shape,
                layout, DepthMode.REVERSED_ZERO_TO_ONE);
            if (program.requiresRawVulkan()) {
                assertInstanceOf(PackPipelineFactory.Result.Ineligible.class, result, program.name());
            }
        }
    }

    /**
     * The bind group layout lists exactly the descriptors the SPIR-V declares, named as Mojang's
     * builder matches them: {@code sb_Frame}/{@code sb_Draw}, the draw profile's host blocks and
     * samplers, and the pack's resources from {@code bindings_used}.
     */
    @Test
    void bindGroupLayoutsNameEveryDeclaredDescriptor() {
        for (Program program : TUTORIAL.dim().programs()) {
            ProgramInterface iface = ProgramInterface.reflect(program, TUTORIAL.blobs());
            PipelineShape shape = program.kind() instanceof ProgramKind.Geometry
                ? PipelineShape.ofProfile(program.drawProfile(), ProfileVertexFormats.get(), PrimitiveTopology.QUADS, null, true).orElseThrow()
                : PipelineShape.fullscreen();
            BindGroupLayout layout = BindGroups.layout(iface, shape);
            List<String> names = BindGroupLayout.flattenUniforms(List.of(layout)).stream().map(BindGroupLayout.UniformDescription::name).toList();
            assertEquals(List.copyOf(iface.descriptors().keySet()), names, program.name());
            DrawProfileInfo profile = DrawProfiles.get().profile(program.drawProfile()).orElseThrow();
            Set<String> allowed = new HashSet<>(Set.of(BindingPlan.FRAME_BLOCK, BindingPlan.DRAW_BLOCK));
            program.bindingsUsed().forEach(b -> allowed.add(b.name()));
            allowed.addAll(profile.blocks());
            profile.samplers().forEach(s -> allowed.add(s.name()));
            Set<String> unexpected = names.stream().filter(n -> !allowed.contains(n)).collect(Collectors.toSet());
            assertEquals(Set.of(), unexpected, program.name());
            for (BindGroupLayout.UniformDescription u : BindGroupLayout.flattenUniforms(List.of(layout))) {
                boolean block = u.name().equals(BindingPlan.FRAME_BLOCK) || u.name().equals(BindingPlan.DRAW_BLOCK) || profile.blocks().contains(u.name())
                    || TUTORIAL.dim().bindings().get(u.name()).map(e -> e.resource() instanceof ResourceRef.UniformBlock).orElse(false);
                assertEquals(block ? UniformType.UNIFORM_BUFFER : UniformType.COMBINED_IMAGE_SAMPLER, u.type(), program.name() + ": " + u.name());
            }
            assertEquals(List.of(), BindingPlan.of(iface, program, TUTORIAL.dim().bindings(), TUTORIAL.dim().uniforms(),
                DrawProfiles.get().profile(program.drawProfile())).unresolved(), program.name());
        }
        assertFalse(TUTORIAL.dim().programs().isEmpty());
    }
}
