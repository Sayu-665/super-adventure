package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.ResourceRef;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;

/** {@link ColorReads}: main or alt per program kind, as {@code color_read} in sb-runtime. */
class ColorReadsTest {
    private static final ProgramKind COMPOSITE = new ProgramKind.Composite(PassGroup.COMPOSITE, 2);
    private static final ProgramKind COMPOSITE_COMPUTE = new ProgramKind.Compute(PassGroup.COMPOSITE, 2, 'a');
    private static final ProgramKind TERRAIN = new ProgramKind.Geometry(GeometryProgram.TERRAIN);
    private static final ProgramKind SHADOW_COMPUTE = new ProgramKind.GeometryCompute(GeometryProgram.SHADOW, null);

    @Test
    void compositeStylePrograms_followUseAlt_andReportDisagreements() {
        FlipState flips = new FlipState();
        flips.flip(List.of(4));
        List<String> warnings = new ArrayList<>();
        assertTrue(ColorReads.alt(COMPOSITE, new ResourceRef.ColorTex(4), true, flips, warnings::add));
        assertTrue(warnings.isEmpty());
        assertFalse(ColorReads.alt(COMPOSITE, new ResourceRef.ColorTex(4), false, flips, warnings::add));
        assertEquals(1, warnings.size());
        assertTrue(ColorReads.alt(COMPOSITE_COMPUTE, new ResourceRef.ColorImage(0), true, flips, warnings::add));
        assertEquals(2, warnings.size(), "colortex0 is in main, use_alt says alt");
    }

    @Test
    void geometryAndGeometryComputesReadTheCurrentTexture() {
        FlipState flips = new FlipState();
        flips.flip(List.of(1));
        List<String> warnings = new ArrayList<>();
        assertTrue(ColorReads.alt(TERRAIN, new ResourceRef.ColorTex(1), false, flips, warnings::add));
        assertFalse(ColorReads.alt(TERRAIN, new ResourceRef.ColorTex(2), true, flips, warnings::add));
        assertTrue(ColorReads.alt(SHADOW_COMPUTE, new ResourceRef.ColorImage(1), false, flips, warnings::add));
        assertTrue(warnings.isEmpty());
        assertTrue(ColorReads.followsCurrentState(TERRAIN));
        assertFalse(ColorReads.followsCurrentState(COMPOSITE_COMPUTE));
    }

    @Test
    void shadowcolorFollowsTheShadowFlipsAndOtherResourcesAreNotPingPonged() {
        FlipState flips = new FlipState();
        flips.flipShadow(List.of(1));
        assertTrue(ColorReads.alt(COMPOSITE, new ResourceRef.ShadowColor(1), false, flips, w -> { }));
        assertTrue(ColorReads.alt(TERRAIN, new ResourceRef.ShadowColorImage(1), false, flips, w -> { }));
        assertFalse(ColorReads.alt(COMPOSITE, new ResourceRef.ShadowColor(0), true, flips, w -> { }));
        assertFalse(ColorReads.alt(COMPOSITE, new ResourceRef.DepthTex(1), true, flips, w -> { }));
        assertFalse(ColorReads.alt(COMPOSITE, new ResourceRef.Noise(), true, flips, w -> { }));
    }
}
