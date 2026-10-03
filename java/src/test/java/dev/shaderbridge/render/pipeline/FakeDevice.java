package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import com.mojang.renderpearl.api.pipeline.ShaderSource;
import com.mojang.renderpearl.api.pipeline.ShaderType;
import java.lang.reflect.Proxy;
import java.nio.ByteBuffer;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import net.minecraft.resources.Identifier;

/**
 * A {@link GpuDevice} whose {@code compilePipeline} does what Mojang's pipeline builder does with
 * ShaderBridge pipelines, minus the GPU: it asks the shader source for every stage, then runs the
 * compiler hook (or not, to simulate an inactive hook). Compiles complete when
 * {@link #completeAll()} is called.
 */
final class FakeDevice {
    /** A compiled pipeline. */
    static final class Compiled implements CompiledRenderPipeline {
        final RenderPipeline pipeline;
        boolean closed;

        Compiled(RenderPipeline pipeline) {
            this.pipeline = pipeline;
        }

        @Override
        public boolean isClosed() {
            return closed;
        }

        @Override
        public void close() {
            closed = true;
        }
    }

    private final SpirvModules modules;
    private final boolean hookActive;
    private final List<CompletableFuture<CompiledRenderPipeline.Pending>> futures = new ArrayList<>();
    private final List<Runnable> work = new ArrayList<>();
    final List<Compiled> compiled = new ArrayList<>();

    FakeDevice(SpirvModules modules, boolean hookActive) {
        this.modules = modules;
        this.hookActive = hookActive;
    }

    GpuDevice device() {
        return (GpuDevice) Proxy.newProxyInstance(GpuDevice.class.getClassLoader(), new Class<?>[] {GpuDevice.class}, (proxy, method, args) -> {
            if (method.getName().equals("compilePipeline")) {
                return compile((RenderPipeline) args[0], (ShaderSource) args[1]);
            }
            throw new UnsupportedOperationException(method.getName());
        });
    }

    private CompletableFuture<CompiledRenderPipeline.Pending> compile(RenderPipeline pipeline, ShaderSource source) {
        CompletableFuture<CompiledRenderPipeline.Pending> future = new CompletableFuture<>();
        futures.add(future);
        work.add(() -> {
            boolean ok = true;
            for (Map.Entry<ShaderType, Identifier> shader : pipeline.getShaders().entrySet()) {
                if (source.getShader(shader.getValue(), shader.getKey()) == null) {
                    ok = false;
                    continue;
                }
                ByteBuffer spirv = hookActive ? modules.copyForCompiler(shader.getValue().toString(), ByteBuffer::allocateDirect) : null;
                ok &= spirv != null;
            }
            boolean success = ok;
            future.complete(() -> {
                if (!success) {
                    return null;
                }
                Compiled c = new Compiled(pipeline);
                compiled.add(c);
                return c;
            });
        });
        return future;
    }

    /** Runs every pending background compile. */
    void completeAll() {
        List<Runnable> pending = new ArrayList<>(work);
        work.clear();
        pending.forEach(Runnable::run);
    }
}
