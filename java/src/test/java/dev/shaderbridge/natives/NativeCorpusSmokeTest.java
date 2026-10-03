package dev.shaderbridge.natives;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import com.google.gson.reflect.TypeToken;
import dev.shaderbridge.model.BlobInfo;
import dev.shaderbridge.model.BlobKind;
import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.BlockLayout;
import dev.shaderbridge.model.BlockMember;
import dev.shaderbridge.model.CompileEnvironment;
import dev.shaderbridge.model.CompiledPack;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.DeviceCaps;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.ModelJson;
import dev.shaderbridge.model.ModelValidation;
import dev.shaderbridge.model.OptionKind;
import dev.shaderbridge.model.OptionsModel;
import dev.shaderbridge.model.OutputTarget;
import dev.shaderbridge.model.PackOption;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.StageModule;
import dev.shaderbridge.pack.PackEntry;
import dev.shaderbridge.pack.PackKind;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.concurrent.TimeUnit;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.Timeout;
import org.junit.jupiter.api.io.TempDir;

/**
 * Drives the real native library through every JNI entry point against a real shader pack:
 * lists the packs of the small corpus, opens ComplementaryReimagined, reads its options,
 * compiles it with a {@link CompileEnvironment} built here, fetches the blob buffer and parses
 * the model with the {@code dev.shaderbridge.model} classes.
 *
 * <p>Needs the system properties {@value NativeLibrary#PATH_PROPERTY} (the built
 * {@code libsb_jni}, set by Gradle when {@code target/release} has it) and
 * {@code shaderbridge.corpus} (a directory containing {@code ComplementaryReimagined/shaders},
 * from {@code -Pshaderbridge.corpus=...} or {@code $SB_CORPUS_DIR}); skipped otherwise.
 */
@Timeout(value = 10, unit = TimeUnit.MINUTES)
class NativeCorpusSmokeTest {
    private static final String PACK = "ComplementaryReimagined";
    private static final int SPIRV_MAGIC = 0x07230203;

    @TempDir
    static Path gameDir;

    private static Path corpus;
    private static long session;

    /** {@code compileVariant}'s JSON. */
    record Variant(Program program, List<BlobInfo> blobs) {
    }

    @BeforeAll
    static void setUp() {
        String library = System.getProperty(NativeLibrary.PATH_PROPERTY);
        assumeTrue(library != null && Files.isRegularFile(Path.of(library)), "no native library (" + NativeLibrary.PATH_PROPERTY + ")");
        String corpusDir = System.getProperty("shaderbridge.corpus");
        assumeTrue(corpusDir != null && Files.isDirectory(Path.of(corpusDir, PACK, "shaders")), "no corpus with " + PACK + " (shaderbridge.corpus)");
        corpus = Path.of(corpusDir).toAbsolutePath();
        assertInstanceOf(NativeLibrary.Status.Loaded.class, NativeLibrary.load(gameDir), () -> String.valueOf(NativeLibrary.status()));
        session = ShaderBridgeNative.openPack(corpus.resolve(PACK).toString());
        assertNotEquals(0L, session, ShaderBridgeNative::lastError);
    }

    @AfterAll
    static void tearDown() {
        if (session != 0) {
            ShaderBridgeNative.closePack(session);
            assertNull(ShaderBridgeNative.lastError());
            // Closing twice is harmless: an error, not a crash.
            ShaderBridgeNative.closePack(session);
            assertNotNull(ShaderBridgeNative.lastError());
            session = 0;
        }
    }

    private static CompileEnvironment environment() {
        DeviceCaps device = new DeviceCaps(true, true, true, true, false, 128, 8, true, null);
        return new CompileEnvironment("26.3", "LINUX", "NVIDIA", "GEFORCE", true, Map.of("SHADERBRIDGE_TEST", "1"),
            List.of(OutputTarget.VULKAN, OutputTarget.RENDERPEARL), DepthMode.REVERSED_ZERO_TO_ONE, device);
    }

