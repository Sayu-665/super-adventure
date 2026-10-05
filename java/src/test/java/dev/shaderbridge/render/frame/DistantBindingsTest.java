package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.pipeline.CompareOp;
import com.mojang.renderpearl.api.pipeline.DepthStencilState;
import com.mojang.renderpearl.api.pipeline.PrimitiveTopology;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import dev.shaderbridge.dh.DhHostBlocks;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.render.pipeline.AttachmentLayout;
import dev.shaderbridge.render.pipeline.BindingPlan;
import dev.shaderbridge.render.pipeline.DrawProfiles;
import dev.shaderbridge.render.pipeline.PackPipelineFactory;
import dev.shaderbridge.render.pipeline.PipelineCapabilities;
import dev.shaderbridge.render.pipeline.PipelineShape;
import dev.shaderbridge.render.pipeline.ProfileVertexFormats;
import dev.shaderbridge.render.pipeline.ProgramVariant;
import dev.shaderbridge.render.pipeline.SpirvModules;
import java.util.List;
import java.util.Optional;
import org.junit.jupiter.api.Test;

/**
 * {@link DistantBindings} and the LOD pipeline shape {@link DistantPasses} builds, on Tutorial 4's
 * synthesized {@code dh_terrain} program (a real compiled pack).
 */
class DistantBindingsTest {
    private static final RenderFixture TUTORIAL = RenderFixture.load(RenderFixture.TUTORIAL4);

    private static PackPipelineFactory.Result build(Program program) {
        PipelineShape shape = PipelineShape.ofProfile(program.drawProfile(), ProfileVertexFormats.get(), PrimitiveTopology.TRIANGLES,
            DepthStencilState.DEFAULT, true).orElseThrow();
        PackPipelineFactory factory = new PackPipelineFactory(TUTORIAL.pack().info().sourceHash(), new SpirvModules(),
            new PipelineCapabilities(true, PipelineCapabilities.DEFAULT_MAX_DESCRIPTORS), DrawProfiles.get());
        return factory.build(TUTORIAL.dim(), new ProgramVariant(TUTORIAL.dim().folder(), program, TUTORIAL.blobs()), shape,
            AttachmentLayout.geometry(TUTORIAL.dim(), program, false), DepthMode.REVERSED_ZERO_TO_ONE);
    }

    @Test
    void synthesizedDhTerrainBuildsWithTheLodShapeAndHostBlocks() {
        Program program = TUTORIAL.program("dh_terrain", DrawProfiles.DH_SYNTH_PROFILE);
        PackPipelineFactory.Result.Built built = assertInstanceOf(PackPipelineFactory.Result.Built.class, build(program));
        RenderPipeline pipeline = built.pipeline().pipeline();
        assertEquals(16, pipeline.getVertexFormatBindings().getFirst().getVertexSize(), "Distant Horizons' 16-byte LOD vertices");
        assertEquals(PrimitiveTopology.TRIANGLES, pipeline.getPrimitiveTopology());
        assertEquals(CompareOp.GREATER_THAN_OR_EQUAL, pipeline.getDepthStencilState().depthTest(), "reversed-Z, like Minecraft and DH 3.3");
        assertTrue(pipeline.getDepthStencilState().writeDepth());
        DistantBindings bindings = DistantBindings.of(built.pipeline().bindings());
        assertTrue(bindings.supported(), bindings.unknown().toString());
        assertTrue(bindings.shared(), "uWorldYOffset etc. (the profile's position semantic)");
        assertTrue(bindings.unique(), "uModelOffset");
        assertEquals(List.of(), built.pipeline().bindings().unresolved());
    }

    @Test
    void hostDescriptorsAreClassified() {
        BindingPlan plan = new BindingPlan(List.of(
            new BindingPlan.Binding("sb_Frame", new BindingPlan.Source.FrameBlock()),
            new BindingPlan.Binding(DistantBindings.LIGHTMAP, new BindingPlan.Source.Host(Optional.of(new ResourceRef.Lightmap()))),
            new BindingPlan.Binding(DistantBindings.BLOCK_ATLAS, new BindingPlan.Source.Host(Optional.empty())),
            new BindingPlan.Binding(DhHostBlocks.SHARED_BLOCK, new BindingPlan.Source.Host(Optional.empty())),
            new BindingPlan.Binding("colortex0", new BindingPlan.Source.Pack(new ResourceRef.ColorTex(0), false))));
        DistantBindings bindings = DistantBindings.of(plan);
        assertEquals(new DistantBindings(true, true, true, false, List.of()), bindings);
        assertTrue(bindings.supported());
    }

    @Test
    void unknownHostDescriptorsWithoutAFallbackCannotBeBound() {
        BindingPlan plan = new BindingPlan(List.of(
            new BindingPlan.Binding("fragUniformBlock", new BindingPlan.Source.Host(Optional.empty())),
            new BindingPlan.Binding("someSampler", new BindingPlan.Source.Host(Optional.of(new ResourceRef.Lightmap())))));
        DistantBindings bindings = DistantBindings.of(plan);
        assertEquals(List.of("fragUniformBlock"), bindings.unknown(), "a host sampler with a pack fallback is bound by the binder");
        assertFalse(bindings.supported());
    }
}
