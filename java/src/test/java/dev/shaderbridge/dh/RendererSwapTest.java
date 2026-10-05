package dev.shaderbridge.dh;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.atomic.AtomicReference;
import org.junit.jupiter.api.Test;

/**
 * {@link RendererSwap}: the stand-in for a Distant Horizons renderer, with a renderer-shaped
 * interface (a {@code render} method plus {@code IBindable}-style default setup methods).
 */
class RendererSwapTest {
    /** Shaped like Distant Horizons' renderer interfaces. */
    public interface Renderer {
        void render(String params, boolean opaque);

        default boolean getDelayedSetupComplete() {
            return true;
        }

        void finishDelayedSetup();
    }

    /** A renderer that records its calls. */
    private static final class Original implements Renderer {
        final List<String> calls = new ArrayList<>();

        @Override
        public void render(String params, boolean opaque) {
            calls.add("render " + params);
        }

        @Override
        public boolean getDelayedSetupComplete() {
            calls.add("complete?");
            return false;
        }

        @Override
        public void finishDelayedSetup() {
            calls.add("finish");
        }
    }

    private final AtomicReference<Object> slot = new AtomicReference<>();
    private final List<Object[]> rendered = new ArrayList<>();
    private final RendererSwap swap = new RendererSwap(Renderer.class, slot::get, slot::set, rendered::add);

    @Test
    void standInRendersInsteadAndForwardsTheRest() {
        Original original = new Original();
        slot.set(original);
        swap.install();
        Renderer standIn = (Renderer) slot.get();
        assertNotSame(original, standIn);
        standIn.render("frame 1", true);
        assertEquals(1, rendered.size());
        assertEquals("frame 1", rendered.getFirst()[0]);
        assertEquals(true, rendered.getFirst()[1]);
        assertFalse(standIn.getDelayedSetupComplete(), "forwarded to the original");
        standIn.finishDelayedSetup();
        assertEquals(List.of("complete?", "finish"), original.calls, "the original never renders");
        assertEquals(standIn, standIn);
        assertEquals(System.identityHashCode(standIn), standIn.hashCode());
    }

    @Test
    void installIsIdempotentAndWaitsForTheRenderer() {
        swap.install();
        assertNull(slot.get(), "Distant Horizons has not created the renderer yet");
        Original original = new Original();
        slot.set(original);
        swap.install();
        Object standIn = slot.get();
        swap.install();
        assertSame(standIn, slot.get());
        swap.restore();
        assertSame(original, slot.get());
        swap.restore();
        assertSame(original, slot.get(), "restoring twice changes nothing");
    }

    @Test
    void aRendererPutThereLaterBecomesTheOriginal() {
        slot.set(new Original());
        swap.install();
        Original rebound = new Original();
        slot.set(rebound);
        swap.install();
        swap.restore();
        assertSame(rebound, slot.get());
    }

    @Test
    void restoreLeavesAForeignReplacementAlone() {
        slot.set(new Original());
        swap.install();
        Original foreign = new Original();
        slot.set(foreign);
        swap.restore();
        assertSame(foreign, slot.get());
    }

    @Test
    void withoutAnOriginalDefaultMethodsRunAndOthersFail() {
        Renderer standIn = (Renderer) java.lang.reflect.Proxy.newProxyInstance(Renderer.class.getClassLoader(), new Class<?>[] {Renderer.class},
            swap);
        assertTrue(standIn.getDelayedSetupComplete(), "the interface default");
        assertThrows(IllegalStateException.class, standIn::finishDelayedSetup);
        standIn.render("x", false);
        assertEquals(1, rendered.size());
    }

    @Test
    void doNothingRendersNothing() {
        Original original = new Original();
        slot.set(original);
        RendererSwap quiet = new RendererSwap(Renderer.class, slot::get, slot::set, RendererSwap.Render.NOTHING);
        quiet.install();
        ((Renderer) slot.get()).render("frame", true);
        assertTrue(original.calls.isEmpty());
    }
}
