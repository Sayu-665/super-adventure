package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.CompiledPack;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.render.RenderFixture;
import java.util.List;
import java.util.Optional;
import org.junit.jupiter.api.Test;

/** {@link DimensionSelector}: Iris' folder choice per dimension. */
class DimensionSelectorTest {
    private static final CompiledPack BASE = RenderFixture.load(RenderFixture.TUTORIAL4).pack();

    private static DimensionPipeline dim(String folder, List<String> ids) {
        DimensionPipeline d = BASE.dimensions().getFirst();
        return new DimensionPipeline(folder, ids, d.targets(), d.settings(), d.uniforms(), d.customUniforms(), d.bindings(), d.programs(), d.geometry(),
            d.passes(), d.gbufferAttachments(), d.shadowAttachments(), d.endOfFrameCopies(), d.distantHorizons());
    }

    private static CompiledPack pack(DimensionPipeline... dims) {
        return new CompiledPack(BASE.formatVersion(), BASE.info(), BASE.options(), BASE.idMaps(), List.of(dims), List.of(), List.of());
    }

    private static Optional<String> folder(CompiledPack pack, String dimension) {
        return DimensionSelector.select(pack, dimension).map(DimensionPipeline::folder);
    }

    @Test
    void assignedFoldersWinOverTheWildcard() {
        CompiledPack pack = pack(dim("world0", List.of("minecraft:overworld", "*")), dim("world-1", List.of("minecraft:the_nether")),
            dim("world1", List.of("minecraft:the_end")));
        assertEquals(Optional.of("world-1"), folder(pack, "minecraft:the_nether"));
        assertEquals(Optional.of("world1"), folder(pack, "minecraft:the_end"));
        assertEquals(Optional.of("world0"), folder(pack, "minecraft:overworld"));
        assertEquals(Optional.of("world0"), folder(pack, "mymod:moon"), "other dimensions take the wildcard folder");
    }

    @Test
    void thePackRootServesDimensionsWithoutAFolder() {
        CompiledPack pack = pack(dim("", List.of("*")), dim("world-1", List.of("minecraft:the_nether")));
        assertEquals(Optional.of(""), folder(pack, "minecraft:overworld"));
        assertEquals(Optional.of("world-1"), folder(pack, "minecraft:the_nether"));
        assertEquals(Optional.of("world0"), folder(pack(dim("world0", List.of("*:*"))), "minecraft:the_end"));
    }

    @Test
    void dimensionsMatchingNothingRenderWithoutShaders() {
        assertTrue(folder(pack(dim("world-1", List.of("minecraft:the_nether"))), "minecraft:overworld").isEmpty());
    }
}
