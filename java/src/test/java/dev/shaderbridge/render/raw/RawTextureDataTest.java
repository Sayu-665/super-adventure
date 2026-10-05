package dev.shaderbridge.render.raw;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.CustomTexture;
import dev.shaderbridge.model.TextureFormat;
import dev.shaderbridge.model.TextureSource;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.Arrays;
import java.util.List;
import org.junit.jupiter.api.Test;

class RawTextureDataTest {
    private static CustomTexture raw(String target, TextureFormat format, List<Integer> size, String pixelFormat, String pixelType) {
        int dimensions = target.startsWith("1") ? 1 : target.startsWith("3") ? 3 : 2;
        return new CustomTexture("tex", "composite", new TextureSource.Raw("lut.dat", target, dimensions, format, size, pixelFormat, pixelType),
            true, false);
    }

    private static RawTextureData.Upload converted(CustomTexture texture, byte[] data) {
        return assertInstanceOf(RawTextureData.Result.Converted.class, RawTextureData.convert(texture, data)).upload();
    }

    private static String unsupported(CustomTexture texture, byte[] data) {
        return assertInstanceOf(RawTextureData.Result.Unsupported.class, RawTextureData.convert(texture, data)).reason();
    }

    private static byte[] bytes(ByteBuffer buffer) {
        byte[] out = new byte[buffer.remaining()];
        buffer.duplicate().get(out);
        return out;
    }

    private static byte[] sequence(int length) {
        byte[] data = new byte[length];
        for (int i = 0; i < length; i++) {
            data[i] = (byte) i;
        }
        return data;
    }

    @Test
    void onlyTexturesRenderpearlCannotUploadAreNeeded() {
        assertTrue(RawTextureData.needed(raw("3d", TextureFormat.RGBA8, List.of(2, 2, 2), "RGBA", "UNSIGNED_BYTE")));
        assertTrue(RawTextureData.needed(raw("1d", TextureFormat.RGBA8, List.of(4), "RGBA", "UNSIGNED_BYTE")));
        assertTrue(RawTextureData.needed(raw("2d", TextureFormat.RGB8, List.of(2, 2), "RGB", "UNSIGNED_BYTE")), "RGB needs expansion");
        assertFalse(RawTextureData.needed(raw("2d", TextureFormat.RGBA8, List.of(2, 2), "RGBA", "UNSIGNED_BYTE")));
        assertFalse(RawTextureData.needed(new CustomTexture("tex", "composite", new TextureSource.PackImage("tex.png"), false, true)));
    }

    @Test
    void matchingLayoutsAreCopiedAsIs() {
        byte[] data = sequence(2 * 2 * 2 * 4 + 5);
        RawTextureData.Upload upload = converted(raw("3d", TextureFormat.RGBA8, List.of(2, 2, 2), "RGBA", "UNSIGNED_BYTE"), data);
        assertEquals("composite.tex.3d", upload.id());
        assertEquals(3, upload.dimensions());
        assertEquals(List.of(2, 2, 2), List.of(upload.width(), upload.height(), upload.depth()));
        assertEquals(GpuFormat.RGBA8_UNORM, upload.format());
        assertTrue(upload.linear());
        assertTrue(upload.repeat());
        assertArrayEquals(Arrays.copyOf(data, 32), bytes(upload.texels()), "trailing file bytes are ignored");
    }

    @Test
    void missingComponentsAreFilledAsGlDoes() {
        ByteBuffer half = ByteBuffer.allocate(12).order(ByteOrder.LITTLE_ENDIAN);
        for (short h : new short[] {0x3800, 0x3400, 0x3000, 0x2C00, 0x2800, 0x2400}) {
            half.putShort(h);
        }
        RawTextureData.Upload upload = converted(raw("1d", TextureFormat.RGB16F, List.of(2), "RGB", "HALF_FLOAT"), half.array());
        assertEquals(1, upload.dimensions());
        assertEquals(List.of(2, 1, 1), List.of(upload.width(), upload.height(), upload.depth()));
        assertEquals(GpuFormat.RGBA16_FLOAT, upload.format());
        ByteBuffer texels = upload.texels().order(ByteOrder.LITTLE_ENDIAN);
        short[] out = new short[8];
        texels.asShortBuffer().get(out);
        assertArrayEquals(new short[] {0x3800, 0x3400, 0x3000, 0x3C00, 0x2C00, 0x2800, 0x2400, 0x3C00}, out);
    }

