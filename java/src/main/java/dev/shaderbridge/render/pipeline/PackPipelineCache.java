package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.api.pipeline.ShaderSource;
import java.util.HashMap;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.Executor;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Compiles pack pipelines asynchronously, one per {@link PipelineKey}. Mojang's
 * {@link GpuDevice#compilePipeline} runs the front end and backend compile on a background executor;
 * {@link #poll()} finishes completed compiles on the render thread, where
 * {@code Pending.finishCompile} must run. A pipeline that fails stays failed (with its reason) until
 * the cache is closed. Render thread only, except where noted.
 */
public final class PackPipelineCache implements AutoCloseable {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");

    private final GpuDevice device;
    private final ShaderSource source;
    private final Executor compileExecutor;
    private final Executor renderThread;
    private final SpirvModules modules;
    private final Map<PipelineKey, Entry> entries = new HashMap<>();
    private boolean injectionInactive;
    private boolean closed;

    /** State of one pipeline. */
    public sealed interface State {
        /** Never requested. */
        record Missing() implements State {
        }

        /** Being compiled. */
        record Compiling() implements State {
        }

        /** @param pipeline the compiled pipeline, owned by the cache */
        record Ready(CompiledRenderPipeline pipeline) implements State {
        }

        /** @param reason why it did not compile */
        record Failed(String reason) implements State {
        }
    }

    private static final class Entry {
        final PackPipeline pipeline;
        CompletableFuture<CompiledRenderPipeline.Pending> future;
        State state = new State.Compiling();

        Entry(PackPipeline pipeline) {
            this.pipeline = pipeline;
        }
    }

    /**
     * @param device          the GPU device
     * @param source          shader source for pack pipelines ({@link PackShaderSource})
     * @param compileExecutor background executor for compiles
     * @param renderThread    executes tasks on the render thread (late compiles finished after close)
     * @param modules         the registry the compiler hook reads
     */
    public PackPipelineCache(GpuDevice device, ShaderSource source, Executor compileExecutor, Executor renderThread, SpirvModules modules) {
        this.device = device;
        this.source = source;
        this.compileExecutor = compileExecutor;
        this.renderThread = renderThread;
        this.modules = modules;
    }

    /**
     * Starts compiling a pipeline unless its key is already known.
     *
     * @param pipeline a pipeline from {@link PackPipelineFactory}; ownership of its modules passes
     *                 to the cache
     * @return the pipeline's state
     */
    public State request(PackPipeline pipeline) {
        if (closed) {
            throw new IllegalStateException("the pipeline cache is closed");
        }
        Entry existing = entries.get(pipeline.key());
        if (existing != null) {
            if (existing.pipeline != pipeline) {
                pipeline.modules().forEach(modules::release);
            }
            return existing.state;
        }
        Entry entry = new Entry(pipeline);
        entries.put(pipeline.key(), entry);
        try {
            entry.future = device.compilePipeline(pipeline.pipeline(), source, compileExecutor);
        } catch (RuntimeException e) {
            finish(entry, new State.Failed("compilePipeline threw " + e));
        }
        return entry.state;
    }

    /**
     * @param key a pipeline key
     * @return its state
     */
    public State state(PipelineKey key) {
        Entry entry = entries.get(key);
        return entry == null ? new State.Missing() : entry.state;
    }

    /**
     * @param key a pipeline key
     * @return the pipeline description of a requested key, if any
     */
    public java.util.Optional<PackPipeline> pipeline(PipelineKey key) {
        Entry entry = entries.get(key);
        return entry == null ? java.util.Optional.empty() : java.util.Optional.of(entry.pipeline);
    }

    /** Finishes every compile whose background part is done. Render thread. */
    public void poll() {
        for (Entry entry : entries.values()) {
            if (entry.state instanceof State.Compiling && entry.future != null && entry.future.isDone()) {
                finish(entry, complete(entry));
            }
        }
    }

    /**
     * @return true once a pipeline failed although its modules were never handed to the compiler:
     *     the SPIR-V injection hook is not active and pack pipelines cannot work
     */
    public boolean injectionInactive() {
        return injectionInactive;
    }

    private State complete(Entry entry) {
        CompiledRenderPipeline.Pending pending;
        try {
            pending = entry.future.join();
        } catch (RuntimeException e) {
            return new State.Failed("compile failed: " + e.getMessage());
        }
        CompiledRenderPipeline compiled = pending.finishCompile();
        if (compiled != null) {
            return new State.Ready(compiled);
        }
        if (entry.pipeline.modules().stream().noneMatch(modules::served)) {
            injectionInactive = true;
            return new State.Failed("the SPIR-V injection hook did not run");
        }
        return new State.Failed("Mojang's pipeline builder rejected it (see the log)");
    }

    private void finish(Entry entry, State state) {
        entry.state = state;
        entry.pipeline.modules().forEach(modules::release);
        if (state instanceof State.Failed failed) {
            LOGGER.warn("Pipeline {} is unavailable: {}", entry.pipeline.pipeline().getLocation(), failed.reason());
        }
    }

    /** Closes every compiled pipeline; compiles still running are closed when they finish. */
    @Override
    public void close() {
        closed = true;
        for (Entry entry : entries.values()) {
            switch (entry.state) {
                case State.Ready ready -> ready.pipeline().close();
                case State.Compiling compiling -> {
                    entry.pipeline.modules().forEach(modules::release);
                    if (entry.future != null) {
                        entry.future.thenAcceptAsync(pending -> {
                            CompiledRenderPipeline late = pending.finishCompile();
                            if (late != null) {
                                late.close();
                            }
                        }, renderThread);
                    }
                }
                default -> {
                }
            }
        }
        entries.clear();
    }
}
