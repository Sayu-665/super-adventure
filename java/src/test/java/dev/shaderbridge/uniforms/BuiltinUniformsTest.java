package dev.shaderbridge.uniforms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import dev.shaderbridge.model.GlslType;
import dev.shaderbridge.model.ScalarKind;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.joml.Vector3f;
import org.junit.jupiter.api.Test;

class BuiltinUniformsTest {
    /** Name, type and frequency of every builtin in crates/sb-uniforms/src/registry.rs. */
    private record RegistryEntry(String name, String type, boolean perDraw) {
    }

    private static List<RegistryEntry> rustRegistry() throws IOException {
        String configured = System.getProperty("shaderbridge.repoRoot");
        Path file = (configured != null ? Path.of(configured) : Path.of("..")).resolve("crates/sb-uniforms/src/registry.rs");
        assumeTrue(Files.isRegularFile(file), "sb-uniforms sources not available");
        String source = Files.readString(file);
        String table = source.substring(source.indexOf("static BUILTINS"), source.indexOf("static BY_NAME"));
        Matcher m = Pattern.compile("(frame|draw)\\(\\s*\"(\\w+)\",\\s*T::(\\w+),").matcher(table);
        List<RegistryEntry> out = new ArrayList<>();
        while (m.find()) {
            out.add(new RegistryEntry(m.group(2), m.group(3), m.group(1).equals("draw")));
        }
        return out;
    }

    private static String rustTypeName(GlslType type) {
        return type.glslName().toUpperCase(Locale.ROOT);
    }

    @Test
    void everyRegistryBuiltinHasAProviderWithTheSameTypeAndBlock() throws IOException {
        List<RegistryEntry> registry = rustRegistry();
        assertTrue(registry.size() > 150, "parsed " + registry.size());
        Map<String, BuiltinUniform> java = new LinkedHashMap<>();
        BuiltinUniforms.all().forEach(u -> java.put(u.name(), u));
        assertEquals(registry.stream().map(RegistryEntry::name).sorted().toList(), java.keySet().stream().sorted().toList(), "same names");
        for (RegistryEntry entry : registry) {
            BuiltinUniform uniform = java.get(entry.name());
            assertEquals(entry.type(), rustTypeName(uniform.type()), entry.name());
            assertEquals(entry.perDraw(), uniform.perDraw(), entry.name());
        }
    }

    @Test
    void everyProviderWritesItsRegistryType() {
        FrameState frame = new FrameState();
        frame.update();
        DrawState draw = new DrawState();
        draw.reset(frame);
        draw.update();
        ByteBuffer block = ByteBuffer.allocateDirect(256).order(ByteOrder.LITTLE_ENDIAN);
        Std140Writer writer = new Std140Writer(block);
        UniformWriter out = new UniformWriter();
        for (BuiltinUniform uniform : BuiltinUniforms.all()) {
            uniform.provider().write(frame, draw, out.bind(writer, 0, uniform.type()));
            uniform.provider().write(frame, draw, out.bind(writer, 128, GlslType.FLOAT.withArray(2)));
        }
    }

    private static ByteBuffer write(String name, FrameState frame, DrawState draw, GlslType declared) {
        ByteBuffer block = ByteBuffer.allocateDirect(64).order(ByteOrder.LITTLE_ENDIAN);
        BuiltinUniforms.get(name).orElseThrow().provider().write(frame, draw, new UniformWriter().bind(new Std140Writer(block), 0, declared));
        return block;
    }

    @Test
    void representativeValues() {
        FrameState frame = new FrameState();
        frame.viewWidth = 1920;
        frame.viewHeight = 1080;
        frame.cameraPosition.set(10.25, 70.5, -3.75);
        frame.sunAngleAttribute = 0;
        frame.hideGui = true;
        frame.fogStart = 10;
        frame.fogEnd = 30;
        frame.update();
        DrawState draw = new DrawState();
        draw.reset(frame);
        draw.entityId = 42;

        assertEquals(1920f / 1080f, write("aspectRatio", frame, draw, GlslType.FLOAT).getFloat(0), 1e-6f);
        ByteBuffer cameraInt = write("cameraPositionInt", frame, draw, GlslType.vector(ScalarKind.INT, 3));
        assertEquals(10, cameraInt.getInt(0));
        assertEquals(-4, cameraInt.getInt(8));
        assertEquals(0.25f, write("cameraPositionFract", frame, draw, GlslType.VEC3).getFloat(8), 1e-6f);
        ByteBuffer sun = write("sunPosition", frame, draw, GlslType.VEC3);
        assertEquals(100, new Vector3f(sun.getFloat(0), sun.getFloat(4), sun.getFloat(8)).length(), 1e-3f);
        assertEquals(1, write("hideGUI", frame, draw, GlslType.INT).getInt(0), "int declaration of a bool builtin");
        assertEquals(42, write("entityId", frame, draw, GlslType.INT).getInt(0));
        assertEquals(0.05f, write("fogScale", frame, draw, GlslType.FLOAT).getFloat(0), 1e-6f);
        assertEquals(9729, write("fogMode", frame, draw, GlslType.INT).getInt(0));
        assertEquals((float) Math.PI, write("pi", frame, draw, GlslType.FLOAT).getFloat(0));
        assertEquals(20f, write("maxPlayerHunger", frame, draw, GlslType.FLOAT).getFloat(0));
        ByteBuffer projection = write("gbufferProjection", frame, draw, GlslType.MAT4);
        assertEquals(frame.gbufferProjection().m00(), projection.getFloat(0));
    }
}
