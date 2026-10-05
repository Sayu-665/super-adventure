package dev.shaderbridge.render.raw;

import java.util.ArrayList;
import java.util.List;

/**
 * The synchronization of one raw dispatch or draw, recorded into its own command buffer between
 * two of Minecraft's passes. Minecraft keeps every image in {@code GENERAL} and ends each of its
 * passes, copies and uploads with a global memory barrier; the raw path does the same, so the two
 * interleave without tracking any per-image state:
 *
 * <ol>
 *   <li>a full memory barrier (all commands, memory read and write), carrying the
 *   {@code UNDEFINED -> GENERAL} transitions of the raw path's own resources used for the first
 *   time;</li>
 *   <li>their initial clears and fills, and the per-frame clears, then another full barrier;</li>
 *   <li>the dispatch or draw;</li>
 *   <li>a final full barrier, so whatever Minecraft records next sees the results.</li>
 * </ol>
 */
public final class CommandPlan {
    private CommandPlan() {
    }

    /**
     * One step.
     *
     * @param <K> the resource key
     */
    public sealed interface Step<K> {
        /**
         * A full memory barrier.
         *
         * @param transitions own images moved from {@code UNDEFINED} to {@code GENERAL} by it
         */
        record Barrier<K>(List<K> transitions) implements Step<K> {
            public Barrier {
                transitions = List.copyOf(transitions);
            }
        }

        /**
         * Gives a resource its initial or per-frame contents (clear to zero, or load the pack's data).
         *
         * @param resource the resource
         * @param initial  its first use (load initial data where it has some)
         */
        record Fill<K>(K resource, boolean initial) implements Step<K> {
        }

        /** The dispatch or draw itself. */
        record Run<K>() implements Step<K> {
        }
    }

    /**
     * @param ops what the used resources need first ({@link ResourceInit#before})
     * @param <K> the resource key
     * @return the steps, in recording order
     */
    public static <K> List<Step<K>> of(List<ResourceInit.Op<K>> ops) {
        List<Step<K>> steps = new ArrayList<>();
        List<K> transitions = ops.stream().filter(op -> op instanceof ResourceInit.Op.Initialize<K>).map(ResourceInit.Op::resource).toList();
        steps.add(new Step.Barrier<>(transitions));
        if (!ops.isEmpty()) {
            for (ResourceInit.Op<K> op : ops) {
                steps.add(new Step.Fill<>(op.resource(), op instanceof ResourceInit.Op.Initialize<K>));
            }
            steps.add(new Step.Barrier<>(List.of()));
        }
        steps.add(new Step.Run<>());
        steps.add(new Step.Barrier<>(List.of()));
        return steps;
    }
}
