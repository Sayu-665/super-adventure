package dev.shaderbridge.render.raw;

import java.util.ArrayList;
import java.util.Collection;
import java.util.HashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;

/**
 * Tracks the resources the raw path creates itself (custom images, fallback images, storage
 * buffers) from creation to first use. Minecraft's textures are moved to {@code GENERAL} when
 * created and stay there; the raw path's own images start {@code UNDEFINED} and are moved to
 * {@code GENERAL} and cleared (buffers zero-filled or loaded) right before their first use, and
 * images the pack clears every frame ({@code image.<name>} with {@code clear}) are cleared again
 * before their first use in each frame. Render thread only.
 *
 * @param <K> the resource key
 */
public final class ResourceInit<K> {
    private final Map<K, State> states = new HashMap<>();

    /** What must happen to a resource before a command uses it. */
    public sealed interface Op<K> {
        /** @return the resource */
        K resource();

        /**
         * First use: move the image from {@code UNDEFINED} to {@code GENERAL} (images) and give it
         * its initial contents.
         *
         * @param resource the resource
         */
        record Initialize<K>(K resource) implements Op<K> {
        }

        /**
         * First use in a frame of a resource the pack clears every frame.
         *
         * @param resource the resource
         */
        record Clear<K>(K resource) implements Op<K> {
        }
    }

    private static final class State {
        final boolean clearEveryFrame;
        boolean initialized;
        long clearedFrame;

        State(boolean clearEveryFrame) {
            this.clearEveryFrame = clearEveryFrame;
        }
    }

    /**
     * Starts tracking a newly created resource (again, if it was recreated).
     *
     * @param resource        the resource
     * @param clearEveryFrame the pack clears it at the start of every frame
     */
    public void created(K resource, boolean clearEveryFrame) {
        states.put(resource, new State(clearEveryFrame));
    }

    /**
     * Stops tracking a destroyed resource.
     *
     * @param resource the resource
     */
    public void destroyed(K resource) {
        states.remove(resource);
    }

    /**
     * @param used  the tracked resources a command is about to use (others are ignored)
     * @param frame the current frame
     * @return what to do to them first, in the order given; they count as done afterwards
     */
    public List<Op<K>> before(Collection<K> used, long frame) {
        List<Op<K>> ops = new ArrayList<>();
        for (K resource : new LinkedHashSet<>(used)) {
            State state = states.get(resource);
            if (state == null) {
                continue;
            }
            if (!state.initialized) {
                state.initialized = true;
                state.clearedFrame = frame;
                ops.add(new Op.Initialize<>(resource));
            } else if (state.clearEveryFrame && state.clearedFrame != frame) {
                state.clearedFrame = frame;
                ops.add(new Op.Clear<>(resource));
            }
        }
        return ops;
    }
}
