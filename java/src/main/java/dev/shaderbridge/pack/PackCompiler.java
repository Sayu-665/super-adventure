package dev.shaderbridge.pack;

import dev.shaderbridge.model.Diagnostic;
import dev.shaderbridge.model.Severity;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executor;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Compiles packs on one dedicated worker thread. Each {@link #submit} supersedes every earlier
 * request: requests that have not started are skipped, and the result of a compile that finishes
 * after being superseded is discarded (the native compile itself cannot be interrupted). Results
 * are delivered on the main thread through the {@link Listener}.
 */
public final class PackCompiler implements AutoCloseable {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");

    /** Receives compile results on the main thread. */
    public interface Listener {
        /**
         * A compile finished and is the latest request. Ownership of the pack passes to the listener.
         *
         * @param pack the compiled pack
         */
        void onCompiled(LoadedPack pack);

        /**
         * The latest request failed.
         *
         * @param pack   the pack that failed
         * @param reason a message for the user
         */
        void onFailed(PackEntry pack, String reason);
    }

    /** The blocking compile step; replaceable in tests. */
    @FunctionalInterface
    interface Backend {
        LoadedPack compile(CompileRequest request) throws PackException;
    }

    private final ExecutorService worker;
    private final Executor mainThread;
    private final Listener listener;
    private final Backend backend;
    private final AtomicLong generation = new AtomicLong();
    private final AtomicInteger pending = new AtomicInteger();
    private volatile boolean closed;

    /**
     * @param mainThread executor of the game's main thread
     * @param listener   result receiver, called on the main thread
     */
    public PackCompiler(Executor mainThread, Listener listener) {
        this(mainThread, listener, PackCompiler::compile);
    }

    PackCompiler(Executor mainThread, Listener listener, Backend backend) {
        this.mainThread = mainThread;
        this.listener = listener;
        this.backend = backend;
        this.worker = Executors.newSingleThreadExecutor(runnable -> {
            Thread thread = new Thread(runnable, "ShaderBridge Compiler");
            thread.setDaemon(true);
            return thread;
        });
    }

    /**
     * Queues a compile and supersedes every earlier request.
     *
     * @param request what to compile
     */
    public void submit(CompileRequest request) {
        if (closed) {
            throw new IllegalStateException("the compiler is closed");
        }
        long ticket = generation.incrementAndGet();
        pending.incrementAndGet();
        worker.execute(() -> {
            try {
                if (ticket == generation.get()) {
                    run(ticket, request);
                }
            } finally {
                pending.decrementAndGet();
            }
        });
    }

    /** Discards the results of every request submitted so far. */
    public void cancel() {
        generation.incrementAndGet();
    }

    /** @return true while a request is queued or compiling */
    public boolean isBusy() {
        return pending.get() > 0;
    }

    private void run(long ticket, CompileRequest request) {
        PackEntry entry = request.pack();
        long start = System.nanoTime();
        LoadedPack pack;
        try {
            pack = backend.compile(request);
        } catch (PackException e) {
            LOGGER.error("Shader pack {} failed to compile: {}", entry.name(), e.getMessage());
            deliver(ticket, () -> listener.onFailed(entry, e.getMessage()), () -> { });
            return;
        } catch (RuntimeException e) {
            LOGGER.error("Unexpected error while compiling shader pack {}", entry.name(), e);
            deliver(ticket, () -> listener.onFailed(entry, "Unexpected error: " + e), () -> { });
            return;
        }
        LOGGER.info("Compiled shader pack {} in {} ms: {}", entry.name(), TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - start), pack.summary().describe());
        logDiagnostics(pack);
        deliver(ticket, () -> listener.onCompiled(pack), pack::close);
    }

    private static LoadedPack compile(CompileRequest request) throws PackException {
        PackSession session = PackSession.open(request.pack().file());
        try {
            PackSession.CompileResult result = session.compile(request.environment(), request.optionValues().toSettingsText(), request.settings());
            if (request.glslDumpDir() != null) {
                GlslDumper.dump(result.model(), result.blobs(), request.glslDumpDir());
            }
            return new LoadedPack(request.pack().name(), result.model(), result.blobs(), session);
        } catch (PackException | RuntimeException e) {
            session.close();
            throw e;
        }
    }

    /** Runs {@code action} on the main thread if the ticket is still current, {@code discard} otherwise. */
    private void deliver(long ticket, Runnable action, Runnable discard) {
        if (ticket != generation.get() || closed) {
            discard.run();
            return;
        }
        mainThread.execute(() -> {
            if (ticket == generation.get() && !closed) {
                action.run();
            } else {
                discard.run();
            }
        });
    }

    private static void logDiagnostics(LoadedPack pack) {
        for (Diagnostic diagnostic : pack.diagnostics()) {
            if (diagnostic.severity() == Severity.ERROR) {
                LOGGER.error("[{}] {}", pack.name(), diagnostic);
            } else if (diagnostic.severity() == Severity.WARNING) {
                LOGGER.warn("[{}] {}", pack.name(), diagnostic);
            } else {
                LOGGER.debug("[{}] {}", pack.name(), diagnostic);
            }
        }
    }

    /** Stops the worker; results that arrive afterwards are discarded. */
    @Override
    public void close() {
        closed = true;
        cancel();
        worker.shutdown();
    }
}
