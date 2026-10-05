package dev.shaderbridge.compat.sodium;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.util.List;
import java.util.Optional;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.MethodNode;

/**
 * {@link SodiumTargets} and {@link SodiumIntegration} against the class files on the test class
 * path: the Sodium jar the build compiles against, the Minecraft 26.3 jar and ShaderBridge's own
 * classes. Every member the Sodium integration hooks or calls must exist; a missing one keeps the
 * integration off with a message naming it.
 */
class SodiumTargetsTest {
    /** Reads class files from the test class path (null when absent). */
    static ClassNode readClass(String internalName) {
        try (InputStream in = SodiumTargetsTest.class.getResourceAsStream("/" + internalName + ".class")) {
            if (in == null) {
                return null;
            }
            ClassNode node = new ClassNode();
            new ClassReader(in.readAllBytes()).accept(node, 0);
            return node;
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    @AfterEach
    void forgetDecision() {
        SodiumIntegration.reset();
    }

    @Test
    void everyTargetExistsInTheSodiumAndMinecraftJars() {
        assertEquals(List.of(), SodiumTargets.problems(SodiumTargetsTest::readClass));
    }

    @Test
    void theCheckedSodiumIsSodium09ForMinecraft263() throws IOException {
        assertNotNull(readClass(SodiumTargets.WORLD_RENDERER), "the Sodium jar must be on the test class path");
        String sodiumVersion = null;
        for (java.net.URL url : java.util.Collections.list(SodiumTargetsTest.class.getClassLoader().getResources("fabric.mod.json"))) {
            try (InputStream in = url.openStream()) {
                com.google.gson.JsonObject mod = com.google.gson.JsonParser.parseString(new String(in.readAllBytes(), java.nio.charset.StandardCharsets.UTF_8))
                    .getAsJsonObject();
                if (mod.has("id") && mod.get("id").getAsString().equals("sodium")) {
                    sodiumVersion = mod.get("version").getAsString();
                }
            }
        }
        assertNotNull(sodiumVersion, "Sodium's fabric.mod.json");
        assertTrue(sodiumVersion.startsWith("0.9.") && sodiumVersion.endsWith("+mc26.3"), sodiumVersion);
    }

    @Test
    void aMissingMethodIsNamed() {
        SodiumTargets.ClassSource withoutInit = name -> {
            ClassNode node = readClass(name);
            if (node != null && name.equals(SodiumTargets.WORLD_RENDERER)) {
                node.methods.removeIf(m -> m.name.equals("initRenderer"));
            }
            return node;
        };
        List<String> problems = SodiumTargets.problems(withoutInit);
        assertEquals(List.of("SodiumWorldRenderer.initRenderer()V is missing"), problems);
    }

    @Test
    void aMissingCallSiteIsNamed() {
        SodiumTargets.ClassSource withoutFluidCall = name -> {
            ClassNode node = readClass(name);
            if (node != null && name.equals(SodiumTargets.MESHING_TASK)) {
                for (MethodNode method : node.methods) {
                    method.instructions.clear();
                }
            }
            return node;
        };
        List<String> problems = SodiumTargets.problems(withoutFluidCall);
        assertEquals(2, problems.size(), problems.toString());
        assertTrue(problems.get(0).startsWith("the call to BlockRenderer.renderModel("), problems.get(0));
        assertTrue(problems.get(1).startsWith("the call to FluidRenderer.render("), problems.get(1));
    }

    @Test
    void aMissingClassIsNamedOncePerMember() {
        SodiumTargets.ClassSource withoutCollector = name -> name.equals(SodiumTargets.GEOMETRY_COLLECTOR) ? null : readClass(name);
        List<String> problems = SodiumTargets.problems(withoutCollector);
        assertEquals(1, problems.size(), problems.toString());
        assertTrue(problems.getFirst().startsWith("class net.caffeinemc.mods.sodium.client.render.chunk.translucent_sorting.TranslucentGeometryCollector is missing"),
            problems.getFirst());
    }

    @Test
    void aStaticMemberThatIsNoLongerStaticCounts() {
        SodiumTargets.ClassSource instanceGetCurrent = name -> {
            ClassNode node = readClass(name);
            if (node != null && name.equals(SodiumTargets.CHUNK_MESH_FORMATS)) {
                node.methods.stream().filter(m -> m.name.equals("getCurrent")).forEach(m -> m.access &= ~org.objectweb.asm.Opcodes.ACC_STATIC);
            }
            return node;
        };
        assertEquals(List.of("ChunkMeshFormats.getCurrent()Lnet/caffeinemc/mods/sodium/client/render/chunk/vertex/format/ChunkVertexType; is missing"),
            SodiumTargets.problems(instanceGetCurrent));
    }

    @Test
    void withoutSodiumNothingIsApplied() {
        assertFalse(SodiumIntegration.decide(Optional.empty(), SodiumTargetsTest::readClass));
        assertInstanceOf(SodiumIntegration.Status.Absent.class, SodiumIntegration.status(Optional.empty()));
        assertFalse(SodiumIntegration.active());
    }

    @Test
    void withAMatchingSodiumEverythingIsApplied() {
        assertTrue(SodiumIntegration.decide(Optional.of("0.9.3-alpha.1+mc26.3"), SodiumTargetsTest::readClass));
        assertEquals(new SodiumIntegration.Status.Active("0.9.3-alpha.1+mc26.3"), SodiumIntegration.status(Optional.of("0.9.3-alpha.1+mc26.3")));
        assertTrue(SodiumIntegration.active());
    }

    @Test
    void withAnotherSodiumNothingIsApplied() {
        assertFalse(SodiumIntegration.decide(Optional.of("0.10.0+mc26.4"), name -> name.startsWith("net/caffeinemc/") ? null : readClass(name)));
        SodiumIntegration.Status status = SodiumIntegration.status(Optional.of("0.10.0+mc26.4"));
        SodiumIntegration.Status.Unavailable unavailable = assertInstanceOf(SodiumIntegration.Status.Unavailable.class, status);
        assertFalse(unavailable.problems().isEmpty());
        assertFalse(SodiumIntegration.active());
    }

    @Test
    void aFailingCheckKeepsTheIntegrationOff() {
        assertFalse(SodiumIntegration.decide(Optional.of("0.9.2+mc26.3"), name -> {
            throw new IllegalStateException("broken class file");
        }));
        SodiumIntegration.Status.Unavailable status = assertInstanceOf(SodiumIntegration.Status.Unavailable.class,
            SodiumIntegration.status(Optional.of("0.9.2+mc26.3")));
        assertTrue(status.problems().getFirst().contains("broken class file"), status.problems().toString());
    }

    @Test
    void sodiumWithoutADecisionIsReportedAsNotLoaded() {
        assertInstanceOf(SodiumIntegration.Status.NotLoaded.class, SodiumIntegration.status(Optional.of("0.9.2+mc26.3")));
    }
}
