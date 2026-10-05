package dev.shaderbridge.render.pipeline;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.GeometrySlot;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.render.RenderFixture;
import java.util.ArrayList;
import java.util.EnumMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/**
 * {@link CompiledVariants}: variants come from the slot's {@code variants} map (profile to program
 * index, translated with the slot's {@code use_alt}), never from another program of the same name.
 */
class CompiledVariantsTest {
    private static final RenderFixture FIXTURE = RenderFixture.load(RenderFixture.TUTORIAL4);
    private static final DimensionPipeline DIM = FIXTURE.dim();

    private static Program withProfile(Program p, String profile) {
        return new Program(p.name(), p.kind(), profile, p.requiresRawVulkan(), p.stages(), p.drawBuffers(), p.outputSlots(), p.outputTypes(), p.blend(),
            p.blendPerBuffer(), p.alphaTest(), p.viewport(), p.mipmapTargets(), p.bindingsUsed(), p.vertexInputs(), p.pushConstantSize(), p.compute(),
            p.cull(), p.synthesizedFrom());
    }

    /** A copy of the fixture with extra programs appended and one slot replaced. */
    private static DimensionPipeline with(List<Program> extra, GeometryProgram slot, GeometrySlot geometry) {
        List<Program> programs = new ArrayList<>(DIM.programs());
        programs.addAll(extra);
        Map<GeometryProgram, GeometrySlot> slots = new EnumMap<>(DIM.geometry());
        slots.put(slot, geometry);
        return new DimensionPipeline(DIM.folder(), DIM.dimensionIds(), DIM.targets(), DIM.settings(), DIM.uniforms(), DIM.customUniforms(),
            DIM.bindings(), programs, slots, DIM.passes(), DIM.gbufferAttachments(), DIM.shadowAttachments(), DIM.endOfFrameCopies(),
            DIM.distantHorizons());
    }

    @Test
    void theSlotProgramServesItsOwnProfile() {
        Program terrain = DIM.programFor(GeometryProgram.TERRAIN_SOLID).orElseThrow();
        VariantSource.Lookup.Found found = assertInstanceOf(VariantSource.Lookup.Found.class,
            new CompiledVariants(FIXTURE.blobs()).find(DIM, GeometryProgram.TERRAIN_SOLID, terrain.drawProfile()));
        assertSame(terrain, found.variant().program());
        assertEquals(DIM.folder(), found.variant().folder());
    }

    @Test
    void otherProfilesComeFromTheVariantsMapNotFromAProgramOfTheSameName() {
        int base = DIM.geometry().get(GeometryProgram.TERRAIN_SOLID).program();
        Program terrain = DIM.programs().get(base);
        // Two translations of gbuffers_terrain for sodium_terrain: the first belongs to another slot's
        // pass (another use_alt); the slot lists the second.
        Program otherPass = withProfile(terrain, "sodium_terrain");
        Program ownPass = withProfile(terrain, "sodium_terrain");
        int ownIndex = DIM.programs().size() + 1;
        DimensionPipeline dim = with(List.of(otherPass, ownPass), GeometryProgram.TERRAIN_SOLID,
            new GeometrySlot(base, GeometryProgram.TERRAIN, Map.of("sodium_terrain", ownIndex)));
        VariantSource.Lookup.Found found = assertInstanceOf(VariantSource.Lookup.Found.class,
            new CompiledVariants(FIXTURE.blobs()).find(dim, GeometryProgram.TERRAIN_SOLID, "sodium_terrain"));
        assertSame(ownPass, found.variant().program());
    }

    @Test
    void profilesNeitherTheSlotNorItsVariantsHaveAreMissing() {
        CompiledVariants variants = new CompiledVariants(FIXTURE.blobs());
        // gbuffers_terrain exists for vanilla_entity (the block slot's program), but TERRAIN_SOLID does not list it.
        VariantSource.Lookup.Missing missing = assertInstanceOf(VariantSource.Lookup.Missing.class,
            variants.find(DIM, GeometryProgram.TERRAIN_SOLID, "vanilla_entity"));
        assertEquals("gbuffers_terrain was not compiled for draw profile vanilla_entity", missing.reason());
    }

    @Test
    void brokenVariantEntriesAreMissing() {
        int base = DIM.geometry().get(GeometryProgram.TERRAIN_SOLID).program();
        CompiledVariants variants = new CompiledVariants(FIXTURE.blobs());
        DimensionPipeline outOfRange = with(List.of(), GeometryProgram.TERRAIN_SOLID,
            new GeometrySlot(base, GeometryProgram.TERRAIN, Map.of("sodium_terrain", 9999)));
        assertInstanceOf(VariantSource.Lookup.Missing.class, variants.find(outOfRange, GeometryProgram.TERRAIN_SOLID, "sodium_terrain"));
        // The listed program was translated for another profile.
        int entities = DIM.geometry().get(GeometryProgram.ENTITIES).program();
        DimensionPipeline mismatch = with(List.of(), GeometryProgram.TERRAIN_SOLID,
            new GeometrySlot(base, GeometryProgram.TERRAIN, Map.of("sodium_terrain", entities)));
        VariantSource.Lookup.Missing missing = assertInstanceOf(VariantSource.Lookup.Missing.class,
            variants.find(mismatch, GeometryProgram.TERRAIN_SOLID, "sodium_terrain"));
        assertTrue(missing.reason().contains("translated for vanilla_entity"), missing.reason());
    }

    @Test
    void slotsThePackLacksAreMissing() {
        Map<GeometryProgram, GeometrySlot> slots = new EnumMap<>(DIM.geometry());
        slots.remove(GeometryProgram.LIGHTNING);
        DimensionPipeline dim = new DimensionPipeline(DIM.folder(), DIM.dimensionIds(), DIM.targets(), DIM.settings(), DIM.uniforms(),
            DIM.customUniforms(), DIM.bindings(), DIM.programs(), slots, DIM.passes(), DIM.gbufferAttachments(), DIM.shadowAttachments(),
            DIM.endOfFrameCopies(), DIM.distantHorizons());
        VariantSource.Lookup.Missing missing = assertInstanceOf(VariantSource.Lookup.Missing.class,
            new CompiledVariants(FIXTURE.blobs()).find(dim, GeometryProgram.LIGHTNING, "vanilla_entity"));
        assertTrue(missing.reason().contains("no program for"), missing.reason());
    }
}
