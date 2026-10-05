package dev.shaderbridge.compat.sodium;

/**
 * Extension data carried by Sodium's {@code ChunkVertexEncoder.Vertex} objects (added by
 * {@code ChunkVertexMixin}). Sodium keeps translucent quads for sorting by copying their vertices
 * ({@code Vertex.copyVertexTo}), and encodes the quads it splits only after the block they came
 * from was meshed; the tags travel with those copies so the split quads keep their block's id,
 * position and texture centre. A vertex is tagged when its quad enters the sorter
 * ({@link SodiumTerrain#tagQuad}); untagged vertices read as "no block".
 */
public interface VertexTags {
    /** @return the {@code sb_Entity} value of the vertex's quad */
    int shaderbridge$entity();

    /** @return the block reference ({@link TerrainExtension#block}) of the vertex's quad */
    int shaderbridge$block();

    /** @return the {@code sb_MidTexCoord} value of the vertex's quad */
    int shaderbridge$midTexCoord();

    /**
     * Tags the vertex.
     *
     * @param entity      {@code sb_Entity} value
     * @param block       block reference
     * @param midTexCoord {@code sb_MidTexCoord} value
     */
    void shaderbridge$tag(int entity, int block, int midTexCoord);
}
