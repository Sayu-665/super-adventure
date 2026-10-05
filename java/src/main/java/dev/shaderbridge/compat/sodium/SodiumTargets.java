package dev.shaderbridge.compat.sodium;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.function.Function;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.tree.AbstractInsnNode;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.FieldNode;
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;

/**
 * Everything of Sodium (and of the game and ShaderBridge) that the Sodium integration hooks or
 * calls, as class-file members: the methods its mixins inject into, the call sites they wrap, the
 * fields they access, and the members {@link SodiumTerrain} calls directly. The list is checked
 * against the class files <em>before</em> any of the integration's mixins is applied
 * ({@code SodiumMixinPlugin}), so that a Sodium version that moved one of them leaves Sodium
 * untouched and makes ShaderBridge refuse packs with a message instead of failing in a mixin or
 * with a {@link LinkageError} in a frame. The unit tests run the same check against the Sodium
 * 0.9 jar the build compiles against.
 *
 * <p>The selector and descriptor constants are compile-time constants, so the mixins use the
 * very strings this class checks.
 *
 * <p>Verified with {@code javap -p -s -c} against Sodium {@code 0.9.3-alpha.1+mc26.3} and
 * {@code 0.9.2+mc26.3} (identical for every entry) and the Minecraft 26.3 jar.
 */
public final class SodiumTargets {
    private static final String SODIUM = "net/caffeinemc/mods/sodium/client/";

    /** {@code ChunkMeshFormats}: the chunk vertex format Sodium meshes with. */
    public static final String CHUNK_MESH_FORMATS = SODIUM + "render/chunk/vertex/format/ChunkMeshFormats";
    /** {@code ChunkVertexType}: a chunk vertex format and its encoder. */
    public static final String CHUNK_VERTEX_TYPE = SODIUM + "render/chunk/vertex/format/ChunkVertexType";
    /** {@code ChunkVertexEncoder}: writes the vertices of a quad. */
    public static final String CHUNK_VERTEX_ENCODER = SODIUM + "render/chunk/vertex/format/ChunkVertexEncoder";
    /** {@code ChunkVertexEncoder.Vertex}: a vertex before encoding. */
    public static final String VERTEX = CHUNK_VERTEX_ENCODER + "$Vertex";
    /** {@code SodiumWorldRenderer}: Sodium's level renderer. */
    public static final String WORLD_RENDERER = SODIUM + "render/SodiumWorldRenderer";
    /** {@code ShaderChunkRenderer}: builds Sodium's terrain pipelines. */
    public static final String SHADER_CHUNK_RENDERER = SODIUM + "render/chunk/ShaderChunkRenderer";
    /** {@code DefaultChunkRenderer}: draws Sodium's terrain. */
    public static final String DEFAULT_CHUNK_RENDERER = SODIUM + "render/chunk/DefaultChunkRenderer";
    /** {@code TranslucentGeometryCollector}: keeps translucent quads for sorting. */
    public static final String GEOMETRY_COLLECTOR = SODIUM + "render/chunk/translucent_sorting/TranslucentGeometryCollector";
    /** {@code ChunkBuilderMeshingTask}: meshes one chunk section. */
    public static final String MESHING_TASK = SODIUM + "render/chunk/compile/tasks/ChunkBuilderMeshingTask";
    /** {@code BlockRenderer}: meshes a block model. */
    public static final String BLOCK_RENDERER = SODIUM + "render/chunk/compile/pipeline/BlockRenderer";
    /** {@code FluidRenderer}: meshes a fluid. */
    public static final String FLUID_RENDERER = SODIUM + "render/chunk/compile/pipeline/FluidRenderer";
    /** {@code SodiumChunkSection}: Sodium's {@code ChunkSectionsToRender}. */
    public static final String CHUNK_SECTION = SODIUM + "util/SodiumChunkSection";
    /** {@code ChunkRenderMatrices}: projection and model-view of a terrain draw. */
    public static final String RENDER_MATRICES = SODIUM + "render/chunk/ChunkRenderMatrices";
    /** {@code GameRendererStorage}: Sodium's interface on {@code GameRenderer}. */
    public static final String GAME_RENDERER_STORAGE = SODIUM + "util/GameRendererStorage";
    /** {@code TerrainRenderPass}: solid, cutout or translucent terrain. */
    public static final String TERRAIN_RENDER_PASS = SODIUM + "render/chunk/terrain/TerrainRenderPass";
    /** Minecraft's {@code LevelRenderer}. */
    public static final String LEVEL_RENDERER = "net/minecraft/client/renderer/LevelRenderer";
    /** ShaderBridge's vanilla pipeline table, extended with Sodium's pipelines. */
    public static final String PIPELINE_TABLE = "dev/shaderbridge/render/mapping/VanillaPipelineTable";

