package dev.shaderbridge.render.targets;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.textures.GpuTexture;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.render.RenderFixture;
import java.util.List;
import org.joml.Vector4f;
import org.junit.jupiter.api.Test;

class PackTargetsTest {
    private static final RenderFixture GLIMMER = RenderFixture.load(RenderFixture.GLIMMER);
    private static final RenderFixture TUTORIAL = RenderFixture.load(RenderFixture.TUTORIAL4);

    @Test
    void createsMainAndAltTexturesWithAttachmentViewsOfTheBaseLevel() {
        FakeGpu gpu = new FakeGpu();
        try (PackTargets targets = new PackTargets(gpu.device(), GLIMMER.dim(), 1920, 1080, GpuFormat.D32_FLOAT, f -> 16384)) {
            ColorPair ct0 = targets.color(0).orElseThrow();
            assertNotSame(ct0.texture(false), ct0.texture(true));
            assertEquals(ColorPair.USAGE, ct0.texture(false).usage());
            assertEquals(GpuTexture.USAGE_RENDER_ATTACHMENT | GpuTexture.USAGE_TEXTURE_BINDING | GpuTexture.USAGE_COPY_SRC | GpuTexture.USAGE_COPY_DST,
                ColorPair.USAGE);
            assertEquals(11, ct0.texture(false).getMipLevels());
            assertEquals(11, ct0.sampleView(false).mipLevels());
            assertEquals(1, ct0.attachmentView(true).mipLevels());
            ColorPair ct1 = targets.color(1).orElseThrow();
            assertSame(ct1.sampleView(false), ct1.attachmentView(false), "a single-level target uses one view");
            int views = gpu.views.size();
            assertSame(ct0.attachmentView(true), ct0.levelView(true, 0));
            FakeGpu.View level3 = (FakeGpu.View) ct0.levelView(true, 3);
            assertEquals(List.of(3, 1), List.of(level3.baseMipLevel(), level3.mipLevels()));
            assertSame(ct0.texture(true), level3.texture());
            assertSame(level3, ct0.levelView(true, 3), "level views are made once, on demand");
            assertEquals(views + 1, gpu.views.size());
            assertEquals(512, targets.color(7).orElseThrow().texture(false).getWidth(0));
            assertEquals(16, targets.colorTargets().size());
            assertEquals(2, targets.shadowColorTargets().size());
            assertEquals(512, targets.shadowDepth(0).getWidth(0));
            assertEquals(GpuFormat.D32_FLOAT, targets.shadowDepth(1).getFormat());
            assertEquals(1920, ((FakeGpu.View) targets.depthCopyView(2)).getWidth(0));
            assertThrows(IllegalArgumentException.class, () -> targets.depthCopyView(0));
        }
        assertTrue(gpu.textures.stream().allMatch(t -> t.closed), "close releases every texture");
        assertTrue(gpu.views.stream().allMatch(FakeGpu.View::isClosed));
    }

    @Test
    void resizeRecreatesOnlyScreenSizedTargets() {
        FakeGpu gpu = new FakeGpu();
        PackTargets targets = new PackTargets(gpu.device(), TUTORIAL.dim(), 800, 600, GpuFormat.D32_FLOAT, f -> 16384);
        GpuTexture oldColor = targets.color(0).orElseThrow().texture(false);
        GpuTexture shadow = targets.shadowDepth(0);
        assertFalse(targets.resize(800, 600));
        assertTrue(targets.resize(1024, 768));
        assertTrue(oldColor.isClosed());
        assertFalse(shadow.isClosed());
        assertEquals(1024, targets.color(0).orElseThrow().texture(false).getWidth(0));
        assertEquals(List.of(1024, 768), List.of(targets.screenSize()[0], targets.screenSize()[1]));
        targets.close();
    }

    @Test
    void clearsFollowThePackAndTheFirstFrame() {
        FakeGpu gpu = new FakeGpu();
        PackTargets targets = new PackTargets(gpu.device(), GLIMMER.dim(), 64, 64, GpuFormat.D32_FLOAT, f -> 16384);
        Rgba fog = new Rgba(0.5f, 0.6f, 0.7f, 1);
        targets.clear(gpu.encoder(), fog, DepthMode.REVERSED_ZERO_TO_ONE);
        int pairs = targets.colorTargets().size() + targets.shadowColorTargets().size();
        assertEquals(2 * pairs, gpu.commands("clearColorTexture").size(), "the first frame clears main and alt of every target");
        assertEquals(4, gpu.commands("clearDepthTexture").size());
        assertEquals(0.0, gpu.commands("clearDepthTexture").getFirst().args().get(1));
        gpu.commands.clear();
        targets.clear(gpu.encoder(), fog, DepthMode.REVERSED_ZERO_TO_ONE);
        // colortex0 and colortex3 are not cleared (clear = false); colortex4 clears to its white.
        List<Object> cleared = gpu.commands("clearColorTexture").stream().map(c -> c.args().getFirst()).toList();
        assertFalse(cleared.contains(targets.color(0).orElseThrow().texture(false)));
        assertFalse(cleared.contains(targets.color(3).orElseThrow().texture(true)));
        GpuTexture ct4 = targets.color(4).orElseThrow().texture(false);
        assertEquals(new Vector4f(1, 1, 1, 1), gpu.commands("clearColorTexture").stream().filter(c -> c.args().getFirst() == ct4).findFirst()
            .orElseThrow().args().get(1));
        GpuTexture ct1 = targets.color(1).orElseThrow().texture(false);
        assertEquals(new Vector4f(1, 1, 1, 1), gpu.commands("clearColorTexture").stream().filter(c -> c.args().getFirst() == ct1).findFirst()
            .orElseThrow().args().get(1), "colortex1 defaults to white");
        targets.close();
    }

    @Test
    void copiesDepthAndAltBuffers() {
        FakeGpu gpu = new FakeGpu();
        PackTargets targets = new PackTargets(gpu.device(), GLIMMER.dim(), 64, 32, GpuFormat.D32_FLOAT, f -> 16384);
        FakeGpu.Texture mainDepth = new FakeGpu.Texture("main depth", 15, GpuFormat.D32_FLOAT, 64, 32, 1);
        targets.copyMainDepth(gpu.encoder(), mainDepth, 1);
        targets.copyShadowDepth(gpu.encoder());
        targets.copyAltToMain(gpu.encoder(), 0);
        targets.copyAltToMain(gpu.encoder(), 31);
        List<FakeGpu.Command> copies = gpu.commands("copyTextureToTexture");
        assertSame(mainDepth, copies.get(0).args().get(0));
        assertEquals(List.of(0, 0, 0, 0, 0, 64, 32), copies.get(0).args().subList(2, 9));
        assertSame(targets.shadowDepth(0), copies.get(1).args().get(0));
        assertSame(targets.shadowDepth(1), copies.get(1).args().get(1));
        // colortex0 has a full mip chain: one copy per level.
        assertEquals(2 + targets.color(0).orElseThrow().spec().mipLevels(), copies.size());
        targets.close();
    }
}
