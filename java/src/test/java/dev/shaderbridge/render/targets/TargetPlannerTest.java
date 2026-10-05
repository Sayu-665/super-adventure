package dev.shaderbridge.render.targets;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.AxisSize;
import dev.shaderbridge.model.ShadowSettings;
import dev.shaderbridge.model.TargetSize;
import dev.shaderbridge.render.RenderFixture;
import java.util.List;
import java.util.Optional;
import org.junit.jupiter.api.Test;

class TargetPlannerTest {
    private static final RenderFixture GLIMMER = RenderFixture.load(RenderFixture.GLIMMER);
    private static final RenderFixture TUTORIAL = RenderFixture.load(RenderFixture.TUTORIAL4);

    @Test
    void targetSizesResolveLikeTheRustModel() {
        assertArrayEquals(new int[] {960, 540}, new TargetSize.Relative(0.5f, 0.5f).resolve(1920, 1080));
        assertArrayEquals(new int[] {640, 1}, new TargetSize.Relative(0.5f, 0.0001f).resolve(1281, 721), "truncated as in Iris, at least one pixel");
        assertArrayEquals(new int[] {64, 1}, new TargetSize.Absolute(64, 0).resolve(1920, 1080));
        assertArrayEquals(new int[] {960, 64}, new TargetSize.PerAxis(new AxisSize.Relative(0.5f), new AxisSize.Absolute(64)).resolve(1920, 1080));
    }

    @Test
    void colorTargetsFollowThePackAndAreClampedToTheDevice() {
        List<TargetSpec> targets = TargetPlanner.colorTargets(GLIMMER.dim(), 1920, 1080, f -> 16384);
        assertEquals(List.of(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15), targets.stream().map(TargetSpec::index).toList(),
            "every colortex the binding table names exists");
        TargetSpec ct0 = targets.get(0);
        assertEquals(GpuFormat.RGBA16_FLOAT, ct0.format());
        assertEquals(1920, ct0.width());
        assertEquals(TargetPlanner.fullMipChain(1920, 1080), ct0.mipLevels(), "composite90 requests colortex0 mipmaps");
        assertEquals(11, ct0.mipLevels());
        assertFalse(ct0.clear());
        assertEquals(GpuFormat.RGBA16_FLOAT, targets.get(2).format(), "RGB16F is widened to four components");
        assertEquals(GpuFormat.RG11B10_FLOAT, targets.get(4).format());
        assertEquals(Optional.of(Rgba.WHITE), targets.get(4).clearColor());
        assertEquals(GpuFormat.R16_UNORM, targets.get(6).format());
        TargetSpec ct7 = targets.get(7);
        assertEquals(512, ct7.width());
        assertEquals(512, ct7.height());
        assertEquals(10, ct7.mipLevels(), "prepare2 requests colortex7 mipmaps");
        TargetSpec ct9 = targets.get(9);
        assertEquals(GpuFormat.RGBA8_UNORM, ct9.format(), "an unconfigured target is RGBA8");
        assertTrue(ct9.clear());
        assertEquals(Optional.empty(), ct9.clearColor());
        List<TargetSpec> small = TargetPlanner.colorTargets(GLIMMER.dim(), 1920, 1080, f -> 256);
        assertEquals(256, small.get(0).width());
        assertEquals(256, small.get(7).width());
    }

    @Test
    void shadowColorTargetsUseTheShadowResolution() {
        List<TargetSpec> targets = TargetPlanner.shadowColorTargets(TUTORIAL.dim(), f -> 16384);
        assertEquals(List.of(0, 1), targets.stream().map(TargetSpec::index).toList());
        assertTrue(targets.stream().allMatch(t -> t.width() == 2048 && t.height() == 2048 && t.shadow() && t.mipLevels() == 1));
        assertEquals("shadowcolor1", targets.get(1).name());
        assertEquals(List.of(0, 1), TargetPlanner.shadowColorTargets(GLIMMER.dim(), f -> 16384).stream().map(TargetSpec::index).toList());
        assertEquals(GpuFormat.R8_UNORM, TargetPlanner.shadowColorTargets(GLIMMER.dim(), f -> 16384).get(1).format());
    }

    @Test
    void colorTargetsIncludeEveryWrittenTarget() {
        List<Integer> indices = TargetPlanner.colorTargets(TUTORIAL.dim(), 800, 600, f -> 16384).stream().map(TargetSpec::index).toList();
        assertEquals(List.of(0, 1, 2), indices);
    }

    @Test
    void shadowResolutionAndMipChains() {
        ShadowSettings shadow = TUTORIAL.dim().targets().shadow();
        assertEquals(2048, TargetPlanner.shadowResolution(shadow));
        assertEquals(1, TargetPlanner.fullMipChain(1, 1));
        assertEquals(2, TargetPlanner.fullMipChain(2, 1));
        assertEquals(12, TargetPlanner.fullMipChain(2048, 16));
    }

    @Test
    void defaultClearColorsFollowIris() {
        Rgba fog = new Rgba(0.2f, 0.3f, 0.4f, 0.5f);
        assertEquals(new Rgba(0.2f, 0.3f, 0.4f, 1), ClearColors.defaultClear(0, false, fog));
        assertEquals(Rgba.WHITE, ClearColors.defaultClear(1, false, fog));
        assertEquals(Rgba.TRANSPARENT, ClearColors.defaultClear(5, false, fog));
        assertEquals(Rgba.WHITE, ClearColors.defaultClear(0, true, fog));
        TargetSpec explicit = new TargetSpec(0, false, GpuFormat.RGBA8_UNORM, 1, 1, 1, true, Optional.of(new Rgba(1, 0, 0, 1)));
        assertEquals(new Rgba(1, 0, 0, 1), ClearColors.of(explicit, fog));
        assertEquals(new Rgba(1, 2, 3, 4), Rgba.of(List.of(1f, 2f, 3f, 4f)));
        assertEquals(new org.joml.Vector4f(1, 2, 3, 4), new Rgba(1, 2, 3, 4).toVector());
    }
}