    private static final String D_VERTEX = "L" + VERTEX + ";";
    private static final String D_FOG = "L" + SODIUM + "util/FogParameters;";

    /** {@code ChunkMeshFormats.getCurrent()}. */
    public static final String GET_CURRENT = "getCurrent()L" + CHUNK_VERTEX_TYPE + ";";
    /** {@code SodiumWorldRenderer.initRenderer()}: creates the section manager and the chunk builder. */
    public static final String INIT_RENDERER = "initRenderer()V";
    /** {@code SodiumWorldRenderer.setupTerrain(...)}: the per-frame terrain update. */
    public static final String SETUP_TERRAIN = "setupTerrain(Lnet/minecraft/client/Camera;L" + SODIUM + "render/viewport/Viewport;" + D_FOG
        + "ZZLorg/joml/Matrix4f;)V";
    /** The call in {@link #SETUP_TERRAIN} after which Sodium itself may reload its renderer. */
    public static final String PROCESS_CHUNK_EVENTS = "L" + WORLD_RENDERER + ";processChunkEvents()V";
    /** {@code ChunkVertexEncoder.Vertex.copyVertexTo(Vertex, Vertex)}. */
    public static final String COPY_VERTEX_TO = "copyVertexTo(" + D_VERTEX + D_VERTEX + ")V";
    /** {@code TranslucentGeometryCollector.appendQuad(Vertex[], ModelQuadFacing, int)}. */
    public static final String APPEND_QUAD = "appendQuad([" + D_VERTEX + "L" + SODIUM + "model/quad/properties/ModelQuadFacing;I)Z";
    /** {@code ChunkBuilderMeshingTask.execute(...)} (not its bridge method). */
    public static final String EXECUTE = "execute(L" + SODIUM + "render/chunk/compile/ChunkBuildContext;L" + SODIUM
        + "util/task/CancellationToken;)L" + SODIUM + "render/chunk/compile/ChunkBuildOutput;";
    /** The block model call in {@link #EXECUTE}. */
    public static final String RENDER_MODEL = "L" + BLOCK_RENDERER + ";renderModel(Lnet/minecraft/client/renderer/block/dispatch/BlockStateModel;"
        + "Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;)V";
    /** The fluid call in {@link #EXECUTE}. */
    public static final String RENDER_FLUID = "L" + FLUID_RENDERER + ";render(L" + SODIUM + "world/LevelSlice;"
        + "Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/material/FluidState;Lnet/minecraft/core/BlockPos;"
        + "Lnet/minecraft/core/BlockPos;L" + GEOMETRY_COLLECTOR + ";L" + SODIUM + "render/chunk/compile/ChunkBuildBuffers;)V";
    /** {@code DefaultChunkRenderer.render(...)}: draws one terrain pass into a render pass. */
    public static final String DRAW_TERRAIN = "render(L" + RENDER_MATRICES + ";L" + SODIUM + "render/chunk/lists/ChunkRenderListIterable;L"
        + TERRAIN_RENDER_PASS + ";L" + SODIUM + "render/viewport/CameraTransform;" + D_FOG + "ZLcom/mojang/renderpearl/api/commands/RenderPass;"
        + "Lcom/mojang/renderpearl/api/textures/GpuSampler;Lcom/mojang/renderpearl/api/buffers/GpuBufferSlice;"
        + "Lcom/mojang/renderpearl/api/buffers/GpuBuffer;Lnet/minecraft/client/renderer/oit/OitStage;)V";
    /** {@code LevelRenderer.prepareChunkRenders(Matrix4fc, boolean)}. */
    public static final String PREPARE_CHUNK_RENDERS = "prepareChunkRenders(Lorg/joml/Matrix4fc;Z)Lnet/minecraft/client/renderer/chunk/ChunkSectionsToRender;";
    /** {@code VanillaPipelineTable.lookup(Identifier)}. */
    public static final String TABLE_LOOKUP = "lookup(Lnet/minecraft/resources/Identifier;)Ldev/shaderbridge/render/mapping/PipelineMapping;";

