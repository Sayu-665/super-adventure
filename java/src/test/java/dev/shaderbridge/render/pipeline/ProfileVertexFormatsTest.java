package dev.shaderbridge.render.pipeline;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import com.mojang.blaze3d.vertex.DefaultVertexFormat;
import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import com.mojang.renderpearl.api.vertex.VertexFormatElement;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.stream.Stream;
import org.junit.jupiter.api.Test;

class ProfileVertexFormatsTest {
    private static final ProfileVertexFormats FORMATS = ProfileVertexFormats.get();

    private static List<String> builtinProfileNames() throws IOException {
        String configured = System.getProperty("shaderbridge.repoRoot");
        Path dir = (configured != null ? Path.of(configured) : Path.of("..")).resolve("crates/sb-transform/profiles");
        assumeTrue(Files.isDirectory(dir), "sb-transform profiles not available");
        try (Stream<Path> files = Files.list(dir)) {
            return files.map(f -> f.getFileName().toString()).filter(n -> n.endsWith(".toml") && !n.equals("defaults.toml"))
                .map(n -> n.substring(0, n.length() - 5)).sorted().toList();
        }
    }

    @Test
    void everyBuiltinProfileHasALayoutThatFeedsEveryInput() throws IOException {
        List<String> names = builtinProfileNames();
        assertTrue(names.size() >= 18, names.toString());
        for (String name : names) {
            DrawProfileInfo info = DrawProfiles.get().profile(name).orElseThrow(() -> new AssertionError("profile " + name + " not packaged"));
            List<VertexFormat> bindings = FORMATS.bindings(name).orElseThrow(() -> new AssertionError(name + " has no vertex layout"));
            for (DrawProfileInfo.Input input : info.inputs()) {
                int slot = input.instanced() ? 1 : 0;
                assertTrue(slot < bindings.size(), name + ": slot " + slot);
                VertexFormatElement element = bindings.get(slot).getElement(input.name());
                assertNotNull(element, name + ": " + input.name());
                GlslInput glsl = GlslInput.of(input.type());
                assertEquals(glsl.scalar(), VertexInputCheck.attributeClass(element.format()), name + ": " + input.name());
                assertTrue(element.format().componentCount() >= glsl.components(), name + ": " + input.name());
                assertEquals(slot, bindings.get(slot).getStepRate(), name + ": " + input.name());
            }
        }
        assertEquals(List.of(), FORMATS.bindings("fullscreen").orElseThrow());
        assertEquals(List.of(), FORMATS.bindings("vanilla_clouds").orElseThrow());
    }

    /** GLSL vertex input types the profiles use. */
    private record GlslInput(ScalarClass scalar, int components) {
        static GlslInput of(String type) {
            ScalarClass scalar = switch (type.charAt(0)) {
                case 'i' -> ScalarClass.INT;
                case 'u' -> ScalarClass.UINT;
                default -> ScalarClass.FLOAT;
            };
            char last = type.charAt(type.length() - 1);
            return new GlslInput(scalar, Character.isDigit(last) ? last - '0' : 1);
        }
    }

    @Test
    void mojangElementFormatsAreDefaultVertexFormats() {
        for (VertexFormat format : List.of(DefaultVertexFormat.ENTITY_GLINT_SPECIAL, DefaultVertexFormat.POSITION_COLOR_NORMAL_LINE_WIDTH,
            DefaultVertexFormat.CHUNK_DATA_INSTANCED)) {
            for (VertexFormatElement e : format.getElements()) {
                assertEquals(e.format(), HostVertexLayouts.MOJANG_ELEMENTS.get(e.name()), e.name());
            }
        }
        assertEquals(DefaultVertexFormat.BLOCK, FORMATS.bindings("vanilla_block").orElseThrow().getFirst());
        assertEquals(List.of(DefaultVertexFormat.BLOCK, DefaultVertexFormat.CHUNK_DATA_INSTANCED), FORMATS.bindings("vanilla_terrain_basic").orElseThrow());
        assertEquals(DefaultVertexFormat.POSITION_TEX_LIGHTMAP_COLOR, FORMATS.bindings("vanilla_text").orElseThrow().getFirst());
        assertEquals(DefaultVertexFormat.PARTICLE, FORMATS.bindings("vanilla_particle").orElseThrow().getFirst());
    }

