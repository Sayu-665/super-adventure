package dev.shaderbridge.compat.sodium;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.render.mapping.PipelineMapping;
import dev.shaderbridge.render.mapping.VanillaPipelineTable;
import java.util.Optional;
import net.minecraft.resources.Identifier;
import org.junit.jupiter.api.Test;

/**
 * {@link SodiumPipelines}: Sodium's three terrain pipelines draw with the programs of the vanilla
 * terrain pipelines they stand for, compiled for {@code sodium_terrain}; anything else stays
 * vanilla or is left to the vanilla table.
 */
class SodiumPipelinesTest {
    private static PipelineMapping.Mapped mapped(String path) {
        return assertInstanceOf(PipelineMapping.Mapped.class, SodiumPipelines.lookup(Identifier.fromNamespaceAndPath("sodium", path)).orElseThrow());
    }

    @Test
    void terrainPassesUseTheTerrainPrograms() {
        PipelineMapping.Mapped solid = mapped("pipeline/solid_terrain");
        assertEquals(GeometryProgram.TERRAIN_SOLID, solid.gbuffers());
        assertEquals(Optional.of(GeometryProgram.SHADOW_SOLID), solid.shadow());
        PipelineMapping.Mapped cutout = mapped("pipeline/cutout_terrain");
        assertEquals(GeometryProgram.TERRAIN_CUTOUT, cutout.gbuffers());
        assertEquals(Optional.of(GeometryProgram.SHADOW_CUTOUT), cutout.shadow());
        PipelineMapping.Mapped translucent = mapped("pipeline/translucent_terrain");
        assertEquals(GeometryProgram.WATER, translucent.gbuffers());
        assertEquals(Optional.of(GeometryProgram.SHADOW_WATER), translucent.shadow());
        for (PipelineMapping.Mapped m : java.util.List.of(solid, cutout, translucent)) {
            assertEquals("sodium_terrain", m.profile());
        }
    }

    @Test
    void sodiumAndVanillaTerrainShadeAlike() {
        for (String path : SodiumPipelines.TERRAIN_PATHS) {
            PipelineMapping.Mapped vanilla = assertInstanceOf(PipelineMapping.Mapped.class, VanillaPipelineTable.lookupPath(path), path);
            PipelineMapping.Mapped sodium = mapped(path);
            assertEquals(vanilla.gbuffers(), sodium.gbuffers(), path);
            assertEquals(vanilla.shadow(), sodium.shadow(), path);
        }
    }

    @Test
    void sodiumPipelinesNamedAfterExtendedTerrainClonesMapAlike() {
        // While ShaderBridge's extended vanilla chunk format is active, ChunkSectionLayer.pipeline()
        // returns clones located at shaderbridge:extended_terrain/minecraft/pipeline/..., and Sodium
        // names its pipelines after their paths.
        for (String path : SodiumPipelines.TERRAIN_PATHS) {
            PipelineMapping.Mapped clone = mapped("extended_terrain/minecraft/" + path);
            assertEquals(mapped(path), clone, path);
        }
        assertEquals("pipeline/solid_terrain", SodiumPipelines.terrainPath("extended_terrain/minecraft/pipeline/solid_terrain"));
        assertEquals("pipeline/solid_terrain", SodiumPipelines.terrainPath("pipeline/solid_terrain"));
        assertEquals("xpipeline/solid_terrain", SodiumPipelines.terrainPath("xpipeline/solid_terrain"));
        assertInstanceOf(PipelineMapping.Vanilla.class, SodiumPipelines.lookupPath("extended_terrain/minecraft/pipeline/lines"));
    }

    @Test
    void otherSodiumPipelinesStayVanilla() {
        PipelineMapping other = SodiumPipelines.lookup(Identifier.fromNamespaceAndPath("sodium", "pipeline/wireframe")).orElseThrow();
        assertInstanceOf(PipelineMapping.Vanilla.class, other);
    }

    @Test
    void otherNamespacesAreLeftToTheVanillaTable() {
        assertEquals(Optional.empty(), SodiumPipelines.lookup(Identifier.fromNamespaceAndPath("minecraft", "pipeline/solid_terrain")));
        // Sodium's OIT pipelines are located in the minecraft namespace (OitPipelineSet), where the
        // table keeps them vanilla.
        Identifier oit = Identifier.fromNamespaceAndPath("minecraft", "pipeline/oit_accumulate_sodium_terrain");
        assertEquals(Optional.empty(), SodiumPipelines.lookup(oit));
        PipelineMapping.Vanilla vanilla = assertInstanceOf(PipelineMapping.Vanilla.class, VanillaPipelineTable.lookup(oit));
        assertTrue(vanilla.reason().contains("order-independent transparency"), vanilla.reason());
    }
}
