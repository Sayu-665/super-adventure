package dev.shaderbridge.render.raw;

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

/** The main/alt choice of raw programs, as sb-runtime's {@code color_read} and Java's {@code ColorReads}. */
class ImageChoiceTest {
    private static final ProgramKind COMPOSITE_COMPUTE = new ProgramKind.Compute(PassGroup.COMPOSITE, 2, null);
    private static final ProgramKind GEOMETRY_COMPUTE = new ProgramKind.GeometryCompute(GeometryProgram.TERRAIN, null);
    private static final List<Boolean> COLOR = List.of(false, true, false);
    private static final List<Boolean> SHADOW = List.of(true);

    @Test
    void compositeComputesFollowUseAltAndReportDisagreements() {
        List<String> warnings = new ArrayList<>();
        assertTrue(ImageChoice.alt(COMPOSITE_COMPUTE, new ResourceRef.ColorTex(1), true, COLOR, SHADOW, warnings::add));
        assertEquals(List.of(), warnings);
        assertTrue(ImageChoice.alt(COMPOSITE_COMPUTE, new ResourceRef.ColorImage(0), true, COLOR, SHADOW, warnings::add));
        assertEquals(1, warnings.size(), "use_alt wins over the flip state, with a message");
    }

    @Test
    void geometryComputesFollowTheCurrentState() {
        List<String> warnings = new ArrayList<>();
        assertTrue(ImageChoice.alt(GEOMETRY_COMPUTE, new ResourceRef.ColorImage(1), false, COLOR, SHADOW, warnings::add));
        assertFalse(ImageChoice.alt(GEOMETRY_COMPUTE, new ResourceRef.ColorTex(0), true, COLOR, SHADOW, warnings::add));
        assertEquals(List.of(), warnings);
    }

    @Test
    void shadowColorFollowsTheShadowcompFlips() {
        assertTrue(ImageChoice.alt(COMPOSITE_COMPUTE, new ResourceRef.ShadowColorImage(0), false, COLOR, SHADOW, m -> { }));
        assertTrue(ImageChoice.alt(GEOMETRY_COMPUTE, new ResourceRef.ShadowColor(0), false, COLOR, SHADOW, m -> { }));
    }

    @Test
    void otherResourcesAndUnknownIndicesUseTheMainTexture() {
        assertFalse(ImageChoice.alt(COMPOSITE_COMPUTE, new ResourceRef.Image("lut"), true, COLOR, SHADOW, m -> { }));
        assertFalse(ImageChoice.alt(GEOMETRY_COMPUTE, new ResourceRef.ColorTex(7), true, COLOR, SHADOW, m -> { }));
        assertFalse(ImageChoice.alt(COMPOSITE_COMPUTE, new ResourceRef.ShadowColor(3), false, COLOR, SHADOW, m -> { }));
    }
}