    /** Reads class files without loading the classes. */
    @FunctionalInterface
    public interface ClassSource {
        /**
         * @param internalName a class name such as {@code net/minecraft/client/Camera}
         * @return the class file, or null if there is no such class
         */
        ClassNode read(String internalName);
    }

    /** One member the integration needs. */
    public sealed interface Requirement {
        /** @return the class that declares it */
        String owner();

        /** @return a human-readable description */
        String describe();
    }

    /**
     * A method.
     *
     * @param owner      declaring class
     * @param selector   name and descriptor, {@code name(args)ret}
     * @param staticKind whether it must be static
     */
    public record Method(String owner, String selector, boolean staticKind) implements Requirement {
        @Override
        public String describe() {
            return simpleName(owner) + "." + selector;
        }
    }

    /**
     * A field.
     *
     * @param owner      declaring class
     * @param name       field name
     * @param descriptor field descriptor
     * @param staticKind whether it must be static
     */
    public record Field(String owner, String name, String descriptor, boolean staticKind) implements Requirement {
        @Override
        public String describe() {
            return simpleName(owner) + "." + name + ":" + descriptor;
        }
    }

    /**
     * A call site inside a method.
     *
     * @param owner    class of the calling method
     * @param selector calling method, {@code name(args)ret}
     * @param target   the called method, {@code Lowner;name(args)ret}
     */
    public record Call(String owner, String selector, String target) implements Requirement {
        @Override
        public String describe() {
            return "the call to " + simpleName(target.substring(1, target.indexOf(';'))) + "." + target.substring(target.indexOf(';') + 1) + " in "
                + simpleName(owner) + "." + selector;
        }
    }

    /** Every requirement, in the order the integration uses them. */
    public static final List<Requirement> ALL = List.of(
        // The extended terrain vertex format (SodiumTerrain, ExtendedChunkVertex).
        new Method(CHUNK_MESH_FORMATS, GET_CURRENT, true),
        new Field(CHUNK_MESH_FORMATS, "COMPACT", "L" + CHUNK_VERTEX_TYPE + ";", true),
        new Method(CHUNK_VERTEX_TYPE, "getVertexFormat()Lcom/mojang/renderpearl/api/vertex/VertexFormat;", false),
        new Method(CHUNK_VERTEX_TYPE, "getEncoder()L" + CHUNK_VERTEX_ENCODER + ";", false),
        new Method(CHUNK_VERTEX_ENCODER, "write(JI[" + D_VERTEX + "I)J", false),
        new Field(VERTEX, "x", "F", false),
        new Field(VERTEX, "y", "F", false),
        new Field(VERTEX, "z", "F", false),
        new Field(VERTEX, "u", "F", false),
        new Field(VERTEX, "v", "F", false),
        new Method(VERTEX, COPY_VERTEX_TO, true),
        new Method(WORLD_RENDERER, INIT_RENDERER, false),
        new Call(WORLD_RENDERER, SETUP_TERRAIN, PROCESS_CHUNK_EVENTS),
        new Method(WORLD_RENDERER, "reload()V", false),
        new Field(SHADER_CHUNK_RENDERER, "programs", "Ljava/util/Map;", true),
        new Field(SHADER_CHUNK_RENDERER, "oitPrograms", "Ljava/util/Map;", true),
        // Block ids, mid-block offsets and light emission per vertex.
        new Call(MESHING_TASK, EXECUTE, RENDER_MODEL),
        new Call(MESHING_TASK, EXECUTE, RENDER_FLUID),
        new Method(GEOMETRY_COLLECTOR, APPEND_QUAD, false),
        // The albedo of terrain draws in ShaderBridge's passes.
        new Method(DEFAULT_CHUNK_RENDERER, DRAW_TERRAIN, false),
        new Method(TERRAIN_RENDER_PASS, "getAtlas()Lcom/mojang/renderpearl/api/textures/GpuTextureView;", false),
        // The shadow pass.
        new Method(LEVEL_RENDERER, PREPARE_CHUNK_RENDERS, false),
        new Method(WORLD_RENDERER, "instanceNullable()L" + WORLD_RENDERER + ";", true),
        new Method(WORLD_RENDERER, "prepareChunkRendering(L" + RENDER_MATRICES + ";DDD)V", false),
        new Method(RENDER_MATRICES, "<init>(Lorg/joml/Matrix4fc;Lorg/joml/Matrix4fc;)V", false),
        new Method(CHUNK_SECTION, "<init>(L" + WORLD_RENDERER + ";L" + RENDER_MATRICES + ";DDD)V", false),
        new Method(GAME_RENDERER_STORAGE, "sodium$getProjectionMatrix()Lorg/joml/Matrix4fc;", false),
        // Pipeline routing.
        new Method(PIPELINE_TABLE, TABLE_LOOKUP, true));

