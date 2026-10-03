package dev.shaderbridge.render.targets;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.blaze3d.platform.NativeImage;
import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.render.RenderFixture;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.zip.CRC32;
import java.util.zip.Deflater;
import com.mojang.blaze3d.systems.SamplerCache;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import org.junit.jupiter.api.Test;

/** Loads the glimmer fixture's textures through a fake device and resolves its bindings. */
class PackTexturesTest {
    private static final RenderFixture GLIMMER = RenderFixture.load(RenderFixture.GLIMMER);

    /** A 1x1 RGBA PNG. */
    static byte[] png(int rgba) throws IOException {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        out.write(new byte[] {(byte) 0x89, 'P', 'N', 'G', '\r', '\n', 0x1a, '\n'});
        chunk(out, "IHDR", ByteBuffer.allocate(13).putInt(1).putInt(1).put((byte) 8).put((byte) 6).put((byte) 0).put((byte) 0).put((byte) 0).array());
        Deflater deflater = new Deflater();
        deflater.setInput(ByteBuffer.allocate(5).put((byte) 0).putInt(rgba).array());
        deflater.finish();
        byte[] compressed = new byte[64];
        int n = deflater.deflate(compressed);
        chunk(out, "IDAT", java.util.Arrays.copyOf(compressed, n));
        chunk(out, "IEND", new byte[0]);
        return out.toByteArray();
    }

    private static void chunk(ByteArrayOutputStream out, String type, byte[] data) throws IOException {
        out.write(ByteBuffer.allocate(4).putInt(data.length).array());
        byte[] t = type.getBytes(java.nio.charset.StandardCharsets.US_ASCII);
        out.write(t);
        out.write(data);
        CRC32 crc = new CRC32();
        crc.update(t);
        crc.update(data);
        out.write(ByteBuffer.allocate(4).putInt((int) crc.getValue()).array());
    }

    /** Reads pack files from a map and has no resource packs. */
    private record MapReader(Map<String, byte[]> files) implements TextureReader {
        @Override
        public Optional<byte[]> packFile(String path) {
            return Optional.ofNullable(files.get(path));
        }

        @Override
        public Optional<NativeImage> resource(String location) {
            return Optional.empty();
        }
    }

