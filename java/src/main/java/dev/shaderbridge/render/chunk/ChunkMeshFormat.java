package dev.shaderbridge.render.chunk;

import com.mojang.blaze3d.vertex.BufferBuilder;
import com.mojang.blaze3d.vertex.ByteBufferBuilder;
import com.mojang.blaze3d.vertex.DefaultVertexFormat;
import com.mojang.renderpearl.api.pipeline.PrimitiveTopology;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import dev.shaderbridge.ShaderBridge;
import dev.shaderbridge.render.frame.RenderBridge;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.chunk.ChunkSectionLayer;
import net.minecraft.client.renderer.extract.LevelExtractor;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.FluidState;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * The vertex format Minecraft meshes chunk sections in, and the hooks that apply it.
 *
 * <p><b>Vanilla</b> (no pack renders): every hook returns Minecraft's own values, so meshing and
 * drawing are exactly vanilla. <b>Extended</b> (a pack renders): sections are meshed in
 * {@link TerrainVertexFormat#EXTENDED}, filled with the pack's block ids and the per-quad normal,
 * tangent, texture centre and block offset ({@link ExtendedTerrainBufferBuilder}), and drawn with
 * the extended clones of the terrain pipelines ({@link ExtendedTerrainPipelines}), which ShaderBridge's
 * passes route to the {@code vanilla_terrain} / {@code vanilla_terrain_section_ext} programs.
 *
 * <p>Everything that depends on the format follows one switch: the layers' vertex format and
 * pipelines ({@code ChunkSectionLayer.vertexFormat()} / {@code pipeline(boolean)}), from which
 * Minecraft derives the section builders' format, the vertex size of the section buffer heaps
 * (fixed when its {@code SectionRenderDispatcher} is created) and every draw's base vertex. The
 * switch happens only at the start of {@code LevelExtractor.extract}, together with a full rebuild
 * that releases every section mesh and the section dispatcher, as a world change does; the new
 * dispatcher is created later in the same call. Meshes in one format are therefore never drawn or
 * stored with the other. A new pack whose {@code block.properties} ids differ also rebuilds.
 *
 * <p>The hooks run on the render thread (switch, pipelines) and on Minecraft's meshing threads
 * (builders, block context); the state they read is published through a volatile field.
 */
public final class ChunkMeshFormat {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");

    /** The format sections are meshed in, with the pack block ids of the extended format. */
    private record Mode(boolean extended, BlockIdTable ids, Map<Integer, List<String>> blocks) {
    }

    private static final Mode VANILLA = new Mode(false, BlockIdTable.EMPTY, null);

    private static volatile Mode mode = VANILLA;
    private static volatile String unavailable;
    private static boolean failureLogged;

    private ChunkMeshFormat() {
    }

    /** What the frame hook does. */
    enum Change {
        /** Keep the current format. */
        NONE,
        /** Rebuild every section in Minecraft's format. */
        TO_VANILLA,
        /** Rebuild every section in the extended format with new block ids. */
        TO_EXTENDED
    }

    /**
     * @param extended the sections are meshed in the extended format
     * @param current  the block id map they were meshed with (extended format only)
     * @param wanted   the block id map of the pack that renders, null if none does
     * @return the change that makes the meshes match the pack
     */
    static Change change(boolean extended, Map<Integer, List<String>> current, Map<Integer, List<String>> wanted) {
        if (wanted == null) {
            return extended ? Change.TO_VANILLA : Change.NONE;
        }
        if (extended && (current == wanted || Objects.equals(current, wanted))) {
            return Change.NONE;
        }
        return Change.TO_EXTENDED;
    }

    /** @return whether chunk sections are meshed in the extended format */
    public static boolean extended() {
        return mode.extended();
    }

    /**
     * @return why the extended format cannot be used in this game (a hook it needs did not
     *     apply), or empty
     */
    public static Optional<String> unavailableReason() {
        return Optional.ofNullable(unavailable);
    }

    // ---------------------------------------------------------------------------------------------
    // Frame hook (LevelExtractor.extract HEAD, render thread)
    // ---------------------------------------------------------------------------------------------

    /**
     * Start of {@code LevelExtractor.extract}: switches the format when a pack starts or stops
     * rendering (or its block ids change) and rebuilds every chunk section.
     *
     * @param extractor the level extractor
     */
    public static void beforeExtract(LevelExtractor extractor) {
        try {
            update(extractor);
        } catch (RuntimeException e) {
            if (!failureLogged) {
                failureLogged = true;
                LOGGER.error("Could not switch the chunk mesh format", e);
            }
        }
    }

    private static void update(LevelExtractor extractor) {
        Map<Integer, List<String>> wanted = wantedBlockIds();
        Mode current = mode;
        switch (change(current.extended(), current.blocks(), wanted)) {
            case NONE -> {
                if (current.extended() && current.blocks() != wanted) {
                    // Another pack (or a recompile) with the same ids: compare by identity from now on.
                    mode = new Mode(true, current.ids(), wanted);
                }
            }
            case TO_VANILLA -> apply(VANILLA, extractor);
            case TO_EXTENDED -> {
                if (unavailable == null) {
                    apply(new Mode(true, table(wanted), wanted), extractor);
                }
            }
        }
    }

    /** @return the block id map of the pack that renders, or null */
    private static Map<Integer, List<String>> wantedBlockIds() {
        if (!RenderBridge.packActive()) {
            return null;
        }
        return ShaderBridge.get().activePack().map(pack -> pack.model().idMaps().blocks()).orElse(null);
    }

    private static BlockIdTable table(Map<Integer, List<String>> blocks) {
        try {
            return BlockIdTable.of(blocks);
        } catch (RuntimeException e) {
            LOGGER.error("Could not resolve the shader pack's block.properties ids; terrain is meshed with mc_Entity = -1", e);
            return BlockIdTable.EMPTY;
        }
    }

    private static void apply(Mode next, LevelExtractor extractor) {
        Mode previous = mode;
        mode = next;
        if (next.extended()) {
            String missing = missingHooks();
            if (missing != null) {
                mode = previous;
                unavailable = missing;
                LOGGER.error("ShaderBridge's extended terrain vertex format is unavailable ({}); terrain programs read default normals, "
                    + "block ids and texture centres", missing);
                return;
            }
        }
        try {
            Minecraft.getInstance().levelRenderer.resetLevelRenderData();
        } catch (RuntimeException e) {
            // Keep the format the remaining meshes and the section dispatcher were made for.
            mode = previous;
            unavailable = "releasing the chunk meshes failed: " + e;
            throw e;
        }
        extractor.allChanged();
        if (next.extended()) {
            LOGGER.info("Rebuilding chunk sections in the extended terrain vertex format ({} block states mapped by {} block.properties "
                + "entries, {} entries name unknown blocks or tags)", next.ids().size(), next.ids().entries(), next.ids().unknownEntries());
        } else {
            LOGGER.info("Rebuilding chunk sections in Minecraft's vertex format");
        }
    }

    /** @return the hook that does not apply the extended format, or null if all do */
    private static String missingHooks() {
        for (ChunkSectionLayer layer : ChunkSectionLayer.values()) {
            if (layer.vertexFormat() != TerrainVertexFormat.EXTENDED) {
                return "ChunkSectionLayer.vertexFormat() is not hooked";
            }
            for (boolean multiDraw : new boolean[] {false, true}) {
                if (layer.pipeline(multiDraw).getVertexFormatBinding(0) != TerrainVertexFormat.EXTENDED) {
                    return "ChunkSectionLayer.pipeline(boolean) is not hooked";
                }
            }
        }
        return null;
    }

    // ---------------------------------------------------------------------------------------------
    // Format hooks
    // ---------------------------------------------------------------------------------------------

    /**
     * {@code ChunkSectionLayer.pipeline(boolean)} and the terrain pipeline overrides of
     * {@code ChunkSectionsToRender} (wireframe, order-independent transparency).
     *
     * @param vanilla the pipeline Minecraft would draw chunk sections with (may be null)
     * @return its extended-format clone while the extended format is active, else itself
     */
    public static RenderPipeline pipeline(RenderPipeline vanilla) {
        return mode.extended() ? ExtendedTerrainPipelines.extended(vanilla) : vanilla;
    }

    /**
     * {@code ChunkSectionLayer.vertexFormat()}: the format the section compiler meshes a layer in.
     *
     * @param vanilla the layer's vanilla format
     * @return the extended format instead of {@code BLOCK} while it is active
     */
    public static VertexFormat vertexFormat(VertexFormat vanilla) {
        return mode.extended() && vanilla == DefaultVertexFormat.BLOCK ? TerrainVertexFormat.EXTENDED : vanilla;
    }

    /**
     * The builder {@code SectionCompiler.getOrBeginLayer} creates for a layer.
     *
     * @param buffer   the layer's memory
     * @param topology the primitive topology
     * @param format   the layer's vertex format ({@link #vertexFormat})
     * @return a builder that fills the extension attributes for the extended format, else
     *     Minecraft's builder
     */
    public static BufferBuilder newBuilder(ByteBufferBuilder buffer, PrimitiveTopology topology, VertexFormat format) {
        if (format == TerrainVertexFormat.EXTENDED) {
            return new ExtendedTerrainBufferBuilder(buffer, topology, format, TerrainVertexContext.current(), mode.ids());
        }
        return new BufferBuilder(buffer, topology, format);
    }

    // ---------------------------------------------------------------------------------------------
    // Block context hooks (SectionCompiler.compile, meshing threads)
    // ---------------------------------------------------------------------------------------------

    /**
     * The section compiler read the block it meshes next.
     *
     * @param state the block state
     * @param pos   its position
     */
    public static void enterBlock(BlockState state, BlockPos pos) {
        if (mode.extended()) {
            TerrainVertexContext.current().block(state, pos);
        }
    }

    /**
     * The section compiler is about to mesh the current block's fluid.
     *
     * @param fluid the fluid
     * @return whether {@link #exitFluid()} must follow
     */
    public static boolean enterFluid(FluidState fluid) {
        if (!mode.extended()) {
            return false;
        }
        TerrainVertexContext.current().fluid(fluid.createLegacyBlock());
        return true;
    }

    /** The current block's fluid is meshed; its model follows. */
    public static void exitFluid() {
        TerrainVertexContext.current().endFluid();
    }
}
