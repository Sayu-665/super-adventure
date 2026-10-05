package dev.shaderbridge.render.raw;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.CustomImage;
import dev.shaderbridge.model.ImageSize;
import dev.shaderbridge.model.StorageBuffer;
import dev.shaderbridge.model.TextureFormat;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import org.junit.jupiter.api.Test;

class ResourceSizesTest {
    private static final ResourceSizes.ImageLimits LIMITS = new ResourceSizes.ImageLimits(16384, 16384, 2048);

    private static CustomImage image(ImageSize size, boolean clear) {
        return new CustomImage("lut", "lutTex", TextureFormat.RGBA16F, "RGBA", "HALF_FLOAT", clear, size);
    }

    @Test
    void customImagesHaveTheirDeclaredDimensionality() {
        List<String> warnings = new ArrayList<>();
        assertEquals(Optional.of(new ResourceSizes.ImageSpec("lut", 1, 256, 1, 1, TextureFormat.RGBA16F, false, false)),
            ResourceSizes.image(image(new ImageSize.Absolute1D(256), false), 1920, 1080, LIMITS, warnings::add));
        assertEquals(Optional.of(new ResourceSizes.ImageSpec("lut", 3, 32, 32, 32, TextureFormat.RGBA16F, true, false)),
            ResourceSizes.image(image(new ImageSize.Absolute3D(32, 32, 32), true), 1920, 1080, LIMITS, warnings::add));
        assertEquals(List.of(), warnings);
    }

    @Test
    void relativeImagesTruncateLikeRenderTargetsAndIris() {
        assertEquals(Optional.of(new ResourceSizes.ImageSpec("lut", 2, 960, 360, 1, TextureFormat.RGBA16F, false, true)),
            ResourceSizes.image(image(new ImageSize.Relative(0.5f, 1f / 3), false), 1921, 1081, LIMITS, m -> { }));
    }

    @Test
    void imagesBeyondTheDeviceLimitsAreNotCreated() {
        List<String> warnings = new ArrayList<>();
        assertEquals(Optional.empty(), ResourceSizes.image(image(new ImageSize.Absolute3D(4096, 4, 4), false), 1, 1, LIMITS, warnings::add));
        assertEquals(Optional.empty(), ResourceSizes.image(image(new ImageSize.Absolute2D(0, 4), false), 1, 1, LIMITS, warnings::add));
        assertEquals(2, warnings.size());
    }

    @Test
    void imageLimitsApplyPerDimensionality() {
        ResourceSizes.ImageLimits limits = new ResourceSizes.ImageLimits(4096, 8192, 256);
        assertEquals(256, limits.max(3));
        assertTrue(limits.fits(1, 4096, 1, 1));
        assertFalse(limits.fits(1, 4097, 1, 1));
        assertTrue(limits.fits(3, 256, 256, 256));
        assertFalse(limits.fits(3, 256, 257, 1));
        assertFalse(limits.fits(2, 0, 16, 1), "empty extents never fit");
    }

    @Test
    void storageBuffersCoverTheirLargestBlockWithinTheRangeLimit() {
        List<String> warnings = new ArrayList<>();
        assertEquals(40, ResourceSizes.storageBuffer(new StorageBuffer(0, 40, null, null), 1920, 1080, 36, 1 << 27, warnings::add));
        assertEquals(List.of(), warnings);
        assertEquals(640, ResourceSizes.storageBuffer(new StorageBuffer(0, 636, null, null), 1920, 1080, 640, 1 << 27, warnings::add),
            "arc-shader declares a 640-byte block over a 636-byte buffer");
        assertEquals(1 << 27, ResourceSizes.storageBuffer(new StorageBuffer(1, 1L << 30, null, null), 1, 1, 0, 1 << 27, warnings::add));
        assertEquals(ResourceSizes.MIN_BUFFER, ResourceSizes.storageBuffer(new StorageBuffer(2, 4, null, null), 1, 1, 0, 1 << 27, warnings::add));
        assertEquals(2, warnings.size(), warnings.toString());
    }

    @Test
    void relativeStorageBuffersHoldBytesPerPixel() {
        assertEquals(16L * 960 * 540, ResourceSizes.storageBuffer(new StorageBuffer(0, 16, List.of(0.5f, 0.5f), null), 1920, 1080, 0, 1L << 31,
            m -> { }));
        assertTrue(ResourceSizes.storageBuffer(new StorageBuffer(0, Long.MAX_VALUE / 2, List.of(1f, 1f), null), 1920, 1080, 0, 1L << 31,
            m -> { }) == 1L << 31, "overflow saturates, then clamps");
    }
}