    @Test
    void listsTheCorpus() throws Exception {
        String json = ShaderBridgeNative.listPacks(corpus.toString());
        assertNotNull(json, ShaderBridgeNative::lastError);
        List<PackEntry> packs = ModelJson.parse(json, new TypeToken<List<PackEntry>>() { });
        PackEntry pack = packs.stream().filter(p -> p.name().equals(PACK)).findFirst().orElseThrow();
        assertTrue(pack.valid(), pack::error);
        assertEquals(PackKind.DIR, pack.kind());
        assertEquals(corpus.resolve(PACK), pack.file());
        assertTrue(packs.size() >= 5, "packs: " + packs);
        // The tutorial collection has no shaders/ directory of its own.
        packs.stream().filter(p -> p.name().equals("MinecraftShaderProgramming"))
            .forEach(p -> assertFalse(p.valid(), "MinecraftShaderProgramming is a collection, not a pack"));

        assertNull(ShaderBridgeNative.listPacks(corpus.resolve("does-not-exist").toString()));
        assertNotNull(ShaderBridgeNative.lastError());
    }

    @Test
    void compilesComplementaryReimagined() throws Exception {
        assertTrue(ShaderBridgeNative.version().matches("\\d+\\.\\d+\\.\\d+.*"), ShaderBridgeNative.version());

        // Options and settings normalization.
        String optionsJson = ShaderBridgeNative.getOptions(session, "en_us");
        assertNotNull(optionsJson, ShaderBridgeNative::lastError);
        OptionsModel options = ModelJson.parse(optionsJson, OptionsModel.class);
        assertTrue(options.options().size() > 50, "options: " + options.options().size());
        assertFalse(options.screens().isEmpty());
        assertFalse(options.lang().isEmpty());
        PackOption toggle = options.options().stream()
            .filter(o -> o.kind() == OptionKind.BOOLEAN_DEFINE && o.allowed().isEmpty())
            .findFirst().orElseThrow();
        String flipped = "true".equals(toggle.defaultValue()) ? "false" : "true";
        String normalized = ShaderBridgeNative.normalizeOptionValues(session, toggle.name() + "=" + flipped + "\nNOT_AN_OPTION=1\n");
        assertNotNull(normalized, ShaderBridgeNative::lastError);
        assertEquals(toggle.name() + "=" + flipped, normalized.strip());

        // Compile every dimension.
        String settings = "{\"dimensions\": null, \"validate\": false, \"cacheDir\": null}";
        String json = ShaderBridgeNative.compile(session, ModelJson.toJson(environment()), "", settings);
        assertNotNull(json, ShaderBridgeNative::lastError);
        CompiledPack model = ModelJson.parse(json, CompiledPack.class);
        assertEquals(CompiledPack.FORMAT_VERSION, model.formatVersion());
        assertEquals(PACK, model.info().name());
        assertEquals(environment(), model.info().environment());
        assertEquals(List.of(), ModelValidation.problems(model));

        long size = ShaderBridgeNative.blobSize(session);
        assertTrue(size > 0 && size <= Integer.MAX_VALUE, "blob size " + size);
        ByteBuffer tooSmall = ByteBuffer.allocateDirect((int) size - 1);
        assertFalse(ShaderBridgeNative.blobData(session, tooSmall));
        assertNotNull(ShaderBridgeNative.lastError());
        assertFalse(ShaderBridgeNative.blobData(session, ByteBuffer.allocate((int) size)), "heap buffers are rejected");
        ByteBuffer buffer = ByteBuffer.allocateDirect((int) size).order(ByteOrder.LITTLE_ENDIAN);
        assertTrue(ShaderBridgeNative.blobData(session, buffer), ShaderBridgeNative::lastError);
        assertEquals(0, buffer.position());
        Blobs blobs = Blobs.of(buffer, model.blobs());

        int programs = 0;
        for (DimensionPipeline dimension : model.dimensions()) {
            for (Program program : dimension.programs()) {
                programs++;
                assertFalse(program.stages().isEmpty(), program.name());
                for (StageModule stage : program.stages()) {
                    assertNotNull(stage.spirv(), program.name() + " " + stage.stage() + " has no SPIR-V");
                    ByteBuffer words = blobs.spirv(stage.spirv());
                    assertTrue(words.remaining() >= 20, program.name());
                    assertEquals(SPIRV_MAGIC, words.getInt(0), program.name() + " SPIR-V magic");
                    assertTrue(blobs.glsl(stage.glslVulkan()).startsWith("#version"), program.name());
                }
            }
        }
        assertTrue(programs > 50, "programs: " + programs);
        assertTrue(model.blobs().stream().anyMatch(b -> b.kind() == BlobKind.GLSL));

        // Custom uniforms: Complementary defines framemod2 = frameCounter % 2.
        DimensionPipeline world0 = model.dimension("world0").orElseThrow();
        BlockLayout frame = world0.uniforms().frame();
        BlockMember frameCounter = frame.member("frameCounter").orElseThrow();
        BlockMember framemod2 = frame.member("framemod2").orElseThrow();
        long evaluator = ShaderBridgeNative.createUniformEvaluator(session, "world0");
        assertNotEquals(0L, evaluator, ShaderBridgeNative::lastError);
        try {
            ByteBuffer block = ByteBuffer.allocateDirect(Math.max(16, frame.size())).order(ByteOrder.LITTLE_ENDIAN);
            block.putInt(frameCounter.offset(), 5);
            assertTrue(ShaderBridgeNative.evaluateUniforms(evaluator, block, 1f / 60f), ShaderBridgeNative::lastError);
            assertEquals(1.0f, block.getFloat(framemod2.offset()));
            assertFalse(ShaderBridgeNative.evaluateUniforms(evaluator, ByteBuffer.allocateDirect(4), 0f));
        } finally {
            ShaderBridgeNative.destroyUniformEvaluator(evaluator);
        }
        assertFalse(ShaderBridgeNative.evaluateUniforms(evaluator, ByteBuffer.allocateDirect(frame.size()), 0f));
        assertEquals(0L, ShaderBridgeNative.createUniformEvaluator(session, "no-such-folder"));

        // An extra (geometry program, draw profile) variant.
        String variantJson = ShaderBridgeNative.compileVariant(session, "world0", "gbuffers_terrain", "vanilla_terrain");
        assertNotNull(variantJson, ShaderBridgeNative::lastError);
        Variant variant = ModelJson.parse(variantJson, Variant.class);
        long variantSize = ShaderBridgeNative.variantBlobSize(session);
        ByteBuffer variantBuffer = ByteBuffer.allocateDirect((int) variantSize).order(ByteOrder.LITTLE_ENDIAN);
        assertTrue(ShaderBridgeNative.variantBlobData(session, variantBuffer), ShaderBridgeNative::lastError);
        Blobs variantBlobs = Blobs.of(variantBuffer, variant.blobs());
        for (StageModule stage : variant.program().stages()) {
            assertEquals(SPIRV_MAGIC, variantBlobs.spirv(stage.spirv()).getInt(0));
        }
        assertNull(ShaderBridgeNative.compileVariant(session, "world0", "not_a_program", "vanilla_terrain"));
        assertNotNull(ShaderBridgeNative.lastError());

        // Profiles: an invalid definition is reported, not thrown.
        assertNotNull(ShaderBridgeNative.registerProfile("name = "));
    }

    @Test
    void reportsBadHandlesAndArguments() {
        assertNull(ShaderBridgeNative.getOptions(0, "en_us"));
        assertNotNull(ShaderBridgeNative.lastError());
        assertEquals(0L, ShaderBridgeNative.blobSize(-5));
        assertNull(ShaderBridgeNative.compile(session, "{not json", "", null));
        assertTrue(ShaderBridgeNative.lastError().contains("envJson"), ShaderBridgeNative::lastError);
        assertEquals(0L, ShaderBridgeNative.openPack(corpus.resolve("does-not-exist").toString()));
        assertNotNull(ShaderBridgeNative.lastError());
        assertNull(ShaderBridgeNative.getOptions(session, "../../escape"));
        // A successful call clears the error.
        assertNotNull(ShaderBridgeNative.version());
        assertNull(ShaderBridgeNative.lastError());
    }
}
