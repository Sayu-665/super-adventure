package dev.shaderbridge.render.raw;

import static org.junit.jupiter.api.Assertions.assertEquals;

import dev.shaderbridge.model.BindingEntry;
import dev.shaderbridge.model.BindingTable;
import dev.shaderbridge.model.ResourceKind;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.render.targets.ColorPair;
import java.util.List;
import java.util.Set;
import org.junit.jupiter.api.Test;

class StorageTargetsTest {
    private static final RenderFixture GLIMMER = RenderFixture.load(RenderFixture.GLIMMER);
    private static final ResourceKind.StorageImage IMAGE = new ResourceKind.StorageImage("2d", "rgba16f", "float", false, false);
    private static final ResourceKind.Sampler SAMPLER = new ResourceKind.Sampler("2d", false, "float");

    @Test
    void targetsBoundAsStorageImagesGetTheBitOnBothTextures() {
        BindingTable table = new BindingTable(List.of(
            new BindingEntry("colorimg3", 2, 3, IMAGE, new ResourceRef.ColorImage(3)),
            new BindingEntry("shadowcolorimg1", 2, 9, IMAGE, new ResourceRef.ShadowColorImage(1)),
            new BindingEntry("colortex5", 2, 5, IMAGE, new ResourceRef.ColorTex(5)),
            new BindingEntry("colortex4", 1, 4, SAMPLER, new ResourceRef.ColorTex(4)),
            new BindingEntry("lut", 2, 20, IMAGE, new ResourceRef.Image("lut"))));
        assertEquals(Set.of(
            ColorPair.label("colortex3", false), ColorPair.label("colortex3", true),
            ColorPair.label("shadowcolor1", false), ColorPair.label("shadowcolor1", true),
            ColorPair.label("colortex5", false), ColorPair.label("colortex5", true)),
            StorageTargets.labels(Models.withBindings(GLIMMER.dim(), table)));
        assertEquals("ShaderBridge colortex3 (alt)", ColorPair.label("colortex3", true), "the labels ColorPair creates its textures with");
    }

    @Test
    void glimmerStoresOnlyToItsCustomImages() {
        assertEquals(Set.of(), StorageTargets.labels(GLIMMER.dim()));
    }
}
