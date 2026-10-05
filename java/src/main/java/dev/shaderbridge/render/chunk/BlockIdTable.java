package dev.shaderbridge.render.chunk;

import dev.shaderbridge.model.IdMaps;
import it.unimi.dsi.fastutil.objects.Reference2IntOpenHashMap;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.tags.TagKey;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Property;

/**
 * The {@code block.properties} id ({@code mc_Entity.x}) of every block state of the game, resolved
 * once per pack from its {@code CompiledPack} id maps ({@link BlockIdMapping}) against the block
 * registry and the block tags bound when it is built. Unmapped states have id -1. Immutable once
 * built, so chunk meshing threads read it without locking.
 */
public final class BlockIdTable {
    /** A table without any mapping. */
    public static final BlockIdTable EMPTY = new BlockIdTable(Map.of(), 0, 0);

    private final Reference2IntOpenHashMap<BlockState> ids;
    private final int entries;
    private final int unknownEntries;

    private BlockIdTable(Map<BlockState, Integer> ids, int entries, int unknownEntries) {
        this.ids = new Reference2IntOpenHashMap<>(ids);
        this.ids.defaultReturnValue(TerrainVertexEncoder.NONE);
        this.entries = entries;
        this.unknownEntries = unknownEntries;
    }

    /**
     * Resolves a pack's block ids against the game's block registry and block tags. Call it on the
     * render thread (tags are rebound when a world is joined).
     *
     * @param blocks the pack's block id map ({@link IdMaps#blocks()})
     * @return the table
     */
    public static BlockIdTable of(Map<Integer, List<String>> blocks) {
        BlockIdMapping.Resolution<BlockState> resolved = BlockIdMapping.resolve(blocks, RegistryCatalog.INSTANCE);
        return new BlockIdTable(resolved.ids(), resolved.entries(), resolved.unknownEntries());
    }

    /**
     * @param state a block state
     * @return its pack id, -1 if no entry maps it
     */
    public int id(BlockState state) {
        return ids.getInt(state);
    }

    /** @return the number of mapped block states */
    public int size() {
        return ids.size();
    }

    /** @return the number of entries that name a known block or tag */
    public int entries() {
        return entries;
    }

    /** @return the number of entries that name no known block or tag, or do not parse */
    public int unknownEntries() {
        return unknownEntries;
    }

    /** The game's blocks and block tags. */
    private static final class RegistryCatalog implements BlockIdMapping.Catalog<BlockState> {
        static final RegistryCatalog INSTANCE = new RegistryCatalog();

        @Override
        public List<BlockState> blockStates(String namespace, String path) {
            Identifier id = Identifier.tryBuild(namespace, path);
            if (id == null) {
                return List.of();
            }
            return BuiltInRegistries.BLOCK.get(id).<List<BlockState>>map(holder -> holder.value().getStateDefinition().getPossibleStates())
                .orElse(List.of());
        }

        @Override
        public List<List<BlockState>> tagStates(String namespace, String path) {
            Identifier id = Identifier.tryBuild(namespace, path);
            if (id == null) {
                return List.of();
            }
            List<List<BlockState>> out = new ArrayList<>();
            for (Holder<Block> holder : BuiltInRegistries.BLOCK.getTagOrEmpty(TagKey.create(Registries.BLOCK, id))) {
                out.add(holder.value().getStateDefinition().getPossibleStates());
            }
            return out;
        }

        @Override
        public String value(BlockState state, String property) {
            Property<?> p = state.getBlock().getStateDefinition().getProperty(property);
            return p == null ? null : valueName(state, p);
        }

        private static <T extends Comparable<T>> String valueName(BlockState state, Property<T> property) {
            return property.getName(state.getValue(property));
        }
    }
}
