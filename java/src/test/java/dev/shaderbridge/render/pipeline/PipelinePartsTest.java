package dev.shaderbridge.render.pipeline;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import com.mojang.blaze3d.vertex.DefaultVertexFormat;
import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.pipeline.BlendFactor;
import com.mojang.renderpearl.api.pipeline.BlendFunction;
import com.mojang.renderpearl.api.pipeline.CompareOp;
import com.mojang.renderpearl.api.pipeline.DepthStencilState;
import com.mojang.renderpearl.api.pipeline.ShaderType;
import dev.shaderbridge.model.BlendMode;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.TextureFormat;
import dev.shaderbridge.render.pipeline.SpirvReflection.InterfaceVariable;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import net.minecraft.resources.Identifier;
import org.junit.jupiter.api.Test;

/** The small pure pieces of pipeline construction. */
class PipelinePartsTest {
    @Test
    void pipelineLocationsAreValidIdentifiers() {
        PipelineKey key = new PipelineKey("world-1", "world-1/gbuffers_Terrain", "vanilla_terrain_basic", "minecraft/pipeline/solid_terrain", "gbuffers");
        Identifier location = key.location("0123456789abcdef0123");
        assertEquals("shaderbridge", location.getNamespace());
        assertEquals("0123456789abcdef/world-1/world-1/gbuffers_terrain/vanilla_terrain_basic/minecraft/pipeline/solid_terrain/gbuffers",
            location.getPath());
        assertEquals("root", new PipelineKey("", "final", "fullscreen", "fullscreen", "final").location("ab").getPath().split("/")[1]);
        assertEquals("a_b_", PipelineKey.sanitize("A B?"));
        assertEquals("_", PipelineKey.sanitize("//"));
    }

    @Test
    void depthStatesFollowTheDepthMode() {
        assertEquals(new DepthStencilState(CompareOp.GREATER_THAN_OR_EQUAL, true), DepthStates.standard(DepthMode.REVERSED_ZERO_TO_ONE));
        assertEquals(new DepthStencilState(CompareOp.LESS_THAN_OR_EQUAL, true), DepthStates.standard(DepthMode.FORWARD_ZERO_TO_ONE));
        DepthStencilState biased = new DepthStencilState(CompareOp.GREATER_THAN_OR_EQUAL, true, 1.0f, 10.0f);
        assertSame(biased, DepthStates.forPack(DepthMode.REVERSED_ZERO_TO_ONE, biased));
        assertEquals(new DepthStencilState(CompareOp.LESS_THAN_OR_EQUAL, true, -1.0f, -10.0f), DepthStates.forPack(DepthMode.FORWARD_ZERO_TO_ONE, biased));
        assertEquals(CompareOp.EQUAL, DepthStates.forPack(DepthMode.FORWARD_ZERO_TO_ONE, new DepthStencilState(CompareOp.EQUAL, false)).depthTest());
        assertNull(DepthStates.forPack(DepthMode.FORWARD_ZERO_TO_ONE, null));
        assertEquals(0.0, DepthStates.clearValue(DepthMode.REVERSED_ZERO_TO_ONE));
        assertEquals(1.0, DepthStates.clearValue(DepthMode.GL_NEG_ONE_TO_ONE));
    }

    @Test
    void blendModesMapFactorByFactor() {
        BlendMode mode = new BlendMode(dev.shaderbridge.model.BlendFactor.SRC_ALPHA, dev.shaderbridge.model.BlendFactor.ONE_MINUS_SRC_ALPHA,
            dev.shaderbridge.model.BlendFactor.ONE, dev.shaderbridge.model.BlendFactor.ONE_MINUS_SRC_ALPHA);
        assertEquals(BlendFunction.TRANSLUCENT, BlendFunctions.of(mode));
        for (dev.shaderbridge.model.BlendFactor f : dev.shaderbridge.model.BlendFactor.values()) {
            assertEquals(f.name(), BlendFunctions.factor(f).name());
        }
        assertEquals(BlendFactor.SRC_ALPHA_SATURATE, BlendFunctions.factor(dev.shaderbridge.model.BlendFactor.SRC_ALPHA_SATURATE));
    }

