package dev.shaderbridge.render;

import static org.junit.jupiter.api.Assertions.assertNotNull;

import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.CompiledPack;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.ModelJson;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.json.ModelParseException;
import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;

/**
 * Real compiled packs for the render tests: {@code shaderbridge compile <pack>/shaders --depth reversed
 * --target vulkan [--dim world0]} on two MIT-licensed packs of the corpus, reduced to their SPIR-V
 * blobs (glimmer-shaders by jbritain; "Tutorial 4 - Advanced Shadow Mapping" of fuzdex's
 * MinecraftShaderProgramming; licenses next to the files).
 *
 * <ul>
 *   <li>{@link #TUTORIAL4}: a small pack whose programs all suit renderpearl; shared gbuffers
 *   attachments {@code [0, 1, 2]}, Distant Horizons synthesized.</li>
 *   <li>{@link #GLIMMER}: a modern pack; most programs need the raw Vulkan path (SSBOs, storage
 *   images, compute), with custom and raw 3D textures, an absolute-size target and mipmaps.</li>
 * </ul>
 *
 * @param pack  the compiled pack
 * @param blobs its SPIR-V
 */
public record RenderFixture(CompiledPack pack, Blobs blobs) {
    /** Resource directory of Tutorial 4. */
    public static final String TUTORIAL4 = "tutorial4";
    /** Resource directory of glimmer-shaders (world0). */
    public static final String GLIMMER = "glimmer";

    /**
     * @param name {@link #TUTORIAL4} or {@link #GLIMMER}
     * @return the fixture
     */
    public static RenderFixture load(String name) {
        try {
            CompiledPack pack = ModelJson.parse(new String(read(name + "/pack.json"), StandardCharsets.UTF_8), CompiledPack.class);
            byte[] bytes = read(name + "/blobs.bin");
            ByteBuffer buffer = ByteBuffer.allocateDirect(bytes.length).order(ByteOrder.LITTLE_ENDIAN);
            buffer.put(bytes).flip();
            return new RenderFixture(pack, Blobs.of(buffer, pack.blobs()));
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        } catch (ModelParseException e) {
            throw new IllegalStateException(e);
        }
    }

    private static byte[] read(String path) throws IOException {
        try (InputStream in = RenderFixture.class.getResourceAsStream("/dev/shaderbridge/render/" + path)) {
            assertNotNull(in, path);
            return in.readAllBytes();
        }
    }

    /** @return the first (only) dimension pipeline */
    public DimensionPipeline dim() {
        return pack.dimensions().getFirst();
    }

    /**
     * @param name     a program name ({@code gbuffers_terrain}, {@code world0/composite})
     * @param profile  its draw profile ({@code null} for compute programs)
     * @return the program
     */
    public Program program(String name, String profile) {
        return dim().programs().stream()
            .filter(p -> p.name().equals(name) && java.util.Objects.equals(p.drawProfile(), profile))
            .findFirst().orElseThrow(() -> new AssertionError("no program " + name + " [" + profile + "]"));
    }

    /**
     * @param program a program of the fixture
     * @return its index in {@code programs}
     */
    public int indexOf(Program program) {
        return dim().programs().indexOf(program);
    }
}
