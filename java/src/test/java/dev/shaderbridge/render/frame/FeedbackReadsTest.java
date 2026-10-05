package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertEquals;

import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.ResourceRef;
import java.util.List;
import java.util.Optional;
import java.util.Set;
import org.junit.jupiter.api.Test;

/** {@link FeedbackReads}: which attached targets geometry passes copy before drawing. */
class FeedbackReadsTest {
    @Test
    void attachedTargetsSampledByGeometryProgramsAreCopied() throws Exception {
        DimensionPipeline dim = FrameSequencerTraceTest.pack("frame/RethinkingVoxels.json").dimension("world0").orElseThrow();
        assertEquals(List.of(0, 1, 3, 5), dim.gbufferAttachments());
        // gbuffers_water and dh_water sample colortex5, which the shared gbuffers pass draws into;
        // colortex4/12, depthtex1 and the shadow maps are not attachments of that pass.
        assertEquals(Set.of(new ResourceRef.ColorTex(5)), FeedbackReads.of(dim, false, dim.gbufferAttachments()));
        assertEquals(Set.of(), FeedbackReads.of(dim, true, dim.shadowAttachments()), "the shadow programs sample nothing they draw into");
        assertEquals(Set.of(), FeedbackReads.of(dim, false, List.of(0)), "a single fallback attachment that nothing samples");
    }

    @Test
    void copiesAreKeptUnderOneNamePerTexture() {
        assertEquals(Optional.of(new ResourceRef.ShadowTex(0)), FeedbackReads.key(new ResourceRef.ShadowTexHw(0)));
        assertEquals(Optional.of(new ResourceRef.DepthTex(0)), FeedbackReads.key(new ResourceRef.DepthTex(0)));
        assertEquals(Optional.empty(), FeedbackReads.key(new ResourceRef.DepthTex(1)), "depthtex1 is a copy, never attached");
        assertEquals(Optional.empty(), FeedbackReads.key(new ResourceRef.ShadowTex(1)));
        assertEquals(Optional.empty(), FeedbackReads.key(new ResourceRef.Noise()));
        assertEquals(Optional.of(new ResourceRef.ShadowColor(1)), FeedbackReads.key(new ResourceRef.ShadowColor(1)));
    }
}
