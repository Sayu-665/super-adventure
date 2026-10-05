package dev.shaderbridge.render.chunk;

import net.minecraft.core.BlockPos;
import net.minecraft.world.level.block.state.BlockState;

/**
 * The block whose quads a chunk meshing thread is emitting, set by the section compiler hooks
 * before each block's fluid and model are tesselated and read by
 * {@link ExtendedTerrainBufferBuilder} at the first vertex of every quad. Sections are meshed on
 * several threads at once, each meshing one section at a time, so there is one context per
 * thread. Not thread-safe: only its own thread uses it.
 */
final class TerrainVertexContext {
    private static final ThreadLocal<TerrainVertexContext> CURRENT = ThreadLocal.withInitial(TerrainVertexContext::new);

    /** The state whose id {@code mc_Entity.x} carries (the fluid's block while a fluid is meshed); null outside any block. */
    private BlockState idState;
    /** The block at the position (its light emission goes to {@code at_midBlock.w}, for fluids too, as in Iris). */
    private BlockState blockState;
    private int renderType = TerrainVertexEncoder.NONE;
    private int localX;
    private int localY;
    private int localZ;

    /** @return the calling thread's context */
    static TerrainVertexContext current() {
        return CURRENT.get();
    }

    /**
     * The section compiler moved to a block: its fluid and model follow.
     *
     * @param state the block state at {@code pos}
     * @param pos   the block's position
     */
    void block(BlockState state, BlockPos pos) {
        idState = state;
        blockState = state;
        renderType = TerrainVertexEncoder.RENDER_TYPE_BLOCK;
        localX = pos.getX() & 15;
        localY = pos.getY() & 15;
        localZ = pos.getZ() & 15;
    }

    /**
     * The fluid of the current block is being meshed.
     *
     * @param fluidBlock the block state of the fluid ({@code FluidState.createLegacyBlock()}),
     *                   whose id the fluid's quads carry
     */
    void fluid(BlockState fluidBlock) {
        idState = fluidBlock;
        renderType = TerrainVertexEncoder.RENDER_TYPE_FLUID;
    }

    /** The fluid of the current block is meshed; its model follows. */
    void endFluid() {
        idState = blockState;
        renderType = blockState == null ? TerrainVertexEncoder.NONE : TerrainVertexEncoder.RENDER_TYPE_BLOCK;
    }

    /**
     * @param ids the pack's block ids
     * @return the record of a quad emitted now ({@link TerrainVertexEncoder#quad})
     */
    long record(BlockIdTable ids) {
        if (idState == null) {
            return TerrainVertexEncoder.NO_BLOCK;
        }
        int emission = blockState == null ? 0 : blockState.getLightEmission();
        return TerrainVertexEncoder.quad(ids.id(idState), renderType, localX, localY, localZ, emission);
    }
}
