package dev.shaderbridge.pack;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import dev.shaderbridge.config.PackOptionValues;
import dev.shaderbridge.model.BlobId;
import dev.shaderbridge.model.CompileEnvironment;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.DeviceCaps;
import dev.shaderbridge.model.OptionsModel;
import dev.shaderbridge.model.OutputTarget;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.StageModule;
import dev.shaderbridge.natives.NativeLibrary;
import dev.shaderbridge.natives.NativePlatform;
import java.io.IOException;
import java.nio.ByteOrder;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

/**
 * Loads the real native library (when it was built) and compiles a minimal pack through JNI.
 * Skipped when the library is not available, e.g. with {@code -PskipNative}.
 */
class NativeSmokeTest {
    @TempDir
    static Path gameDir;

    @BeforeAll
    static void loadLibrary() {
        if (System.getProperty(NativeLibrary.PATH_PROPERTY) == null) {
            String stagingRoot = System.getProperty("shaderbridge.nativeDir");
            NativePlatform platform = NativePlatform.current().orElse(null);
            assumeTrue(stagingRoot != null && platform != null, "no native staging directory");
            Path library = Path.of(stagingRoot).resolve("natives").resolve(platform.toString()).resolve(platform.libraryFileName());
            assumeTrue(Files.isRegularFile(library), "the native library was not built: " + library);
            System.setProperty(NativeLibrary.PATH_PROPERTY, library.toString());
        }
        assertInstanceOf(NativeLibrary.Status.Loaded.class, NativeLibrary.load(gameDir), () -> String.valueOf(NativeLibrary.status()));
    }

    private static Path writePack() throws IOException {
        Path shaders = Files.createDirectories(gameDir.resolve("shaderpacks").resolve("Smoke").resolve("shaders"));
        Files.writeString(shaders.resolve("final.vsh"), """
            #version 120
            varying vec2 texcoord;
            void main() {
                gl_Position = ftransform();
                texcoord = gl_MultiTexCoord0.xy;
            }
            """);
        Files.writeString(shaders.resolve("final.fsh"), """
            #version 120
            #define BRIGHTNESS 1.0 // [0.5 1.0 1.5]
            uniform sampler2D colortex0;
            uniform float frameTimeCounter;
            varying vec2 texcoord;
            void main() {
                gl_FragColor = texture2D(colortex0, texcoord) * BRIGHTNESS + frameTimeCounter * 0.0;
            }
            """);
        return shaders.getParent();
    }

    private static CompileEnvironment environment() {
        DeviceCaps caps = new DeviceCaps(false, false, false, false, false, 128, 8, false, 32);
        return new CompileEnvironment("26.3", "LINUX", "OTHER", "OTHER", false, Map.of(), List.of(OutputTarget.VULKAN, OutputTarget.RENDERPEARL),
            DepthMode.REVERSED_ZERO_TO_ONE, caps);
    }

    @Test
    void listsOpensAndCompilesAPack() throws Exception {
        Path pack = writePack();
        List<PackEntry> packs = new PackRepository(pack.getParent()).scan();
        assertEquals(List.of("Smoke"), packs.stream().map(PackEntry::name).toList());
        assertTrue(packs.get(0).valid());

        try (PackSession session = PackSession.open(pack)) {
            OptionsModel options = session.options("en_us");
            assertTrue(options.option("BRIGHTNESS").isPresent());
            String normalized = session.normalizeOptionValues(PackOptionValues.empty().with("BRIGHTNESS", "1.5").with("UNKNOWN", "1").toSettingsText());
            assertEquals("1.5", PackOptionValues.parse(normalized).get("BRIGHTNESS").orElseThrow());
            assertFalse(PackOptionValues.parse(normalized).get("UNKNOWN").isPresent());

            PackSession.CompileResult result = session.compile(environment(), "BRIGHTNESS=1.5\n", new CompileSettings(null, false, null, 0));
            assertFalse(result.model().dimensions().isEmpty());
            Program finalProgram = result.model().dimensions().stream()
                .flatMap(d -> d.programs().stream())
                .filter(p -> p.name().endsWith("final"))
                .findFirst().orElseThrow();
            StageModule fragment = finalProgram.stages().stream().filter(s -> s.spirv() != null).findFirst().orElseThrow();
            BlobId spirv = fragment.spirv();
            assertEquals(0x07230203, result.blobs().spirv(spirv).order(ByteOrder.LITTLE_ENDIAN).getInt(0), "SPIR-V magic");
            assertTrue(result.blobs().glsl(fragment.glslVulkan()).contains("#version"));
        }
    }
}
