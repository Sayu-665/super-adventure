package dev.shaderbridge.render.targets;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.TextureFormat;
import dev.shaderbridge.model.TextureSource;
import java.util.List;
import java.util.Random;
import org.junit.jupiter.api.Test;

class TextureDataTest {
    @Test
    void noiseIsIrisNoiseColumnByColumn() {
        int[] pixels = NoiseTexture.argb(4);
        assertEquals(16, pixels.length);
        Random reference = new Random(0);
        for (int x = 0; x < 4; x++) {
            for (int y = 0; y < 4; y++) {
                assertEquals(reference.nextInt() | 0xFF000000, pixels[y * 4 + x]);
            }
        }
        assertEquals(1, NoiseTexture.argb(0).length);
        assertEquals(NoiseTexture.MAX_RESOLUTION * NoiseTexture.MAX_RESOLUTION, NoiseTexture.argb(100_000).length);
    }

    private static TextureSource.Raw raw(String target, TextureFormat format, List<Integer> size, String pixelFormat, String pixelType) {
        return new TextureSource.Raw("lut.bin", target, size.size(), format, size, pixelFormat, pixelType);
    }

    @Test
    void rawTexturesUploadOnlyWhenTheFileLayoutIsTheTextureLayout() {
        RawTextureLayout.Upload rgba8 = RawTextureLayout.of(raw("2d", TextureFormat.RGBA8, List.of(4, 2), "RGBA", "UNSIGNED_BYTE")).orElseThrow();
        assertEquals(new RawTextureLayout.Upload(GpuFormat.RGBA8_UNORM, 4, 2), rgba8);
        assertEquals(32, rgba8.byteSize());
        assertEquals(GpuFormat.R32_FLOAT, RawTextureLayout.of(raw("2d_rect", TextureFormat.R32F, List.of(8, 8), "RED", "FLOAT")).orElseThrow().format());
        assertEquals(GpuFormat.RG16_SINT, RawTextureLayout.of(raw("2d", TextureFormat.RG16I, List.of(8, 8), "RG_INTEGER", "SHORT")).orElseThrow().format());
        assertTrue(RawTextureLayout.of(raw("3d", TextureFormat.RGBA32F, List.of(48, 48, 48), "RGBA", "FLOAT")).isEmpty(), "3D");
        assertTrue(RawTextureLayout.of(raw("1d", TextureFormat.RGBA8, List.of(64), "RGBA", "UNSIGNED_BYTE")).isEmpty(), "1D");
        assertTrue(RawTextureLayout.of(raw("2d", TextureFormat.RGB8, List.of(4, 4), "RGB", "UNSIGNED_BYTE")).isEmpty(), "RGB8 is stored as RGBA8");
        assertTrue(RawTextureLayout.of(raw("2d", TextureFormat.RGBA8, List.of(4, 4), "BGRA", "UNSIGNED_BYTE")).isEmpty(), "swizzled");
        assertTrue(RawTextureLayout.of(raw("2d", TextureFormat.RGBA8, List.of(0, 4), "RGBA", "UNSIGNED_BYTE")).isEmpty(), "empty");
        assertEquals(0, RawTextureLayout.bytesPerPixel("RGBA", "UNSIGNED_INT_8_8_8_8"));
        assertEquals(8, RawTextureLayout.bytesPerPixel("rgba", "half_float"));
    }
}
