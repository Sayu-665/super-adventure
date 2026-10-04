package dev.shaderbridge.render.pipeline;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.render.RenderFixture;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;

/** {@link OnDemandVariants}: compiled variants are reused, missing ones compiled once in the background. */
class OnDemandVariantsTest {
    private static final RenderFixture FIXTURE = RenderFixture.load(RenderFixture.TUTORIAL4);
    private static final DimensionPipeline DIM = FIXTURE.dim();
    private static final Program TERRAIN = DIM.programFor(GeometryProgram.TERRAIN_SOLID).orElseThrow();

    private static Program withProfile(Program p, String profile) {
        return new Program(p.name(), p.kind(), profile, p.requiresRawVulkan(), p.stages(), p.drawBuffers(), p.outputSlots(), p.outputTypes(), p.blend(),
            p.blendPerBuffer(), p.alphaTest(), p.viewport(), p.mipmapTargets(), p.bindingsUsed(), p.vertexInputs(), p.pushConstantSize(), p.compute(),
            p.cull(), p.synthesizedFrom());
    }

    @Test
    void variantsThePackContainsNeedNoCompile() {
        List<String> compiles = new ArrayList<>();
        OnDemandVariants v = new OnDemandVariants(new CompiledVariants(FIXTURE.blobs()), (folder, slot, profile) -> {
            compiles.add(profile);
            throw new AssertionError("no compile expected");
        }, Runnable::run);
        VariantSource.Lookup.Found found = assertInstanceOf(VariantSource.Lookup.Found.class,
            v.find(DIM, GeometryProgram.TERRAIN_SOLID, TERRAIN.drawProfile()));
        assertSame(TERRAIN, found.variant().program());
        assertEquals(List.of(), compiles);
    }

    @Test
    void missingVariantsArePendingUntilCompiledOnce() {
        List<Runnable> queue = new ArrayList<>();
        List<String> compiles = new ArrayList<>();
        OnDemandVariants v = new OnDemandVariants(new CompiledVariants(FIXTURE.blobs()), (folder, slot, profile) -> {
            compiles.add(folder + "/" + slot.fileName() + "@" + profile);
            return new ProgramVariant(folder, withProfile(TERRAIN, profile), FIXTURE.blobs());
        }, queue::add);
        assertInstanceOf(VariantSource.Lookup.Pending.class, v.find(DIM, GeometryProgram.TERRAIN_SOLID, "vanilla_terrain_section"));
        assertInstanceOf(VariantSource.Lookup.Pending.class, v.find(DIM, GeometryProgram.TERRAIN_SOLID, "vanilla_terrain_section"));
        assertEquals(1, queue.size(), "one compile per variant");
        queue.removeFirst().run();
        VariantSource.Lookup.Found found = assertInstanceOf(VariantSource.Lookup.Found.class,
            v.find(DIM, GeometryProgram.TERRAIN_SOLID, "vanilla_terrain_section"));
        assertEquals("vanilla_terrain_section", found.variant().profile());
        assertEquals(List.of(DIM.folder() + "/gbuffers_terrain_solid@vanilla_terrain_section"), compiles);
    }

    @Test
    void failedCompilesStayMissing() {
        OnDemandVariants failing = new OnDemandVariants(new CompiledVariants(FIXTURE.blobs()), (folder, slot, profile) -> {
            throw new IllegalStateException("profile does not exist");
        }, Runnable::run);
        VariantSource.Lookup.Missing missing = assertInstanceOf(VariantSource.Lookup.Missing.class,
            failing.find(DIM, GeometryProgram.TERRAIN_SOLID, "nope"));
        assertTrue(missing.reason().contains("profile does not exist"), missing.reason());
        OnDemandVariants wrongProfile = new OnDemandVariants(new CompiledVariants(FIXTURE.blobs()),
            (folder, slot, profile) -> new ProgramVariant(folder, TERRAIN, FIXTURE.blobs()), Runnable::run);
        assertInstanceOf(VariantSource.Lookup.Missing.class, wrongProfile.find(DIM, GeometryProgram.TERRAIN_SOLID, "vanilla_terrain_section"));
        OnDemandVariants erroring = new OnDemandVariants(new CompiledVariants(FIXTURE.blobs()), (folder, slot, profile) -> {
            throw new StackOverflowError();
        }, Runnable::run);
        assertInstanceOf(VariantSource.Lookup.Missing.class, erroring.find(DIM, GeometryProgram.TERRAIN_SOLID, "vanilla_terrain_section"),
            "even errors end as a missing variant, never an exception on the render thread");
    }
}
