package dev.shaderbridge.render.targets;

import com.mojang.blaze3d.platform.NativeImage;
import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.model.CustomTexture;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.TextureSource;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.util.HashMap;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.function.Consumer;
import org.lwjgl.system.MemoryUtil;

/**
 * The textures a pack samples besides its render targets: {@code noisetex} (the pack's
 * {@code texture.noise} or {@link NoiseTexture generated noise}), the custom textures, opaque
 * white and black 1x1 textures, and the 1x1 defaults Iris binds when no PBR atlases are loaded
 * (a flat normal {@code (0.5, 0.5, 1, 1)} and zero specular data). A texture that cannot be loaded is reported to the warning sink
 * and replaced by white (as Iris does for missing custom textures); dynamic lightmap textures are
 * bound to Minecraft's lightmap at draw time. Render thread only.
 */
public final class PackTextures implements AutoCloseable {
    /** Usage of uploaded textures. */
    static final int USAGE = GpuTexture.USAGE_TEXTURE_BINDING | GpuTexture.USAGE_COPY_DST;

    private final Map<String, Custom> custom = new HashMap<>();
    private final Loaded noise;
    private final Loaded white;
    private final Loaded black;
    private final Loaded flatNormal;
    private final Loaded noSpecular;

    /** Where a custom texture's pixels come from at draw time. */
    public sealed interface Custom {
        /** The pack's dynamic lightmap texture ({@code minecraft:dynamic/lightmap_1}): Minecraft's lightmap. */
        record HostLightmap() implements Custom {
        }
    }

    /**
     * A texture ShaderBridge uploaded.
     *
     * @param texture the texture
     * @param view    its view
     */
    public record Loaded(GpuTexture texture, GpuTextureView view) implements Custom, AutoCloseable {
        @Override
        public void close() {
            view.close();
            texture.close();
        }
    }

    private PackTextures(Loaded noise, Loaded white, Loaded black, Loaded flatNormal, Loaded noSpecular) {
        this.noise = noise;
        this.white = white;
        this.black = black;
        this.flatNormal = flatNormal;
        this.noSpecular = noSpecular;
    }

    /**
     * Loads and uploads every texture of a dimension pipeline.
     *
     * @param device   the GPU device
     * @param encoder  the command encoder for the uploads
     * @param dim      the dimension pipeline
     * @param reader   reads pack files and resources
     * @param warnings receives one message per texture that could not be loaded
     * @return the textures
     */
    public static PackTextures load(GpuDevice device, CommandEncoder encoder, DimensionPipeline dim, TextureReader reader, Consumer<String> warnings) {
        Uploader up = new Uploader(device, encoder);
        Loaded white = up.solid("white", 0xFFFFFFFF);
        PackTextures textures = new PackTextures(up.noise(dim, reader, warnings), white, up.solid("black", 0xFF000000), up.solid("flat normal", 0xFF8080FF),
            up.solid("no specular", 0x00000000));
        for (CustomTexture t : dim.targets().customTextures()) {
            String id = CustomTextureIds.id(t);
            try {
                textures.custom.put(id, up.custom(t, reader).orElseGet(() -> {
                    warnings.accept("custom texture " + id + " (" + describe(t.source()) + ") is not available; white is bound instead");
                    return white;
                }));
            } catch (IOException | RuntimeException e) {
                warnings.accept("custom texture " + id + " (" + describe(t.source()) + ") cannot be loaded: " + e.getMessage() + "; white is bound instead");
                textures.custom.put(id, white);
            }
        }
        return textures;
    }

    /** @return {@code noisetex} */
    public GpuTextureView noise() {
        return noise.view();
    }

    /** @return opaque white, 1x1 */
    public GpuTextureView white() {
        return white.view();
    }

    /** @return opaque black, 1x1 */
    public GpuTextureView black() {
        return black.view();
    }

    /** @return the default normal map texel {@code (0.5, 0.5, 1, 1)}, 1x1 */
    public GpuTextureView flatNormal() {
        return flatNormal.view();
    }

    /** @return the default specular map texel {@code (0, 0, 0, 0)}, 1x1 */
    public GpuTextureView noSpecular() {
        return noSpecular.view();
    }

    /**
     * @param id a {@code ResourceRef.CustomTexture} id ({@link CustomTextureIds})
     * @return the texture, if the pack declares it
     */
    public Optional<Custom> custom(String id) {
        return Optional.ofNullable(custom.get(id));
    }

    @Override
    public void close() {
        custom.values().stream().filter(c -> c instanceof Loaded l && l != white).forEach(c -> ((Loaded) c).close());
        custom.clear();
        noise.close();
        white.close();
        black.close();
        flatNormal.close();
        noSpecular.close();
    }