    private SodiumTargets() {
    }

    /**
     * Checks every requirement.
     *
     * @param classes reads class files
     * @return what is missing (empty when the integration can be applied)
     */
    public static List<String> problems(ClassSource classes) {
        Map<String, ClassNode> cache = new HashMap<>();
        Function<String, ClassNode> read = name -> cache.computeIfAbsent(name, classes::read);
        List<String> problems = new ArrayList<>();
        for (Requirement requirement : ALL) {
            ClassNode node = read.apply(requirement.owner());
            if (node == null) {
                problems.add("class " + requirement.owner().replace('/', '.') + " is missing (" + requirement.describe() + ")");
                continue;
            }
            boolean found = switch (requirement) {
                case Method m -> method(node, m.selector()) instanceof MethodNode method && isStatic(method.access) == m.staticKind();
                case Field f -> field(node, f.name(), f.descriptor()) instanceof FieldNode field && isStatic(field.access) == f.staticKind();
                case Call c -> method(node, c.selector()) instanceof MethodNode method && calls(method, c.target());
            };
            if (!found) {
                problems.add(requirement.describe() + " is missing");
            }
        }
        return problems;
    }

    private static MethodNode method(ClassNode node, String selector) {
        int paren = selector.indexOf('(');
        String name = selector.substring(0, paren);
        String descriptor = selector.substring(paren);
        for (MethodNode method : node.methods) {
            if (method.name.equals(name) && method.desc.equals(descriptor)) {
                return method;
            }
        }
        return null;
    }

    private static FieldNode field(ClassNode node, String name, String descriptor) {
        for (FieldNode field : node.fields) {
            if (field.name.equals(name) && field.desc.equals(descriptor)) {
                return field;
            }
        }
        return null;
    }

    private static boolean calls(MethodNode method, String target) {
        int semicolon = target.indexOf(';');
        String owner = target.substring(1, semicolon);
        String selector = target.substring(semicolon + 1);
        int paren = selector.indexOf('(');
        String name = selector.substring(0, paren);
        String descriptor = selector.substring(paren);
        if (method.instructions == null) {
            return false;
        }
        for (AbstractInsnNode insn : method.instructions) {
            if (insn instanceof MethodInsnNode call && call.owner.equals(owner) && call.name.equals(name) && call.desc.equals(descriptor)) {
                return true;
            }
        }
        return false;
    }

    private static boolean isStatic(int access) {
        return (access & Opcodes.ACC_STATIC) != 0;
    }

    private static String simpleName(String internalName) {
        return internalName.substring(internalName.lastIndexOf('/') + 1).replace('$', '.');
    }
}
