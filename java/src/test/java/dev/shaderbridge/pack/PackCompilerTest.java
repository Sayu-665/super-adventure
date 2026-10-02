package dev.shaderbridge.pack;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.config.PackOptionValues;
import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.CompiledPack;
import dev.shaderbridge.model.IdMaps;
import dev.shaderbridge.model.OptionsModel;
import dev.shaderbridge.model.PackInfo;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.concurrent.TimeUnit;
import org.junit.jupiter.api.Test;

class PackCompilerTest {
    private final LinkedBlockingQueue<Runnable> mainThread = new LinkedBlockingQueue<>();
    private final List<String> delivered = new CopyOnWriteArrayList<>();
    private final Map<String, LoadedPack> built = new ConcurrentHashMap<>();

    private final PackCompiler.Listener listener = new PackCompiler.Listener() {
        @Override
        public void onCompiled(LoadedPack pack) {
            delivered.add("compiled " + pack.name());
        }

        @Override
        public void onFailed(PackEntry pack, String reason) {
            delivered.add("failed " + pack.name() + ": " + reason);
        }
    };

    private static CompileRequest request(String name) {
        PackEntry entry = new PackEntry(name, "/packs/" + name, PackKind.DIR, true, null);
        return new CompileRequest(entry, null, PackOptionValues.empty(), new CompileSettings(null, false, null, 0), null);
    }

    private LoadedPack build(CompileRequest request) {
        String name = request.pack().name();
        CompiledPack model = new CompiledPack(1, new PackInfo(name, "", "0", List.of(), List.of(), null),
            new OptionsModel(null, null, null, null, null, null, null, null, null), new IdMaps(null, null, null, null, null), null, null, null);
        LoadedPack pack = new LoadedPack(name, model, Blobs.empty(), PackSession.detached(Path.of(name)));
        built.put(name, pack);
        return pack;
    }

    /** Runs main-thread tasks until {@code count} results arrived or the compiler went idle. */
    private void drain(PackCompiler compiler, int count) throws InterruptedException {
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(10);
        while (delivered.size() < count && System.nanoTime() < deadline) {
            Runnable task = mainThread.poll(10, TimeUnit.MILLISECONDS);
            if (task != null) {
                task.run();
            } else if (!compiler.isBusy() && mainThread.isEmpty() && count == 0) {
                return;
            }
        }
    }

    @Test
    void newerRequestsSupersedeOlderOnes() throws Exception {
        CountDownLatch started = new CountDownLatch(1);
        CountDownLatch release = new CountDownLatch(1);
        PackCompiler compiler = new PackCompiler(mainThread::add, listener, request -> {
            if (request.pack().name().equals("A")) {
                started.countDown();
                try {
                    release.await();
                } catch (InterruptedException e) {
                    throw new PackException("interrupted", e);
                }
            }
            return build(request);
        });
        compiler.submit(request("A"));
        assertTrue(started.await(10, TimeUnit.SECONDS));
        compiler.submit(request("B"));
        compiler.submit(request("C"));
        release.countDown();
        drain(compiler, 1);
        compiler.close();

        assertEquals(List.of("compiled C"), delivered);
        assertTrue(built.get("A").isClosed(), "the superseded result is closed");
        assertNull(built.get("B"), "a request superseded before it started is skipped");
        assertFalse(built.get("C").isClosed(), "the delivered pack belongs to the listener");
    }

    @Test
    void failuresAreDeliveredOnTheMainThread() throws Exception {
        PackCompiler compiler = new PackCompiler(mainThread::add, listener, request -> {
            throw new PackException("syntax error in final.fsh");
        });
        compiler.submit(request("Broken"));
        drain(compiler, 1);
        compiler.close();
        assertEquals(List.of("failed Broken: syntax error in final.fsh"), delivered);
    }

    @Test
    void cancelDiscardsTheResultQueuedForTheMainThread() throws Exception {
        PackCompiler compiler = new PackCompiler(mainThread::add, listener, this::build);
        compiler.submit(request("A"));
        Runnable delivery = mainThread.poll(10, TimeUnit.SECONDS);
        assertNotNull(delivery);
        compiler.cancel();
        delivery.run();
        compiler.close();
        assertEquals(List.of(), delivered);
        assertTrue(built.get("A").isClosed());
    }

    @Test
    void withoutTheNativeLibraryCompilesFail() throws Exception {
        PackCompiler compiler = new PackCompiler(mainThread::add, listener);
        compiler.submit(request("Real"));
        drain(compiler, 1);
        compiler.close();
        assertEquals(List.of("failed Real: The ShaderBridge native library is not loaded"), delivered);
    }
}