    private static String describe(TextureSource source) {
        return switch (source) {
            case TextureSource.PackImage p -> p.path();
            case TextureSource.Resource r -> r.location();
            case TextureSource.Dynamic d -> d.name();
            case TextureSource.Raw r -> r.path() + " (raw " + r.target() + ")";
        };
    }

    /** Creates and fills textures. */
    private record Uploader(GpuDevice device, CommandEncoder encoder) {
        Loaded solid(String name, int argb) {
            try (NativeImage image = new NativeImage(1, 1, false)) {
                image.setPixel(0, 0, argb);
                return upload("ShaderBridge " + name, image);
            }
        }

        Loaded noise(DimensionPipeline dim, TextureReader reader, Consumer<String> warnings) {
            TextureSource source = dim.targets().noiseTexture();
            if (source != null) {
                try {
                    Optional<NativeImage> image = image(source, reader);
                    if (image.isPresent()) {
                        try (NativeImage owned = image.get()) {
                            return upload("ShaderBridge noisetex", owned);
                        }
                    }
                    warnings.accept("texture.noise (" + describe(source) + ") is not available; generated noise is used instead");
                } catch (IOException | RuntimeException e) {
                    warnings.accept("texture.noise (" + describe(source) + ") cannot be loaded: " + e.getMessage() + "; generated noise is used instead");
                }
            }
            int[] pixels = NoiseTexture.argb(dim.targets().noiseTextureResolution());
            int size = (int) Math.round(Math.sqrt(pixels.length));
            try (NativeImage image = new NativeImage(size, size, false)) {
                for (int y = 0; y < size; y++) {
                    for (int x = 0; x < size; x++) {
                        image.setPixel(x, y, pixels[y * size + x]);
                    }
                }
                return upload("ShaderBridge noisetex", image);
            }
        }

        Optional<Custom> custom(CustomTexture t, TextureReader reader) throws IOException {
            if (t.source() instanceof TextureSource.Dynamic d) {
                return d.name().toLowerCase(Locale.ROOT).contains("lightmap") ? Optional.of(new Custom.HostLightmap()) : Optional.empty();
            }
            if (t.source() instanceof TextureSource.Raw raw) {
                return raw(raw, reader);
            }
            Optional<NativeImage> image = image(t.source(), reader);
            if (image.isEmpty()) {
                return Optional.empty();
            }
            try (NativeImage owned = image.get()) {
                return Optional.of(upload("ShaderBridge " + CustomTextureIds.id(t), owned));
            }
        }

        private Optional<Custom> raw(TextureSource.Raw raw, TextureReader reader) throws IOException {
            Optional<RawTextureLayout.Upload> layout = RawTextureLayout.of(raw);
            if (layout.isEmpty()) {
                throw new IOException("raw " + raw.target() + " " + raw.pixelFormat() + "/" + raw.pixelType() + " textures need the raw Vulkan path");
            }
            Optional<byte[]> bytes = reader.packFile(raw.path());
            if (bytes.isEmpty()) {
                return Optional.empty();
            }
            RawTextureLayout.Upload u = layout.get();
            if (bytes.get().length < u.byteSize()) {
                throw new IOException(raw.path() + " holds " + bytes.get().length + " bytes, " + u.byteSize() + " are needed");
            }
            GpuTexture texture = device.createTexture("ShaderBridge raw " + raw.path(), USAGE, u.format(), u.width(), u.height(), 1, 1);
            ByteBuffer staging = MemoryUtil.memAlloc((int) u.byteSize());
            try {
                staging.put(bytes.get(), 0, (int) u.byteSize()).flip();
                encoder.writeToTexture(texture, staging, 0, 0, 0, 0, u.width(), u.height());
                return Optional.of(new Loaded(texture, device.createTextureView(texture)));
            } catch (RuntimeException e) {
                texture.close();
                throw e;
            } finally {
                MemoryUtil.memFree(staging);
            }
        }

        private static Optional<NativeImage> image(TextureSource source, TextureReader reader) throws IOException {
            return switch (source) {
                case TextureSource.PackImage p -> reader.packImage(p.path());
                case TextureSource.Resource r -> reader.resource(r.location());
                default -> Optional.empty();
            };
        }

        private Loaded upload(String label, NativeImage image) {
            GpuTexture texture = device.createTexture(label, USAGE, GpuFormat.RGBA8_UNORM, image.getWidth(), image.getHeight(), 1, 1);
            encoder.writeToTexture(texture, image);
            return new Loaded(texture, device.createTextureView(texture));
        }
    }
}
