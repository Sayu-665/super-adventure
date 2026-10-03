package dev.shaderbridge.render.pipeline;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.IntFunction;
import net.minecraft.resources.Identifier;

/**
 * SPIR-V modules waiting to be compiled into pipelines, by shader id
 * ({@code shaderbridge:spv/<serial>}). Ids are serial numbers rather than blob ids because a pack's
 * main blobs and its later variants live in separate blob buffers. The {@code GlslCompiler} hook
 * ({@code dev.shaderbridge.mixin.GlslCompilerMixin}) asks for a copy of a module instead of
 * compiling GLSL. Thread-safe: pipelines compile on Minecraft's background executor.
 */
public final class SpirvModules {
    /** Path prefix of every module id. */
    public static final String PATH_PREFIX = "spv/";

    private static final SpirvModules GLOBAL = new SpirvModules();

    private final Map<String, ByteBuffer> modules = new ConcurrentHashMap<>();
    private final Set<String> served = ConcurrentHashMap.newKeySet();
    private final AtomicLong serial = new AtomicLong();

    /** @return the registry the compiler hook reads */
    public static SpirvModules global() {
        return GLOBAL;
    }

    /**
     * Registers a module. The buffer is not copied: it must stay unchanged until
     * {@link #release(Identifier)}.
     *
     * @param spirv SPIR-V words (little-endian) from position to limit
     * @return the module's shader id
     */
    public Identifier register(ByteBuffer spirv) {
        Identifier id = Identifier.fromNamespaceAndPath(PipelineKey.NAMESPACE, PATH_PREFIX + serial.incrementAndGet());
        modules.put(id.toString(), spirv.slice().order(ByteOrder.LITTLE_ENDIAN).asReadOnlyBuffer());
        return id;
    }

    /**
     * Forgets a module (after its pipeline compiled, or when the pack is unloaded).
     *
     * @param id a module id; unknown ids are ignored
     */
    public void release(Identifier id) {
        modules.remove(id.toString());
        served.remove(id.toString());
    }

    /**
     * @param id a shader id
     * @return whether a module is registered under it
     */
    public boolean contains(Identifier id) {
        return modules.containsKey(id.toString());
    }

    /**
     * @param id a module id
     * @return whether the compiler hook has handed out the module (the hook is active)
     */
    public boolean served(Identifier id) {
        return served.contains(id.toString());
    }

    /**
     * For the compiler hook: a copy of a registered module in memory from {@code allocator}, which
     * the caller owns ({@code SPIRVModule} frees it with {@code MemoryUtil.memFree}).
     *
     * @param shaderId  the shader id as Mojang passes it ({@code Identifier.toString()})
     * @param allocator allocates a direct buffer of the given size
     * @return the copy (position 0, limit = size), or null if no module has that id
     */
    public ByteBuffer copyForCompiler(String shaderId, IntFunction<ByteBuffer> allocator) {
        ByteBuffer module = modules.get(shaderId);
        if (module == null) {
            return null;
        }
        ByteBuffer copy = allocator.apply(module.remaining());
        copy.put(module.duplicate()).flip();
        served.add(shaderId);
        return copy;
    }

    /** @return the number of registered modules */
    public int size() {
        return modules.size();
    }
}
