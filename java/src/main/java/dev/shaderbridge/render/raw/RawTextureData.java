package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.CustomTexture;
import dev.shaderbridge.model.TextureSource;
import dev.shaderbridge.render.pipeline.TextureFormats;
import dev.shaderbridge.render.targets.CustomTextureIds;
import dev.shaderbridge.render.targets.RawTextureLayout;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.List;
import java.util.Locale;

/**
 * Raw custom textures ({@code texture.<stage>.<name>=<file> TEXTURE_3D <format> <size> <pixel format>
 * <pixel type>}) that only the raw path can provide: 1D and 3D ones, and 2D ones whose file layout
 * is not the texture format's texel layout (Mojang's textures are 2D and uploaded as is), with
 * their texels converted to the renderable format: components are copied when the file's
 * component type is the format's (unsigned bytes for UNORM/UINT formats, half floats for 16-bit
 * float formats, ...), {@code BGR(A)} is swizzled, and missing components are filled as GL does
 * (green and blue 0, alpha 1).
 */
public final class RawTextureData {
    private RawTextureData() {
    }

    /**
     * A raw texture ready for upload.
     *
     * @param id         its {@code ResourceRef.CustomTexture} id
     * @param dimensions 1, 2 or 3
     * @param width      width in texels
     * @param height     height in texels
     * @param depth      depth in texels
     * @param format     the format the texels are in
     * @param linear     sampled with linear filtering ({@code blur})
     * @param repeat     sampled with repeat addressing (not {@code clamp})
     * @param texels     the texels, tightly packed, little-endian
     */
    public record Upload(String id, int dimensions, int width, int height, int depth, GpuFormat format, boolean linear, boolean repeat,
                         ByteBuffer texels) {
    }

    /**
     * @param texture a custom texture
     * @return whether the raw path provides it (a raw texture renderpearl cannot upload as is)
     */
    public static boolean needed(CustomTexture texture) {
        return texture.source() instanceof TextureSource.Raw raw && RawTextureLayout.of(raw).isEmpty();
    }

    /**
     * @param texture a custom texture the raw path provides ({@link #needed})
     * @param data    the contents of its file
     * @return the texture to upload, or why it cannot be
     * @throws IllegalArgumentException if the texture is not raw
     */
    public static Result convert(CustomTexture texture, byte[] data) {
        if (!(texture.source() instanceof TextureSource.Raw raw)) {
            throw new IllegalArgumentException(texture.sampler() + " is not a raw texture");
        }
        String target = raw.target().toLowerCase(Locale.ROOT);
        int dimensions = switch (target) {
            case "1d" -> 1;
            case "3d" -> 3;
            default -> 2;
        };
        List<Integer> size = raw.size();
        int width = size.isEmpty() ? 0 : size.get(0);
        int height = dimensions == 1 ? 1 : size.size() > 1 ? size.get(1) : 0;
        int depth = dimensions == 3 && size.size() > 2 ? size.get(2) : dimensions == 3 ? 0 : 1;
        if (width < 1 || height < 1 || depth < 1) {
            return new Result.Unsupported("its size " + size + " is empty");
        }
        GpuFormat format = TextureFormats.renderable(raw.format());
        String pixelFormat = raw.pixelFormat().toUpperCase(Locale.ROOT).replace("_INTEGER", "");
        int channels = switch (pixelFormat) {
            case "RED", "R" -> 1;
            case "RG" -> 2;
            case "RGB", "BGR" -> 3;
            case "RGBA", "BGRA" -> 4;
            default -> 0;
        };
        if (channels == 0 || !sameComponents(raw.pixelType(), format.componentType())) {
            return new Result.Unsupported("its file layout " + raw.pixelFormat() + "/" + raw.pixelType() + " cannot be converted to " + format);
        }
        int componentSize = format.componentType().byteSize();
        long texels = (long) width * height * depth;
        long needed = texels * channels * componentSize;
        if (data.length < needed) {
            return new Result.Unsupported(raw.path() + " holds " + data.length + " bytes, " + needed + " are needed");
        }
        int targetChannels = format.componentCount();
        ByteBuffer out = ByteBuffer.allocateDirect(Math.toIntExact(texels * targetChannels * componentSize)).order(ByteOrder.LITTLE_ENDIAN);
        boolean bgr = pixelFormat.startsWith("BGR");
        if (channels == targetChannels && !bgr) {
            out.put(data, 0, out.capacity());
            return converted(texture, dimensions, width, height, depth, format, out.flip());
        }
        byte[] one = one(format.componentType());
        for (long t = 0; t < texels; t++) {
            for (int c = 0; c < targetChannels; c++) {
                int source = bgr && c < 3 ? 2 - c : c;
                if (source < channels) {
                    out.put(data, (int) ((t * channels + source) * componentSize), componentSize);
                } else {
                    out.put(c == 3 ? one : new byte[componentSize]);
                }
            }
        }
        return converted(texture, dimensions, width, height, depth, format, out.flip());
    }

    private static Result converted(CustomTexture texture, int dimensions, int width, int height, int depth, GpuFormat format, ByteBuffer texels) {
        return new Result.Converted(new Upload(CustomTextureIds.id(texture), dimensions, width, height, depth, format, texture.blur(),
            !texture.clamp(), texels));
    }

    /** The outcome of {@link #convert}. */
    public sealed interface Result {
        /** @param upload the texture to upload */
        record Converted(Upload upload) implements Result {
        }

        /** @param reason why the texture cannot be provided */
        record Unsupported(String reason) implements Result {
        }
    }

    /** Whether the file's GL pixel type stores exactly the format's components. */
    private static boolean sameComponents(String pixelType, GpuFormat.ComponentType format) {
        return switch (pixelType.toUpperCase(Locale.ROOT)) {
            case "UNSIGNED_BYTE" -> format == GpuFormat.ComponentType.UNORM_8 || format == GpuFormat.ComponentType.UINT_8;
            case "BYTE" -> format == GpuFormat.ComponentType.SNORM_8 || format == GpuFormat.ComponentType.SINT_8;
            case "UNSIGNED_SHORT" -> format == GpuFormat.ComponentType.UNORM_16 || format == GpuFormat.ComponentType.UINT_16;
            case "SHORT" -> format == GpuFormat.ComponentType.SNORM_16 || format == GpuFormat.ComponentType.SINT_16;
            case "HALF_FLOAT" -> format == GpuFormat.ComponentType.FLOAT_16;
            case "UNSIGNED_INT" -> format == GpuFormat.ComponentType.UINT_32;
            case "INT" -> format == GpuFormat.ComponentType.SINT_32;
            case "FLOAT" -> format == GpuFormat.ComponentType.FLOAT_32;
            default -> false;
        };
    }

    /** The little-endian bytes of 1 (full intensity for normalized types) in a component type. */
    private static byte[] one(GpuFormat.ComponentType type) {
        ByteBuffer b = ByteBuffer.allocate(type.byteSize()).order(ByteOrder.LITTLE_ENDIAN);
        switch (type) {
            case UNORM_8 -> b.put((byte) 0xFF);
            case SNORM_8 -> b.put((byte) 0x7F);
            case UINT_8, SINT_8 -> b.put((byte) 1);
            case UNORM_16 -> b.putShort((short) 0xFFFF);
            case SNORM_16 -> b.putShort((short) 0x7FFF);
            case UINT_16, SINT_16 -> b.putShort((short) 1);
            case FLOAT_16 -> b.putShort((short) 0x3C00);
            case UINT_32, SINT_32 -> b.putInt(1);
            case FLOAT_32 -> b.putFloat(1);
            default -> {
            }
        }
        return b.array();
    }
}
