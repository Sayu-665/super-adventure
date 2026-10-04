package dev.shaderbridge.render.frame;

import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Pass;
import dev.shaderbridge.model.PassGroup;
import java.util.ArrayList;
import java.util.EnumSet;
import java.util.List;
import java.util.Set;

/**
 * The steps of one frame of a dimension pipeline, flattened from its pass list exactly as the
 * headless executor's {@code record_frame} walks it ({@code sb-runtime/src/frame.rs}):
 *
 * <ul>
 *   <li>each pass starts by adopting its flip state ({@link Step.Begin});</li>
 *   <li>a geometry group ({@code shadow}, {@code gbuffers_opaque}, {@code gbuffers_translucent})
 *   is drawn at its own pass, or implicitly before the first pass of a later group, or at the end
 *   of the frame, exactly once ({@link Step.Geometry}); a second pass of the same group only
 *   dispatches its computes;</li>
 *   <li>{@code setup} computes run on the first frame only;</li>
 *   <li>composite-style passes dispatch their computes, then draw their fullscreen program;</li>
 *   <li>{@code flips_after} applies after every pass except {@code shadowcomp}, whose flips are
 *   the shadowcolor targets its program writes.</li>
 * </ul>
 *
 * @param steps the steps in execution order; exactly one {@link Step.Geometry} per geometry group
 */
public record FramePlan(List<Step> steps) {
    /** The geometry groups, in frame order. */
    public static final List<PassGroup> GEOMETRY_GROUPS = List.of(PassGroup.SHADOW, PassGroup.GBUFFERS_OPAQUE, PassGroup.GBUFFERS_TRANSLUCENT);

    public FramePlan {
        steps = List.copyOf(steps);
    }

    /** One step. */
    public sealed interface Step {
        /**
         * A pass of the model starts: its {@code flip_state} is adopted.
         *
         * @param pass         the pass
         * @param firstOfGroup the previous pass belongs to another group (Iris applies the group's
         *                     {@code flip.<group>_pre} flips before it, which the model folds into
         *                     {@code flip_state} only, so a disagreement there is expected)
         */
        record Begin(Pass pass, boolean firstOfGroup) implements Step {
        }

        /**
         * The geometry of a group.
         *
         * @param group the geometry group
         * @param pass  the group's own pass (its computes run first), or null when implicit
         */
        record Geometry(PassGroup group, Pass pass) implements Step {
        }

        /**
         * The compute programs of a pass.
         *
         * @param pass           the pass
         * @param firstFrameOnly {@code setup} computes run once, on the first frame
         */
        record Computes(Pass pass, boolean firstFrameOnly) implements Step {
        }

        /**
         * A composite-style (fullscreen) program.
         *
         * @param program index into {@code programs}
         * @param group   the pass group ({@code shadowcomp} writes shadowcolor targets,
         *                {@code final} writes Minecraft's main color target)
         */
        record Fullscreen(int program, PassGroup group) implements Step {
        }

        /** @param buffers colortex buffers whose main and alt textures swap roles */
        record Flip(List<Integer> buffers) implements Step {
            public Flip {
                buffers = List.copyOf(buffers);
            }
        }
    }

    /**
     * @param dim a dimension pipeline
     * @return its frame steps
     */
    public static FramePlan of(DimensionPipeline dim) {
        List<Step> steps = new ArrayList<>();
        Set<PassGroup> done = EnumSet.noneOf(PassGroup.class);
        PassGroup previous = null;
        for (Pass pass : dim.passes()) {
            for (PassGroup g : GEOMETRY_GROUPS) {
                if (!done.contains(g) && g.compareTo(pass.group()) < 0) {
                    steps.add(new Step.Geometry(g, null));
                    done.add(g);
                }
            }
            steps.add(new Step.Begin(pass, previous != pass.group()));
            previous = pass.group();
            PassGroup group = pass.group();
            if (group == PassGroup.SETUP) {
                computes(steps, pass, true);
            } else if (GEOMETRY_GROUPS.contains(group)) {
                if (done.add(group)) {
                    steps.add(new Step.Geometry(group, pass));
                } else {
                    computes(steps, pass, false);
                }
            } else {
                computes(steps, pass, false);
                if (pass.program() != null) {
                    steps.add(new Step.Fullscreen(pass.program(), group));
                }
            }
            if (group != PassGroup.SHADOW_COMP && !pass.flipsAfter().isEmpty()) {
                steps.add(new Step.Flip(pass.flipsAfter()));
            }
        }
        for (PassGroup g : GEOMETRY_GROUPS) {
            if (done.add(g)) {
                steps.add(new Step.Geometry(g, null));
            }
        }
        return new FramePlan(steps);
    }

    private static void computes(List<Step> steps, Pass pass, boolean firstFrameOnly) {
        if (!pass.computes().isEmpty()) {
            steps.add(new Step.Computes(pass, firstFrameOnly));
        }
    }
}