    @Test
    void textureFormatsAreRenderable() {
        for (TextureFormat format : TextureFormat.values()) {
            GpuFormat gpu = TextureFormats.renderable(format);
            assertTrue(gpu.hasColorAspect(), format.name());
            assertTrue(gpu.componentCount() != 3 || gpu.componentType() == GpuFormat.ComponentType.OPAQUE_32, format + " is not widened");
        }
        assertEquals(GpuFormat.RGBA16_FLOAT, TextureFormats.renderable(TextureFormat.RGB9_E5));
        assertEquals(ScalarClass.UINT, TextureFormats.numericClass(GpuFormat.RGB10A2_UINT));
        assertEquals(ScalarClass.INT, TextureFormats.numericClass(GpuFormat.RG16_SINT));
        assertEquals(ScalarClass.FLOAT, TextureFormats.numericClass(GpuFormat.RG11B10_FLOAT));
        assertFalse(TextureFormats.blendable(GpuFormat.R32_UINT));
    }

    /** VkFormat numbers of the renderable formats sb-core uses, as Mojang formats. */
    private static final Map<Integer, GpuFormat> VK_FORMATS = Map.ofEntries(Map.entry(9, GpuFormat.R8_UNORM), Map.entry(10, GpuFormat.R8_SNORM),
        Map.entry(13, GpuFormat.R8_UINT), Map.entry(14, GpuFormat.R8_SINT), Map.entry(16, GpuFormat.RG8_UNORM), Map.entry(17, GpuFormat.RG8_SNORM),
        Map.entry(20, GpuFormat.RG8_UINT), Map.entry(21, GpuFormat.RG8_SINT), Map.entry(37, GpuFormat.RGBA8_UNORM), Map.entry(38, GpuFormat.RGBA8_SNORM),
        Map.entry(41, GpuFormat.RGBA8_UINT), Map.entry(42, GpuFormat.RGBA8_SINT), Map.entry(64, GpuFormat.RGB10A2_UNORM),
        Map.entry(68, GpuFormat.RGB10A2_UINT), Map.entry(70, GpuFormat.R16_UNORM), Map.entry(71, GpuFormat.R16_SNORM),
        Map.entry(74, GpuFormat.R16_UINT), Map.entry(75, GpuFormat.R16_SINT), Map.entry(76, GpuFormat.R16_FLOAT), Map.entry(77, GpuFormat.RG16_UNORM),
        Map.entry(78, GpuFormat.RG16_SNORM), Map.entry(81, GpuFormat.RG16_UINT), Map.entry(82, GpuFormat.RG16_SINT), Map.entry(83, GpuFormat.RG16_FLOAT),
        Map.entry(91, GpuFormat.RGBA16_UNORM), Map.entry(92, GpuFormat.RGBA16_SNORM), Map.entry(95, GpuFormat.RGBA16_UINT),
        Map.entry(96, GpuFormat.RGBA16_SINT), Map.entry(97, GpuFormat.RGBA16_FLOAT), Map.entry(98, GpuFormat.R32_UINT), Map.entry(99, GpuFormat.R32_SINT),
        Map.entry(100, GpuFormat.R32_FLOAT), Map.entry(101, GpuFormat.RG32_UINT), Map.entry(102, GpuFormat.RG32_SINT),
        Map.entry(103, GpuFormat.RG32_FLOAT), Map.entry(107, GpuFormat.RGBA32_UINT), Map.entry(108, GpuFormat.RGBA32_SINT),
        Map.entry(109, GpuFormat.RGBA32_FLOAT), Map.entry(122, GpuFormat.RG11B10_FLOAT));

    @Test
    void renderableFormatsMatchSbCore() throws IOException {
        String configured = System.getProperty("shaderbridge.repoRoot");
        Path file = (configured != null ? Path.of(configured) : Path.of("..")).resolve("crates/sb-core/src/format.rs");
        assumeTrue(Files.isRegularFile(file), "sb-core sources not available");
        Matcher m = Pattern.compile("(?m)^    (\\w+) = \"\\w+\", \\d+, (\\d+), ").matcher(Files.readString(file));
        int count = 0;
        while (m.find()) {
            TextureFormat format = TextureFormat.valueOf(m.group(1));
            assertEquals(VK_FORMATS.get(Integer.parseInt(m.group(2))), TextureFormats.renderable(format), format.name());
            count++;
        }
        assertEquals(TextureFormat.values().length, count);
    }

    private static InterfaceVariable input(String name, int location, ScalarClass scalar, int size) {
        return new InterfaceVariable(name, location, scalar, size, 1, false, false, false);
    }