    @Test
    void explicitLayoutsMatchTheHostByteLayouts() {
        assertLayout(HostVertexLayouts.DH_TERRAIN, 16, Map.of("vPosition", 0, "meta", 6, "vColor", 8, "irisMaterial", 12, "irisNormal", 13,
            "textureTile", 14));
        assertLayout(HostVertexLayouts.DH_GENERIC, 20, Map.of("vPosition", 0, "aColor", 12, "aMaterial", 16));
        assertLayout(HostVertexLayouts.EXTENDED_TERRAIN, 52, Map.of("Position", 0, "Color", 12, "UV0", 16, "UV2", 24, "sb_Normal", 28,
            "sb_Entity", 32, "sb_MidTexCoord", 36, "sb_Tangent", 44, "sb_MidBlock", 48));
        assertLayout(HostVertexLayouts.CHUNK_INSTANCE, 16, Map.of("ChunkPosition", 0, "ChunkVisibility", 12));
        assertLayout(HostVertexLayouts.SODIUM_TERRAIN, 36, Map.of("a_Position", 0, "a_Color", 8, "a_TexCoord", 12, "a_LightAndData", 16,
            "sb_Entity", 20, "sb_Normal", 24, "sb_MidTexCoord", 28, "sb_MidBlock", 32));
        assertEquals(FORMATS.bindings("dh_terrain"), FORMATS.bindings(DrawProfiles.DH_SYNTH_PROFILE));
        assertEquals(DrawProfiles.get().profile("dh_terrain").orElseThrow().samplers(),
            DrawProfiles.get().profile(DrawProfiles.DH_SYNTH_PROFILE).orElseThrow().samplers());
    }

    private static void assertLayout(VertexFormat format, int stride, Map<String, Integer> offsets) {
        assertEquals(stride, format.getVertexSize(), format.toString());
        assertEquals(offsets.size(), format.getElements().size(), format.toString());
        offsets.forEach((name, offset) -> assertEquals(offset, format.getElement(name).offset(), name));
    }

    @Test
    void compatibilityAcceptsSupersetsAndReportsDifferences() {
        List<VertexFormat> entity = FORMATS.bindings("vanilla_entity").orElseThrow();
        assertEquals(List.of(), ProfileVertexFormats.compatibility(entity, List.of(DefaultVertexFormat.ENTITY)));
        assertEquals(List.of(), ProfileVertexFormats.compatibility(entity, List.of(DefaultVertexFormat.ENTITY_GLINT_SPECIAL)));
        assertEquals(2, ProfileVertexFormats.compatibility(entity, List.of(DefaultVertexFormat.BLOCK)).size(), "UV1 and Normal missing, one line each");
        assertFalse(ProfileVertexFormats.compatibility(entity, List.of()).isEmpty());
        VertexFormat wrongType = VertexFormat.builder(0).addAttribute("Position", GpuFormat.RGBA32_FLOAT).build();
        assertEquals(List.of("vertex element Position is RGBA32_FLOAT, the profile expects RGB32_FLOAT"),
            ProfileVertexFormats.compatibility(FORMATS.bindings("vanilla_position").orElseThrow(), List.of(wrongType)));
        List<VertexFormat> terrain = FORMATS.bindings("vanilla_terrain_basic").orElseThrow();
        VertexFormat perVertexChunk = VertexFormat.builder(0).addAttribute("ChunkPosition", GpuFormat.RGB32_SINT)
            .addAttribute("ChunkVisibility", GpuFormat.R32_FLOAT).build();
        assertEquals(List.of("vertex buffer slot 1 has step rate 0, the profile expects 1"),
            ProfileVertexFormats.compatibility(terrain, List.of(DefaultVertexFormat.BLOCK, perVertexChunk)));
    }

    @Test
    void derivedLayoutsPutInstancedInputsInSlotOne() {
        DrawProfileInfo info = new DrawProfileInfo("t", false, List.of(new DrawProfileInfo.Input("ChunkVisibility", "float", 1, true),
            new DrawProfileInfo.Input("Position", "vec3", 0, false)), List.of(), List.of());
        List<VertexFormat> bindings = ProfileVertexFormats.derive(info).orElseThrow();
        assertEquals(0, bindings.get(0).getStepRate());
        assertEquals(1, bindings.get(1).getStepRate());
        assertEquals(List.of("Position"), bindings.get(0).getElements().stream().map(VertexFormatElement::name).toList());
        DrawProfileInfo foreign = new DrawProfileInfo("f", false, List.of(new DrawProfileInfo.Input("a_Pos", "vec3", 0, false)), List.of(), List.of());
        assertTrue(ProfileVertexFormats.derive(foreign).isEmpty());
        ProfileVertexFormats registry = new ProfileVertexFormats(DrawProfiles.get());
        assertTrue(registry.bindings("othermod_terrain").isEmpty());
        VertexFormat custom = VertexFormat.builder(0).addAttribute("a_Pos", GpuFormat.RGB32_FLOAT).build();
        registry.register("othermod_terrain", List.of(custom));
        assertEquals(List.of(custom), registry.bindings("othermod_terrain").orElseThrow());
    }
}