    @Test
    void loadsCustomTexturesAndBindsEveryResource() throws IOException {
        FakeGpu gpu = new FakeGpu();
        List<String> warnings = new ArrayList<>();
        Map<String, byte[]> files = Map.of("textures/perlinNoise.png", png(0x80808080), "textures/blueNoise.png", png(0x102030FF));
        PackTextures textures = PackTextures.load(gpu.device(), gpu.encoder(), GLIMMER.dim(), new MapReader(files), warnings::add);
        // perlin and blue noise load; the raw 3D LUT needs the raw path; caustics is missing.
        assertEquals(2, warnings.size(), warnings.toString());
        assertTrue(warnings.stream().anyMatch(w -> w.contains("tonyMcMapfaceTex") && w.contains("raw Vulkan path")), warnings.toString());
        assertTrue(warnings.stream().anyMatch(w -> w.contains("causticsTex") && w.contains("not available")), warnings.toString());
        PackTextures.Loaded perlin = assertInstanceOf(PackTextures.Loaded.class, textures.custom("custom.perlinNoiseTex").orElseThrow());
        assertEquals(GpuFormat.RGBA8_UNORM, perlin.texture().getFormat());
        assertSame(textures.white(), ((PackTextures.Loaded) textures.custom("custom.causticsTex").orElseThrow()).view());
        // Generated noise at noiseTextureResolution (no texture.noise in glimmer).
        assertEquals(256, ((FakeGpu.View) textures.noise()).getWidth(0));
        assertEquals(7, gpu.commands("writeToTexture").size(), "noise, white, black, flat normal, no specular, two custom textures");

        PackTargets targets = new PackTargets(gpu.device(), GLIMMER.dim(), 64, 64, GpuFormat.D32_FLOAT, f -> 16384);
        List<String> missing = new ArrayList<>();
        TextureResolver resolver = new TextureResolver(GLIMMER.dim(), targets, textures, new SamplerCache(), DepthMode.REVERSED_ZERO_TO_ONE, missing::add);
        Program finalPass = GLIMMER.program("world0/final", "fullscreen");
        HostStub host = new HostStub(gpu);
        assertSame(targets.color(2).orElseThrow().sampleView(true), resolver.resolve(new ResourceRef.ColorTex(2), true, finalPass, host).view());
        assertSame(targets.color(2).orElseThrow().sampleView(false), resolver.resolve(new ResourceRef.ColorTex(2), false, finalPass, host).view());
        assertSame(host.mainDepth, resolver.resolve(new ResourceRef.DepthTex(0), false, finalPass, host).view());
        assertSame(targets.depthCopyView(1), resolver.resolve(new ResourceRef.DepthTex(1), false, finalPass, host).view());
        assertSame(targets.shadowDepthView(1), resolver.resolve(new ResourceRef.ShadowTexHw(1), false, finalPass, host).view());
        assertSame(textures.noise(), resolver.resolve(new ResourceRef.Noise(), false, finalPass, host).view());
        assertSame(perlin.view(), resolver.resolve(new ResourceRef.CustomTexture("custom.perlinNoiseTex"), false, finalPass, host).view());
        assertSame(textures.flatNormal(), resolver.resolve(new ResourceRef.Normals(), false, finalPass, host).view());
        assertSame(textures.black(), resolver.resolve(new ResourceRef.DhDepthTex(0), false, finalPass, host).view(), "far plane in reversed-Z");
        assertSame(targets.color(0).orElseThrow().sampleView(false), resolver.resolve(new ResourceRef.Unknown("voxelMap"), false, finalPass, host).view());
        assertEquals(List.of(), missing);
        assertSame(textures.black(), resolver.resolve(new ResourceRef.ColorTex(31), false, finalPass, host).view());
        assertSame(textures.black(), resolver.resolve(new ResourceRef.Image("skyViewLUT"), false, finalPass, host).view());
        assertEquals(2, missing.size(), missing.toString());
        targets.close();
        textures.close();
        assertTrue(gpu.textures.stream().allMatch(t -> t.closed));
    }

    /** Host textures backed by fake views. */
    private static final class HostStub implements HostTextures {
        final GpuTextureView mainDepth;
        final TextureBinding atlas;

        HostStub(FakeGpu gpu) {
            FakeGpu.Texture depth = new FakeGpu.Texture("depth", 15, GpuFormat.D32_FLOAT, 64, 64, 1);
            mainDepth = new FakeGpu.View(depth, 0, 1, new boolean[1]);
            FakeGpu.Texture atlasTexture = new FakeGpu.Texture("atlas", 15, GpuFormat.RGBA8_UNORM, 64, 64, 1);
            atlas = new TextureBinding(new FakeGpu.View(atlasTexture, 0, 1, new boolean[1]), null);
        }

        @Override
        public TextureBinding atlas() {
            return atlas;
        }

        @Override
        public TextureBinding lightmap() {
            return atlas;
        }

        @Override
        public TextureBinding overlay() {
            return atlas;
        }

        @Override
        public Optional<TextureBinding> normals() {
            return Optional.empty();
        }

        @Override
        public Optional<TextureBinding> specular() {
            return Optional.empty();
        }

        @Override
        public GpuTextureView mainDepth() {
            return mainDepth;
        }

        @Override
        public Optional<GpuTextureView> dhDepth(int index) {
            return Optional.empty();
        }

        @Override
        public Optional<TextureBinding> dhBlockAtlas() {
            return Optional.empty();
        }
    }

    @Test
    void pngHelperIsReadable() throws IOException {
        try (NativeImage image = NativeImage.read(png(0x11223344))) {
            assertEquals(1, image.getWidth());
            assertEquals(NativeImage.Format.RGBA, image.format());
        }
        assertEquals(ByteOrder.BIG_ENDIAN, ByteBuffer.allocate(1).order());
    }
}
