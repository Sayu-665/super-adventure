package dev.shaderbridge.render.frame;

import dev.shaderbridge.model.Pass;
import dev.shaderbridge.model.PassGroup;
import java.util.List;

/**
 * What a {@link FrameSequencer} asks the renderer to do. The opaque and translucent geometry is
 * not here: Minecraft draws it, and the sequencer stops before it (see
 * {@link FrameSequencer#runUntil}). Render thread only.
 */
public interface FrameSteps {
    /**
     * A pass of the model starts (labels, profiling).
     *
     * @param pass the pass
     */
    void passStarted(Pass pass);

    /**
     * A geometry group is drawn without a pass of its own in the model.
     *
     * @param group the geometry group
     */
    void implicitGeometry(PassGroup group);

    /**
     * Dispatches the compute programs of a pass, in order.
     *
     * @param pass  the pass
     * @param flips the flip state the computes see
     */
    void computes(Pass pass, FlipState flips);

    /**
     * Renders the shadow pass (when the pack enables shadows): opaque casters, the
     * {@code shadowtex1} copy, translucent casters.
     *
     * @param flips the flip state (shadowcolor attachments are the current textures)
     */
    void shadow(FlipState flips);

    /**
     * Draws a composite-style program.
     *
     * @param program index into {@code programs}
     * @param group   its pass group
     * @param flips   the flip state: outputs go to the {@link FlipState#write} textures, inputs
     *                follow {@code BindingUse.use_alt}
     * @return whether it drew and which shadowcolor targets it wrote
     */
    Drawn fullscreen(int program, PassGroup group, FlipState flips);

    /**
     * No {@code final} program drew: the current {@code colortex0} goes to Minecraft's main color
     * target.
     *
     * @param flips the flip state at the end of the frame
     */
    void copyToOutput(FlipState flips);

    /**
     * End of the frame: alt to main copies of buffers left in their alt texture.
     *
     * @param color  colortex indices to copy
     * @param shadow shadowcolor indices to copy
     */
    void endOfFrame(List<Integer> color, List<Integer> shadow);

    /**
     * Reports a model inconsistency (logged once by the implementation).
     *
     * @param message what is wrong
     */
    void warn(String message);

    /**
     * Outcome of {@link #fullscreen}.
     *
     * @param drew          the program drew
     * @param shadowWritten shadowcolor targets a {@code shadowcomp} program wrote (they flip)
     */
    record Drawn(boolean drew, List<Integer> shadowWritten) {
        /** The program did not draw. */
        public static final Drawn NOTHING = new Drawn(false, List.of());

        public Drawn {
            shadowWritten = List.copyOf(shadowWritten);
        }
    }
}
