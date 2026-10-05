package dev.shaderbridge.render.chunk;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;

import com.mojang.blaze3d.vertex.DefaultVertexFormat;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.client.renderer.RenderPipelines;
import org.junit.jupiter.api.Test;

class ChunkMeshFormatTest {
    private static Map<Integer, List<String>> ids(String... entries) {
        Map<Integer, List<String>> out = new LinkedHashMap<>();
        out.put(10001, List.of(entries));
        return out;
    }

    @Test
    void formatChangesFollowThePackAndItsBlockIds() {
        Map<Integer, List<String>> leaves = ids("oak_leaves");
        assertEquals(ChunkMeshFormat.Change.NONE, ChunkMeshFormat.change(false, null, null), "vanilla stays vanilla");
        assertEquals(ChunkMeshFormat.Change.TO_EXTENDED, ChunkMeshFormat.change(false, null, leaves), "a pack starts rendering");
        assertEquals(ChunkMeshFormat.Change.NONE, ChunkMeshFormat.change(true, leaves, leaves));
        assertEquals(ChunkMeshFormat.Change.NONE, ChunkMeshFormat.change(true, leaves, ids("oak_leaves")), "another pack with the same ids");
        assertEquals(ChunkMeshFormat.Change.TO_EXTENDED, ChunkMeshFormat.change(true, leaves, ids("birch_leaves")), "other ids rebuild");
        assertEquals(ChunkMeshFormat.Change.TO_VANILLA, ChunkMeshFormat.change(true, leaves, null), "the pack stops rendering");
        assertEquals(ChunkMeshFormat.Change.NONE, ChunkMeshFormat.change(true, Map.of(), Map.of()), "a pack without block.properties");
    }

    @Test
    void withoutAPackEveryHookIsVanilla() {
        assertFalse(ChunkMeshFormat.extended());
        assertSame(DefaultVertexFormat.BLOCK, ChunkMeshFormat.vertexFormat(DefaultVertexFormat.BLOCK));
        assertSame(RenderPipelines.SOLID_TERRAIN_MULTIDRAW, ChunkMeshFormat.pipeline(RenderPipelines.SOLID_TERRAIN_MULTIDRAW));
        assertSame(RenderPipelines.WIREFRAME, ChunkMeshFormat.pipeline(RenderPipelines.WIREFRAME));
        assertNull(ChunkMeshFormat.pipeline(null));
    }
}
