package dev.shaderbridge.compat.sodium;

import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.textures.GpuSampler;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.ShaderBridge;
import dev.shaderbridge.pack.LoadedPack;
import dev.shaderbridge.render.draw.ActivePasses;
import dev.shaderbridge.render.frame.RenderBridge;
import dev.shaderbridge.render.pipeline.ProfileVertexFormats;
import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import net.caffeinemc.mods.sodium.client.render.SodiumWorldRenderer;
import net.caffeinemc.mods.sodium.client.render.chunk.ChunkRenderMatrices;
import net.caffeinemc.mods.sodium.client.render.chunk.ShaderChunkRenderer;
import net.caffeinemc.mods.sodium.client.render.chunk.vertex.format.ChunkMeshFormats;
import net.caffeinemc.mods.sodium.client.render.chunk.vertex.format.ChunkVertexEncoder;
import net.caffeinemc.mods.sodium.client.render.chunk.vertex.format.ChunkVertexType;
import net.caffeinemc.mods.sodium.client.util.GameRendererStorage;
import net.caffeinemc.mods.sodium.client.util.SodiumChunkSection;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.chunk.ChunkSectionsToRender;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.Vec3;
import org.joml.Matrix4f;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * What the Sodium integration's mixins ({@code dev.shaderbridge.mixin.sodium}) do. Only used when
 * those mixins are applied ({@link SodiumIntegration#active()}).
 *
 * <p><b>Terrain vertex.</b> While a pack is active, Sodium meshes the extended terrain vertex
 * ({@link TerrainVertexLayout}) instead of its own: {@code ChunkMeshFormats.getCurrent()} returns
 * {@link #meshFormat()}. The format is chosen ("latched") when Sodium creates its section manager
 * and chunk builder ({@code SodiumWorldRenderer.initRenderer}), because every buffer of that
 * renderer uses it; when the active pack (or its block ids) changes, Sodium's renderer is reloaded
 * at the start of its next terrain update ({@link #reloadNeeded()}), as Sodium itself does when
 * the render distance changes, and every section is meshed again. Sodium memoizes its terrain
 * pipelines per pass, with the vertex format they were built for; the memo is cleared when the
 * format changes.
 *
 * <p><b>Shadow pass.</b> ShaderBridge's shadow pass asks Minecraft for the chunk sections to draw
 * ({@code LevelRenderer.prepareChunkRenders}); with Sodium, Minecraft has none (Sodium replaced
 * that call in the level render with its own). During a pack frame the call is answered with
 * Sodium's sections instead ({@link #shadowSections()}): Sodium's draw commands for its current,
 * camera-visible render lists, prepared early with the camera's matrices, which Sodium then draws
 * into the shadow pass, where the pipeline substitution binds the pack's shadow programs.
 *
 * <p>Render thread, except the meshing hooks ({@link #enterBlock}, {@link #tagQuad}), which run on
 * Sodium's worker threads.
 */
public final class SodiumTerrain {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");

    private static volatile ExtendedChunkVertex latched;
    private static MeshPlan latchedPlan = MeshPlan.COMPACT;
    private static List<String> layoutProblems;
    private static volatile String extensionFailure;

    private SodiumTerrain() {
    }

    /**
     * @return the vertex type Sodium meshes with, or null for Sodium's own
     *     ({@code ChunkMeshFormats.getCurrent()})
     */
    public static ChunkVertexType meshFormat() {
        return latched;
    }

    /**
     * Chooses the vertex Sodium's new renderer meshes ({@code SodiumWorldRenderer.initRenderer},
     * before the section manager is created).
     */
    public static void latch() {
        MeshPlan plan = desiredPlan();
        ExtendedChunkVertex next = null;
        if (plan.extended()) {
            BlockIdTable ids;
            try {
                ids = BlockIdTable.build(plan.ids());
            } catch (RuntimeException e) {
                LOGGER.warn("Cannot resolve the shader pack's block ids for Sodium's terrain; mc_Entity is -1 for every block", e);
                ids = BlockIdTable.EMPTY;
            }
            next = new ExtendedChunkVertex(ChunkMeshFormats.COMPACT, ids);
        }
        if ((next == null) != (latched == null)) {
            Optional<String> failure = clearPipelineMemo();
            if (failure.isPresent()) {
                // Sodium's memoized pipelines keep the current format: so must its meshes.
                extensionFailure = failure.get();
                LOGGER.error("Shader packs cannot shade Sodium's terrain: {}", extensionFailure);
                return;
            }
            LOGGER.info(next == null ? "Sodium meshes terrain in its own vertex format"
                : "Sodium meshes terrain in ShaderBridge's extended vertex format (normals, block ids, texture centres, mid-block offsets)");
        }
        latched = next;
        latchedPlan = plan;
    }

    /**
     * @return why Sodium's vertex format can no longer follow the active pack, if it cannot
     *     (checked by {@link SodiumCompat#blocker()} every frame)
     */
    static Optional<String> extensionFailure() {
        return Optional.ofNullable(extensionFailure);
    }

    /**
     * Called at the start of every Sodium terrain update ({@code SodiumWorldRenderer.setupTerrain},
     * after chunk events were processed).
     *
     * @return whether Sodium's renderer must be reloaded to mesh another vertex
     */
    public static boolean reloadNeeded() {
        if (extensionFailure != null) {
            return false;
        }
        MeshPlan want = desiredPlan();
        if (want.sameAs(latchedPlan)) {
            latchedPlan = want;
            return false;
        }
        return true;
    }

    /** @return how Sodium should mesh terrain now */
    static MeshPlan desiredPlan() {
        if (!SodiumIntegration.active() || !layoutProblems().isEmpty()) {
            return MeshPlan.COMPACT;
        }
        Optional<LoadedPack> pack;
        try {
            pack = ShaderBridge.get().activePack();
        } catch (IllegalStateException e) {
            return MeshPlan.COMPACT;
        }
        return pack.filter(p -> !p.isClosed()).map(p -> MeshPlan.extended(p.model().idMaps())).orElse(MeshPlan.COMPACT);
    }

    /**
     * Checks the extended vertex against Sodium's compact vertex (the part Sodium writes) and the
     * {@value SodiumPipelines#PROFILE} profile (the part packs read). Computed once.
     *
     * @return what does not match (empty when the extended vertex can be used)
     */
    public static synchronized List<String> layoutProblems() {
        if (layoutProblems == null) {
            List<String> problems = new ArrayList<>();
            try {
                TerrainVertexLayout.sodiumProblems(ChunkMeshFormats.COMPACT.getVertexFormat()).forEach(p -> problems.add("Sodium's chunk vertex: " + p));
                TerrainVertexLayout.profileProblems(ProfileVertexFormats.get().bindings(SodiumPipelines.PROFILE))
                    .forEach(p -> problems.add("draw profile " + SodiumPipelines.PROFILE + ": " + p));
            } catch (RuntimeException | LinkageError e) {
                problems.add("the terrain vertex check failed: " + e);
            }
            layoutProblems = List.copyOf(problems);
        }
        return layoutProblems;
    }

    /** Forgets Sodium's memoized terrain pipelines, which were built for the previous vertex format. */
    private static Optional<String> clearPipelineMemo() {
        try {
            for (String name : List.of("programs", "oitPrograms")) {
                Field field = ShaderChunkRenderer.class.getDeclaredField(name);
                field.setAccessible(true);
                if (!(field.get(null) instanceof Map<?, ?> memo)) {
                    return Optional.of("ShaderChunkRenderer." + name + " is not a map");
                }
                memo.clear();
            }
            return Optional.empty();
        } catch (ReflectiveOperationException | RuntimeException e) {
            return Optional.of("Sodium's terrain pipelines cannot be rebuilt for another vertex format: " + e);
        }
    }

    /**
     * Sodium's chunk sections for ShaderBridge's shadow pass ({@code LevelRenderer.prepareChunkRenders}
     * during a pack frame). Prepares Sodium's draw commands for its current render lists (as
     * Sodium does later in the frame for the main pass, with the same camera matrices: Sodium's
     * per-frame terrain uniforms are written by the first draw of the frame, which is now the
     * shadow pass's). Call outside any render pass.
     *
     * @return the sections, or null when the vanilla answer stands (no pack frame, no Sodium
     *     renderer)
     */
    public static ChunkSectionsToRender shadowSections() {
        if (!RenderBridge.packActive()) {
            return null;
        }
        SodiumWorldRenderer sodium = SodiumWorldRenderer.instanceNullable();
        Minecraft minecraft = Minecraft.getInstance();
        if (sodium == null || !(minecraft.gameRenderer instanceof GameRendererStorage storage)) {
            return null;
        }
        CameraRenderState camera = minecraft.gameRenderer.gameRenderState().levelRenderState.cameraRenderState;
        ChunkRenderMatrices matrices = new ChunkRenderMatrices(new Matrix4f(storage.sodium$getProjectionMatrix()), new Matrix4f(camera.viewRotationMatrix));
        Vec3 position = camera.pos;
        sodium.prepareChunkRendering(matrices, position.x, position.y, position.z);
        return new SodiumChunkSection(sodium, matrices, position.x, position.y, position.z);
    }

    /**
     * Before Sodium draws a terrain pass into a render pass: in a ShaderBridge pass, binds the
     * block atlas as the draw's albedo ({@code Sampler0}, which ShaderBridge sizes {@code sb_Draw}
     * and resolves unnamed pack samplers by), as Minecraft's terrain draws do. Sodium binds its
     * own sampler names ({@code u_BlockTex}) only after its pipeline.
     *
     * @param pass    the render pass
     * @param atlas   the block atlas
     * @param sampler Sodium's terrain sampler
     */
    public static void bindAlbedo(RenderPass pass, GpuTextureView atlas, GpuSampler sampler) {
        if (ActivePasses.owns(pass)) {
            pass.setUniform(ActivePasses.ALBEDO_SAMPLER, atlas, sampler);
        }
    }

    /**
     * Starts meshing one block on the calling (worker) thread.
     *
     * @param idState the block state whose {@code block.properties} id the quads carry (for a
     *                fluid, the fluid's own block, e.g. {@code minecraft:water} in a waterlogged
     *                stair)
     * @param state   the block state at the position (its light emission goes to
     *                {@code at_midBlock.w}, for fluids too, as in Iris)
     * @param origin  the block's position within its section
     * @param fluid   whether the block's fluid is meshed
     * @return the context to {@link #exitBlock}, or null when the extended vertex is not in use
     */
    public static Object enterBlock(BlockState idState, BlockState state, BlockPos origin, boolean fluid) {
        ExtendedChunkVertex type = latched;
        if (type == null) {
            return null;
        }
        BlockContext context = BlockContext.current();
        context.enter(TerrainExtension.entity(type.ids().id(idState), fluid),
            TerrainExtension.block(origin.getX(), origin.getY(), origin.getZ(), state.getLightEmission()));
        return context;
    }

    /**
     * Ends meshing a block.
     *
     * @param context what {@link #enterBlock} returned
     */
    public static void exitBlock(Object context) {
        if (context instanceof BlockContext block) {
            block.exit();
        }
    }

    /**
     * Tags the vertices of a translucent quad entering Sodium's sorter with its block, so that
     * pieces of it Sodium splits off and encodes later keep the block's data ({@link VertexTags}).
     *
     * @param vertices the quad's vertices
     */
    public static void tagQuad(ChunkVertexEncoder.Vertex[] vertices) {
        ExtendedChunkVertex type = latched;
        if (type == null) {
            return;
        }
        BlockContext context = BlockContext.current();
        int entity = context.active() ? context.entity() : 0;
        int block = context.active() ? context.block() : TerrainExtension.NO_BLOCK;
        int midTexCoord = ExtendedChunkVertex.midTexCoord(vertices);
        for (ChunkVertexEncoder.Vertex vertex : vertices) {
            if (vertex instanceof VertexTags tags) {
                tags.shaderbridge$tag(entity, block, midTexCoord);
            }
        }
    }
}
