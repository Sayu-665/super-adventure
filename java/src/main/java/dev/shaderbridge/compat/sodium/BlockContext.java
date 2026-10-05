package dev.shaderbridge.compat.sodium;

/**
 * The block Sodium is meshing on the current thread, set around Sodium's calls that mesh one
 * block model or one fluid ({@code ChunkBuilderMeshingTaskMixin}, {@link SodiumTerrain#enterBlock}),
 * so that the extended vertex encoder knows the extension data of the quads it writes. One mutable
 * instance per thread.
 */
final class BlockContext {
    private static final ThreadLocal<BlockContext> CURRENT = ThreadLocal.withInitial(BlockContext::new);

    private boolean active;
    private int entity;
    private int block = TerrainExtension.NO_BLOCK;

    private BlockContext() {
    }

    /** @return the current thread's context */
    static BlockContext current() {
        return CURRENT.get();
    }

    /**
     * Starts a block.
     *
     * @param entity its {@code sb_Entity} value ({@link TerrainExtension#entity})
     * @param block  its block reference ({@link TerrainExtension#block})
     */
    void enter(int entity, int block) {
        this.active = true;
        this.entity = entity;
        this.block = block;
    }

    /** Ends the block. */
    void exit() {
        this.active = false;
        this.entity = 0;
        this.block = TerrainExtension.NO_BLOCK;
    }

    /** @return whether a block is being meshed on this thread */
    boolean active() {
        return active;
    }

    /** @return the {@code sb_Entity} value of the block being meshed */
    int entity() {
        return entity;
    }

    /** @return the block reference of the block being meshed */
    int block() {
        return block;
    }
}