    @Test
    void singleChannelsExpandWithZeroGreenBlueAndOpaqueAlpha() {
        RawTextureData.Upload upload = converted(raw("2d", TextureFormat.RGBA8, List.of(2, 1), "RED", "UNSIGNED_BYTE"), new byte[] {10, 20});
        assertArrayEquals(new byte[] {10, 0, 0, (byte) 0xFF, 20, 0, 0, (byte) 0xFF}, bytes(upload.texels()));
    }

    @Test
    void integerFormatsGetAlphaOne() {
        RawTextureData.Upload upload = converted(raw("3d", TextureFormat.RGBA8UI, List.of(1, 1, 1), "RG_INTEGER", "UNSIGNED_BYTE"),
            new byte[] {7, 9});
        assertEquals(GpuFormat.RGBA8_UINT, upload.format());
        assertArrayEquals(new byte[] {7, 9, 0, 1}, bytes(upload.texels()));
    }

    @Test
    void bgrLayoutsAreSwizzled() {
        RawTextureData.Upload bgra = converted(raw("3d", TextureFormat.RGBA8, List.of(1, 1, 1), "BGRA", "UNSIGNED_BYTE"), new byte[] {1, 2, 3, 4});
        assertArrayEquals(new byte[] {3, 2, 1, 4}, bytes(bgra.texels()));
        RawTextureData.Upload bgr = converted(raw("3d", TextureFormat.RGB8, List.of(1, 1, 1), "BGR", "UNSIGNED_BYTE"), new byte[] {1, 2, 3});
        assertArrayEquals(new byte[] {3, 2, 1, (byte) 0xFF}, bytes(bgr.texels()));
    }

    @Test
    void convertedTexturesKeepTheirSamplingSettings() {
        CustomTexture nearestClamped = new CustomTexture("tex", "composite",
            new TextureSource.Raw("lut.dat", "3d", 3, TextureFormat.R8, List.of(1, 1, 1), "RED", "UNSIGNED_BYTE"), false, true);
        RawTextureData.Upload upload = converted(nearestClamped, new byte[] {1});
        assertFalse(upload.linear());
        assertFalse(upload.repeat());
    }

    @Test
    void unconvertibleTexturesAreReported() {
        assertTrue(unsupported(raw("3d", TextureFormat.RGBA8, List.of(1, 1, 1), "RGBA", "FLOAT"), new byte[16]).contains("RGBA/FLOAT"));
        assertTrue(unsupported(raw("3d", TextureFormat.RGBA8, List.of(1, 1, 1), "LUMINANCE", "UNSIGNED_BYTE"), new byte[4])
            .contains("LUMINANCE"));
        assertTrue(unsupported(raw("3d", TextureFormat.RGBA8, List.of(2, 2, 2), "RGBA", "UNSIGNED_BYTE"), new byte[31])
            .contains("31 bytes, 32 are needed"));
        assertTrue(unsupported(raw("3d", TextureFormat.RGBA8, List.of(2, 2), "RGBA", "UNSIGNED_BYTE"), new byte[16]).contains("empty"));
        assertTrue(unsupported(raw("2d", TextureFormat.RGBA8, List.of(0, 2), "RGBA", "UNSIGNED_BYTE"), new byte[16]).contains("empty"));
    }

    @Test
    void onlyRawTexturesConvert() {
        CustomTexture image = new CustomTexture("tex", "composite", new TextureSource.PackImage("tex.png"), false, false);
        assertThrows(IllegalArgumentException.class, () -> RawTextureData.convert(image, new byte[0]));
    }
}