    @Test
    void vertexInputsNeedAMatchingElement() {
        List<com.mojang.renderpearl.api.vertex.VertexFormat> entity = List.of(DefaultVertexFormat.ENTITY);
        assertEquals(List.of(), VertexInputCheck.check(entity, List.of(input("Position", 0, ScalarClass.FLOAT, 3), input("UV2", 4, ScalarClass.INT, 2))));
        assertTrue(VertexInputCheck.check(entity, List.of(input("UV2", 4, ScalarClass.FLOAT, 2))).getFirst().contains("is FLOAT"));
        assertTrue(VertexInputCheck.check(entity, List.of(input("Position", 0, ScalarClass.FLOAT, 4))).getFirst().contains("needs 4 components"));
        assertTrue(VertexInputCheck.check(entity, List.of(input("sb_Normal", 6, ScalarClass.FLOAT, 3))).getFirst().contains("no matching"));
        assertEquals(ScalarClass.OTHER, VertexInputCheck.attributeClass(GpuFormat.RGB10A2_UNORM));
    }

    @Test
    void stageInterfacesAreCheckedLikeMojangsBuilder() {
        SpirvReflection vertex = new SpirvReflection(SpirvReflection.Stage.VERTEX, List.of(), List.of(), List.of(input("a", 0, ScalarClass.FLOAT, 4),
            new InterfaceVariable("b", 1, ScalarClass.INT, 1, 1, true, false, false)), 0);
        SpirvReflection fragment = new SpirvReflection(SpirvReflection.Stage.FRAGMENT, List.of(), List.of(input("a", 0, ScalarClass.FLOAT, 4),
            new InterfaceVariable("b", 1, ScalarClass.INT, 1, 1, true, false, false)), List.of(), 0);
        ProgramInterface ok = ProgramInterface.of(Map.of(dev.shaderbridge.model.ShaderStage.VERTEX, vertex, dev.shaderbridge.model.ShaderStage.FRAGMENT,
            fragment));
        assertEquals(List.of(), Eligibility.stageInterfaceProblems(ok));
        SpirvReflection smooth = new SpirvReflection(SpirvReflection.Stage.FRAGMENT, List.of(), List.of(new InterfaceVariable("b", 1, ScalarClass.INT,
            1, 1, false, false, false), input("c", 2, ScalarClass.FLOAT, 1)), List.of(), 0);
        List<String> problems = Eligibility.stageInterfaceProblems(ProgramInterface.of(Map.of(dev.shaderbridge.model.ShaderStage.VERTEX, vertex,
            dev.shaderbridge.model.ShaderStage.FRAGMENT, smooth)));
        assertEquals(2, problems.size(), problems.toString());
        SpirvReflection noLocation = new SpirvReflection(SpirvReflection.Stage.FRAGMENT, List.of(), List.of(), List.of(input("out", -1,
            ScalarClass.FLOAT, 4)), 0);
        assertEquals(List.of("fragment output out has no location"), Eligibility.locationProblems(ProgramInterface.of(Map.of(
            dev.shaderbridge.model.ShaderStage.FRAGMENT, noLocation))));
    }

    @Test
    void modulesAreServedToTheCompilerHookOnce() {
        SpirvModules modules = new SpirvModules();
        ByteBuffer spirv = ByteBuffer.allocate(8).putInt(0, SpirvReflector.MAGIC);
        Identifier id = modules.register(spirv);
        assertTrue(SpirvModules.isModuleId(id.toString()));
        assertFalse(SpirvModules.isModuleId("minecraft:core/terrain"));
        PackShaderSource source = new PackShaderSource(modules);
        assertTrue(source.getShader(id, ShaderType.VERTEX).contains("#error"));
        assertNull(source.getShader(Identifier.parse("minecraft:core/terrain"), ShaderType.VERTEX));
        assertFalse(modules.served(id));
        ByteBuffer copy = modules.copyForCompiler(id.toString(), ByteBuffer::allocateDirect);
        assertEquals(8, copy.remaining());
        assertEquals(spirv.rewind(), copy);
        assertTrue(modules.served(id));
        assertNull(modules.copyForCompiler("shaderbridge:spv/unknown", ByteBuffer::allocateDirect));
        modules.release(id);
        assertFalse(modules.contains(id));
        assertEquals(0, modules.size());
    }

    @Test
    void diagnosticsAreReportedOnce() {
        PipelineDiagnostics diagnostics = new PipelineDiagnostics();
        diagnostics.report("a");
        diagnostics.report("b");
        diagnostics.report("a");
        assertEquals(List.of("a", "b"), diagnostics.messages());
    }
}
