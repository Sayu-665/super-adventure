package dev.shaderbridge.compat.sodium;

import dev.shaderbridge.model.IdMaps;
import java.util.Arrays;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;

/**
 * The pack's {@code block.properties} id of every block state, resolved once and then looked up by
 * the state's numeric id while Sodium meshes chunk sections on its worker threads. The resolution
 * is the one of ShaderBridge's vanilla terrain vertex ({@link dev.shaderbridge.render.chunk.BlockIdTable}:
 * Iris' precedence, every block entry before any tag entry, filters on properties a block does not
 * have ignored), so terrain gets the same ids with and without Sodium. Block tags are resolved with
 * the tags bound when the table is built (those of the world being played). Immutable; thread-safe.
 */
final class BlockIdTable {
    /** A table that maps nothing. */
    static final BlockIdTable EMPTY = new BlockIdTable(new int[0]);

    private final int[] ids;

    private BlockIdTable(int[] ids) {
        this.ids = ids;
    }

    /**
     * @param maps the pack's id maps
     * @return the table of every registered block state
     */
    static BlockIdTable build(IdMaps maps) {
        dev.shaderbridge.render.chunk.BlockIdTable resolved = dev.shaderbridge.render.chunk.BlockIdTable.of(maps.blocks());
        int[] ids = new int[Block.BLOCK_STATE_REGISTRY.size()];
        Arrays.fill(ids, -1);
        for (BlockState state : Block.BLOCK_STATE_REGISTRY) {
            int index = Block.getId(state);
            if (index >= 0 && index < ids.length) {
                ids[index] = resolved.id(state);
            }
        }
        return new BlockIdTable(ids);
    }

    /**
     * @param state a block state
     * @return its pack id, or -1
     */
    int id(BlockState state) {
        int index = Block.getId(state);
        return index >= 0 && index < ids.length ? ids[index] : -1;
    }
}
