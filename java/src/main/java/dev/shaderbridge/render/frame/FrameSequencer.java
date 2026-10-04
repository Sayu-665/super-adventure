package dev.shaderbridge.render.frame;

import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Pass;
import dev.shaderbridge.model.PassGroup;
import java.util.List;
import java.util.function.IntPredicate;

/**
 * Walks the {@link FramePlan} of a dimension pipeline once per frame and keeps its
 * {@link FlipState}. Minecraft draws the opaque and translucent geometry at times it chooses, so a
 * frame is split: {@link #runUntil} runs every step before a geometry group (including that
 * group's computes) and returns, the caller lets the geometry draw, and {@link #finish} runs the
 * rest, the copy to Minecraft's main target when no {@code final} program drew, and the
 * end-of-frame copies. The order and the flip bookkeeping are those of the headless executor's
 * {@code record_frame}. Render thread only.
 */
public final class FrameSequencer {
    private final FramePlan plan;
    private final List<Integer> endOfFrameCopies;
    private final List<Integer> keptShadowColor;
    private final IntPredicate colorExists;
    private final FlipState flips = new FlipState();
    private int cursor = -1;
    private boolean firstFrame;
    private boolean finalDrawn;

    /**
     * @param dim             the dimension pipeline
     * @param colorExists     which colortex targets exist (flip disagreements and end-of-frame
     *                        copies of other indices are ignored)
     * @param keptShadowColor shadowcolor targets that exist and are not cleared every frame
     */
    public FrameSequencer(DimensionPipeline dim, IntPredicate colorExists, List<Integer> keptShadowColor) {
        this.plan = FramePlan.of(dim);
        this.endOfFrameCopies = dim.endOfFrameCopies();
        this.keptShadowColor = List.copyOf(keptShadowColor);
        this.colorExists = colorExists;
    }

    /** @return the steps this sequencer runs */
    public FramePlan plan() {
        return plan;
    }

    /** @return the flip state of the current frame */
    public FlipState flips() {
        return flips;
    }

    /** @return a frame was begun and not finished */
    public boolean inFrame() {
        return cursor >= 0;
    }

    /**
     * Starts a frame: every buffer back in main.
     *
     * @param first the first frame after the pipeline was loaded ({@code setup} computes run)
     */
    public void begin(boolean first) {
        flips.reset();
        cursor = 0;
        firstFrame = first;
        finalDrawn = false;
    }

    /**
     * Runs the steps before the geometry of a group, then that group's computes. The shadow pass
     * is drawn on the way ({@link FrameSteps#shadow}); a group passed over without its own
     * {@code runUntil} is not drawn.
     *
     * @param geometry {@link PassGroup#GBUFFERS_OPAQUE} or {@link PassGroup#GBUFFERS_TRANSLUCENT}
     * @param steps    the renderer
     * @return whether the geometry is now due (false once the frame has passed it)
     */
    public boolean runUntil(PassGroup geometry, FrameSteps steps) {
        if (cursor < 0) {
            throw new IllegalStateException("begin() was not called");
        }
        while (cursor < plan.steps().size()) {
            FramePlan.Step step = plan.steps().get(cursor++);
            if (step instanceof FramePlan.Step.Geometry g) {
                if (g.pass() == null) {
                    steps.implicitGeometry(g.group());
                } else {
                    steps.computes(g.pass(), flips);
                }
                if (g.group() == geometry) {
                    return true;
                }
                if (g.group() == PassGroup.SHADOW) {
                    steps.shadow(flips);
                }
            } else {
                run(step, steps);
            }
        }
        return false;
    }

    /**
     * Runs the remaining steps and ends the frame.
     *
     * @param steps the renderer
     */
    public void finish(FrameSteps steps) {
        if (cursor < 0) {
            throw new IllegalStateException("begin() was not called");
        }
        runUntil(null, steps);
        if (!finalDrawn) {
            steps.copyToOutput(flips);
        }
        List<Integer> color = flips.endOfFrameCopies(endOfFrameCopies).stream().filter(colorExists::test).toList();
        steps.endOfFrame(color, flips.shadowCopies(keptShadowColor));
        cursor = -1;
    }

    /** Abandons the current frame (a failure while rendering it); the next frame begins afresh. */
    public void abandon() {
        cursor = -1;
    }

    private void run(FramePlan.Step step, FrameSteps steps) {
        switch (step) {
            case FramePlan.Step.Begin b -> begin(b.pass(), b.firstOfGroup(), steps);
            case FramePlan.Step.Computes c -> {
                if (!c.firstFrameOnly() || firstFrame) {
                    steps.computes(c.pass(), flips);
                }
            }
            case FramePlan.Step.Fullscreen f -> {
                FrameSteps.Drawn drawn = steps.fullscreen(f.program(), f.group(), flips);
                if (drawn.drew() && f.group() == PassGroup.SHADOW_COMP) {
                    flips.flipShadow(drawn.shadowWritten());
                }
                if (drawn.drew() && f.group() == PassGroup.FINAL) {
                    finalDrawn = true;
                }
            }
            case FramePlan.Step.Flip f -> flips.flip(f.buffers());
            case FramePlan.Step.Geometry g -> throw new IllegalStateException("geometry is handled by runUntil: " + g);
        }
    }

    private void begin(Pass pass, boolean firstOfGroup, FrameSteps steps) {
        steps.passStarted(pass);
        if (pass.flipState().isEmpty()) {
            return;
        }
        List<Integer> mismatches = flips.adopt(pass.flipState(), colorExists);
        if (!mismatches.isEmpty() && !firstOfGroup) {
            steps.warn("pass " + pass.group().wireName() + pass.index() + ": flip_state disagrees with the flips of the previous passes for colortex "
                + mismatches + "; using flip_state");
        }
    }
}
