package dev.shaderbridge.render.targets;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.TextureSource;
import dev.shaderbridge.render.pipeline.TextureFormats;
import java.util.Locale;
import java.util.Optional;

/**
 * Whether a raw custom texture ({@code texture.<stage>.<name>=<file> TEXTURE_2D <format> ...})
 * can be uploaded through renderpearl as is: only two-dimensional textures (2D and rectangle)
 * whose file layout (GL pixel format times pixel type) is exactly the texel layout of the texture
 * format. 1D and 3D textures and layouts that need conversion are left to the raw path.
 */
public final class RawTextureLayout {
    private RawTextureLayout() {
    }

    /**
     * An uploadable raw texture.
     *
     * @param format the texture format
     * @param width  width in texels
     * @param height height in texels
     */
    public record Upload(GpuFormat format, int width, int height) {
        /** @return bytes the file must hold */
        public long byteSize() {
            return (long) width * height * format.blockSize();
        }
    }

    /**
     * @param raw a raw texture source
     * @return how to upload it, or empty if renderpearl cannot take it as is
     */
    public static Optional<Upload> of(TextureSource.Raw raw) {
        String target = raw.target().toLowerCase(Locale.ROOT);
        if (!target.equals("2d") && !target.equals("2d_rect") || raw.size().size() < 2) {
            return Optional.empty();
        }
        GpuFormat format = TextureFormats.renderable(raw.format());
        int bytes = bytesPerPixel(raw.pixelFormat(), raw.pixelType());
        int width = raw.size().get(0);
        int height = raw.size().get(1);
        if (bytes != format.blockSize() || width <= 0 || height <= 0) {
            return Optional.empty();
        }
        return Optional.of(new Upload(format, width, height));
    }

    /**
     * @param pixelFormat GL pixel format name ({@code RGBA}, {@code RED_INTEGER}, ...)
     * @param pixelType   GL pixel type name ({@code UNSIGNED_BYTE}, {@code FLOAT}, ...)
     * @return bytes per pixel, or 0 for unknown, packed or swizzled ({@code BGRA}) layouts
     */
    static int bytesPerPixel(String pixelFormat, String pixelType) {
        int channels = switch (pixelFormat.toUpperCase(Locale.ROOT).replace("_INTEGER", "")) {
            case "RED", "R", "GREEN", "BLUE", "ALPHA", "LUMINANCE" -> 1;
            case "RG" -> 2;
            case "RGB" -> 3;
            case "RGBA" -> 4;
            default -> 0;
        };
        int size = switch (pixelType.toUpperCase(Locale.ROOT)) {
            case "BYTE", "UNSIGNED_BYTE" -> 1;
            case "SHORT", "UNSIGNED_SHORT", "HALF_FLOAT" -> 2;
            case "INT", "UNSIGNED_INT", "FLOAT" -> 4;
            default -> 0;
        };
        return channels * size;
    }
}
