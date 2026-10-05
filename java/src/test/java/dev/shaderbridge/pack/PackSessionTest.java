package dev.shaderbridge.pack;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTimeoutPreemptively;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Path;
import java.time.Duration;
import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import org.junit.jupiter.api.Test;

/** Closing a session never waits for a running native call (a compile on a worker thread). */
class PackSessionTest {
    @Test
    void closeDuringACallReleasesWhenTheCallReturns() throws Exception {
        List<Long> released = new CopyOnWriteArrayList<>();
        PackSession session = new PackSession(Path.of("pack"), 42, released::add);
        CountDownLatch inCall = new CountDownLatch(1);
        CountDownLatch finish = new CountDownLatch(1);
        ExecutorService worker = Executors.newSingleThreadExecutor();
        try {
            Future<Long> call = worker.submit(() -> session.withHandle(h -> {
                inCall.countDown();
                try {
                    finish.await();
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                }
                return h;
            }));
            assertTrue(inCall.await(5, TimeUnit.SECONDS));
            // The main thread closes while the worker holds the session: no waiting.
            assertTimeoutPreemptively(Duration.ofSeconds(2), session::close);
            assertFalse(session.isReleased(), "released under a running call");
            assertTrue(released.isEmpty());
            finish.countDown();
            assertEquals(42L, call.get(5, TimeUnit.SECONDS));
            assertEquals(List.of(42L), released, "the call releases the session when it returns");
            assertTrue(session.isReleased());
            assertThrows(PackException.class, () -> session.withHandle(h -> h));
            session.close();
            assertEquals(List.of(42L), released, "closing is idempotent");
        } finally {
            worker.shutdownNow();
        }
    }

    @Test
    void closeWithoutACallReleasesAtOnce() throws PackException {
        List<Long> released = new CopyOnWriteArrayList<>();
        PackSession session = new PackSession(Path.of("pack"), 7, released::add);
        assertEquals(7L, session.handle());
        session.close();
        assertEquals(List.of(7L), released);
        assertThrows(PackException.class, session::handle);
    }
}
