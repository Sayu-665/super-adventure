package dev.shaderbridge.pack;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import dev.shaderbridge.model.Diagnostic;
import dev.shaderbridge.model.Severity;
import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;

class PackSupportTest {
    @Test
    void vendorAndRendererMacros() {
        assertEquals("NVIDIA", GpuIdentity.vendor("NVIDIA"));
        assertEquals("NVIDIA", GpuIdentity.vendor("NVIDIA Corporation"));
        assertEquals("AMD", GpuIdentity.vendor("AMD"));
        assertEquals("ATI", GpuIdentity.vendor("ATI Technologies Inc."));
        assertEquals("INTEL", GpuIdentity.vendor("Intel"));
        assertEquals("XORG", GpuIdentity.vendor("X.Org"));
        assertEquals("OTHER", GpuIdentity.vendor("0x10005"));
        assertEquals("OTHER", GpuIdentity.vendor("Mesa"));

        assertEquals("GEFORCE", GpuIdentity.renderer("NVIDIA GeForce RTX 4070"));
        assertEquals("GEFORCE", GpuIdentity.renderer("NVIDIA RTX A4000"));
        assertEquals("QUADRO", GpuIdentity.renderer("Quadro RTX 4000/PCIe/SSE2"));
        assertEquals("RADEON", GpuIdentity.renderer("AMD Radeon RX 6800 (RADV NAVI21)"));
        assertEquals("GALLIUM", GpuIdentity.renderer("llvmpipe (LLVM 19.1.7, 256 bits)"));
        assertEquals("INTEL", GpuIdentity.renderer("Intel(R) Arc(tm) A770 Graphics (DG2)"));
        assertEquals("MESA", GpuIdentity.renderer("Mesa Intel(R) UHD Graphics 620 (KBL GT2)"));
        assertEquals("APPLE", GpuIdentity.renderer("Apple M2"));
        assertEquals("OTHER", GpuIdentity.renderer(null));

        assertEquals("MAC", GpuIdentity.os("OSX"));
        assertEquals("WINDOWS", GpuIdentity.os("WINDOWS"));
        assertEquals("UNKNOWN", GpuIdentity.os("SOLARIS"));
    }

    @Test
    void compileSettingsJson() {
        JsonObject all = JsonParser.parseString(new CompileSettings(null, true, Path.of("/tmp/cache"), 0).toJson()).getAsJsonObject();
        assertTrue(all.get("dimensions").isJsonNull());
        assertTrue(all.get("validate").getAsBoolean());
        assertEquals(Path.of("/tmp/cache").toAbsolutePath().toString(), all.get("cacheDir").getAsString());
        assertNull(all.get("threads"));

        JsonObject some = JsonParser.parseString(new CompileSettings(List.of("world0"), false, null, 4).toJson()).getAsJsonObject();
        assertEquals("world0", some.getAsJsonArray("dimensions").get(0).getAsString());
        assertTrue(some.get("cacheDir").isJsonNull());
        assertEquals(4, some.get("threads").getAsInt());
    }

    @Test
    void diagnosticSummary() {
        List<Diagnostic> diagnostics = List.of(
            new Diagnostic(Severity.ERROR, "a", "x", null, null, null),
            new Diagnostic(Severity.ERROR, "b", "y", null, null, null),
            new Diagnostic(Severity.WARNING, "c", "z", null, null, null),
            new Diagnostic(Severity.INFO, "d", "w", null, null, null));
        DiagnosticSummary summary = DiagnosticSummary.of(diagnostics);
        assertEquals(new DiagnosticSummary(2, 1, 1), summary);
        assertEquals("2 errors, 1 warning", summary.describe());
        assertEquals("no problems", DiagnosticSummary.of(List.of()).describe());
        assertEquals("no problems", DiagnosticSummary.of(diagnostics.subList(3, 4)).describe(), "notes are not problems");
        assertEquals("1 warning", DiagnosticSummary.of(diagnostics.subList(2, 3)).describe());
    }

    @Test
    void dumpFileNamesStayInsideTheDumpDirectory() {
        assertEquals("world0/gbuffers_terrain", GlslDumper.safeName("world0/gbuffers_terrain"));
        assertEquals("world-1/composite3_b", GlslDumper.safeName("world-1/composite3_b"));
        assertEquals("evil/name", GlslDumper.safeName("../../evil/./name"));
        assertEquals("a_b/c_d", GlslDumper.safeName("a:b/c d"));
        assertEquals("_", GlslDumper.safeName(""));
    }
}
