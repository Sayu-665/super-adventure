package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.Optional;
import org.junit.jupiter.api.Test;

/**
 * The rendering diagnostic about chunk vertices is reported only when the extended terrain vertex
 * cannot be used, never while it supplies normals, block ids and mid-texture coordinates.
 */
class TerrainVertexNoteTest {
    @Test
    void silentWhileTheExtendedVertexWorks() {
        assertEquals(Optional.empty(), PackRenderer.terrainVertexNote(false, Optional.empty()));
    }

    @Test
    void silentWithSodiumWhichExtendsItsOwnVertex() {
        assertEquals(Optional.empty(), PackRenderer.terrainVertexNote(true, Optional.of("ChunkSectionLayer.vertexFormat() is not hooked")));
    }

    @Test
    void namesTheReasonWhenTheFormatCannotSwitch() {
        String note = PackRenderer.terrainVertexNote(false, Optional.of("ChunkSectionLayer.vertexFormat() is not hooked")).orElseThrow();
        assertTrue(note.startsWith("chunk terrain keeps Minecraft's own vertices (ChunkSectionLayer.vertexFormat() is not hooked)"), note);
        assertTrue(note.contains("mc_Entity -1"), note);
    }
}
