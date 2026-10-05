package dev.shaderbridge.compat.sodium;

import dev.shaderbridge.model.IdMaps;
import dev.shaderbridge.uniforms.IdMapLookup;
import java.util.Arrays;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.tags.TagKey;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Property;

/**
 * The pack's {@code block.properties} id of every block state, resolved once ({@link IdMapLookup},
 * the same resolution the rest of ShaderBridge uses) and then looked up by the state's numeric id
 * while Sodium meshes chunk sections on its worker threads. Block tags are resolved with the tags
 * bound when the table is built (those of the world being played). Immutable; thread-safe.
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
        IdMapLookup lookup = IdMapLookup.of(maps);
        int[] ids = new int[Block.BLOCK_STATE_REGISTRY.size()];
        Arrays.fill(ids, -1);
        for (BlockState state : Block.BLOCK_STATE_REGISTRY) {
            int index = Block.getId(state);
            if (index >= 0 && index < ids.length) {
                ids[index] = lookup.block(BuiltInRegistries.BLOCK.getKey(state.getBlock()).toString(), name -> propertyValue(state, name),
                    tag -> inTag(state, tag));
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

    private static boolean inTag(BlockState state, String tag) {
        Identifier id = Identifier.tryParse(tag);
        return id != null && state.typeHolder().is(TagKey.create(Registries.BLOCK, id));
    }

    private static String propertyValue(BlockState state, String name) {
        for (Property<?> property : state.getProperties()) {
            if (property.getName().equals(name)) {
                return valueName(state, property);
            }
        }
        return null;
    }

    private static <T extends Comparable<T>> String valueName(BlockState state, Property<T> property) {
        return property.getName(state.getValue(property));
    }
}
