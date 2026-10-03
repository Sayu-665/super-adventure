package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.pipeline.ShaderSource;
import com.mojang.renderpearl.api.pipeline.ShaderType;
import net.minecraft.resources.Identifier;

/**
 * The {@link ShaderSource} pack pipelines compile with. Mojang's pipeline builder asks it for GLSL
 * before calling the compiler; the compiler hook then substitutes the registered SPIR-V. The text
 * returned here is only compiled if the hook did not run, and then fails with a clear message.
 */
public final class PackShaderSource implements ShaderSource {
    private final SpirvModules modules;

    /** @param modules the registry the compiler hook reads */
    public PackShaderSource(SpirvModules modules) {
        this.modules = modules;
    }

    @Override
    public String getShader(Identifier id, ShaderType type) {
        return modules.contains(id) ? placeholder(id) : null;
    }

    @Override
    public CachedIncludeSource getInclude(Identifier id) {
        return null;
    }

    @Override
    public void close() {
        // Nothing to release: the modules belong to their pipelines.
    }

    /**
     * @param id a module id
     * @return GLSL that fails to compile with a message naming the inactive hook
     */
    static String placeholder(Identifier id) {
        return "#version 450\n#error ShaderBridge: the SPIR-V injection hook did not handle " + id + "\nvoid main() {}\n";
    }
}
