package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.render.pipeline.TextureFormats;
import org.junit.jupiter.api.Test;

/** {@link MipGenerator}: which formats get mipmaps, and how they are filtered. */
class MipGeneratorTest {
    @Test
    void floatTargetsGetMipmapsAndOnlyGuaranteedFormatsAreFilteredLinearly() {
        assertTrue(MipGenerator.generated(GpuFormat.RGBA16_FLOAT));
        assertTrue(MipGenerator.generated(GpuFormat.RGBA32_FLOAT));
        assertTrue(MipGenerator.generated(GpuFormat.RGBA8_SNORM));
        assertFalse(MipGenerator.generated(GpuFormat.RGBA16_UINT), "the blit writes floats; integer targets keep their base level only");
        assertFalse(MipGenerator.generated(GpuFormat.RGB10A2_UINT));
        assertTrue(TextureFormats.filterable(GpuFormat.RGBA8_UNORM));
        assertTrue(TextureFormats.filterable(GpuFormat.RG11B10_FLOAT));
        assertFalse(TextureFormats.filterable(GpuFormat.RGBA32_FLOAT), "linear filtering of 32-bit floats is optional in Vulkan");
        assertFalse(TextureFormats.filterable(GpuFormat.RGBA16_UNORM));
    }
}
