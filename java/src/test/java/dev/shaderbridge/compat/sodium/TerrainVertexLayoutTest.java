package dev.shaderbridge.compat.sodium;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.pipeline.BindGroupLayout;
import com.mojang.renderpearl.api.pipeline.UniformType;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import com.mojang.renderpearl.api.vertex.VertexFormatElement;
import dev.shaderbridge.render.pipeline.DrawProfileInfo;
import dev.shaderbridge.render.pipeline.DrawProfiles;
import dev.shaderbridge.render.pipeline.ProfileVertexFormats;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import net.caffeinemc.mods.sodium.client.render.chunk.ShaderChunkRenderer;
import net.caffeinemc.mods.sodium.client.render.chunk.vertex.format.ChunkMeshFormats;
import org.junit.jupiter.api.Test;

/**
 * {@link TerrainVertexLayout}: the extended terrain vertex starts with Sodium's real compact
 * vertex (read from the Sodium jar), and feeds the packaged {@code sodium_terrain} draw profile:
 * every profile input is an element of the same name whose format has the input's base type and
 * enough components (Mojang's pipeline builder's rules).
 */
class TerrainVertexLayoutTest {
    /** GLSL input type of each vertex format, as the pipeline builder accepts them. */
    private static final Map<GpuFormat, String> GLSL = Map.of(
        GpuFormat.RG32_UINT, "uvec2",
        GpuFormat.RGBA8_UNORM, "vec4",
        GpuFormat.RG16_UINT, "uvec2",
        GpuFormat.RGBA8_UINT, "uvec4",
        GpuFormat.R32_UINT, "uint",
        GpuFormat.RGBA8_SNORM, "vec4");

    @Test
    void theExtendedVertexIsSodiumsFollowedByTheExtension() {
        VertexFormat format = TerrainVertexLayout.format();
        assertEquals(TerrainVertexLayout.VERTEX_SIZE, format.getVertexSize());
        assertEquals(0, format.getStepRate());
        List<String> names = format.getElements().stream().map(VertexFormatElement::name).toList();
        assertEquals(List.of("a_Position", "a_Color", "a_TexCoord", "a_LightAndData", "sb_Entity", "sb_Normal", "sb_MidTexCoord", "sb_MidBlock"), names);
        assertEquals(List.of(0, 8, 12, 16, 20, 24, 28, 32), format.getElements().stream().map(VertexFormatElement::offset).toList());
        assertEquals(TerrainVertexLayout.ENTITY_OFFSET, format.getElement("sb_Entity").offset());
        assertEquals(TerrainVertexLayout.NORMAL_OFFSET, format.getElement("sb_Normal").offset());
        assertEquals(TerrainVertexLayout.MID_TEX_COORD_OFFSET, format.getElement("sb_MidTexCoord").offset());
        assertEquals(TerrainVertexLayout.MID_BLOCK_OFFSET, format.getElement("sb_MidBlock").offset());
    }

    @Test
    void sodiumsCompactVertexIsTheOneTheExtensionExtends() {
        VertexFormat compact = ChunkMeshFormats.COMPACT.getVertexFormat();
        assertEquals(List.of(), TerrainVertexLayout.sodiumProblems(compact));
        assertEquals(TerrainVertexLayout.SODIUM_VERTEX_SIZE, compact.getVertexSize());
    }

    @Test
    void anotherCompactVertexIsReported() {
        VertexFormat wider = VertexFormat.builder(0)
            .addAttribute("a_Position", GpuFormat.RGB32_FLOAT)
            .addAttribute("a_Color", GpuFormat.RGBA8_UNORM)
            .addAttribute("a_TexCoord", GpuFormat.RG16_UINT)
            .addAttribute("a_LightAndData", GpuFormat.RGBA8_UINT)
            .build();
        List<String> problems = TerrainVertexLayout.sodiumProblems(wider);
        assertTrue(problems.contains("vertex size 24, expected 20"), problems.toString());
        assertTrue(problems.stream().anyMatch(p -> p.startsWith("element a_Position is RGB32_FLOAT at 0")), problems.toString());
        assertTrue(problems.stream().anyMatch(p -> p.startsWith("element a_Color is RGBA8_UNORM at 12")), problems.toString());
    }

    @Test
    void theExtendedVertexFeedsTheSodiumTerrainProfile() {
        assertEquals(List.of(), TerrainVertexLayout.profileProblems(ProfileVertexFormats.get().bindings(SodiumPipelines.PROFILE)));
        assertFalse(TerrainVertexLayout.profileProblems(Optional.empty()).isEmpty());
    }

    @Test
    void everyProfileInputHasAnElementOfItsType() {
        DrawProfileInfo profile = DrawProfiles.get().profile(SodiumPipelines.PROFILE).orElseThrow();
        assertFalse(profile.inputs().isEmpty());
        VertexFormat format = TerrainVertexLayout.format();
        for (DrawProfileInfo.Input input : profile.inputs()) {
            VertexFormatElement element = format.getElement(input.name());
            assertTrue(element != null, input.name() + " has no element");
            assertEquals(GLSL.get(element.format()), input.type(), input.name());
            assertFalse(input.instanced(), input.name());
        }
        assertEquals(TerrainVertexLayout.elements().size(), profile.inputs().size(), "every element is a profile input");
    }

    @Test
    void sodiumsCompactVertexAloneDoesNotFeedTheProfile() {
        List<String> problems = ProfileVertexFormats.compatibility(ProfileVertexFormats.get().bindings(SodiumPipelines.PROFILE).orElseThrow(),
            List.of(ChunkMeshFormats.COMPACT.getVertexFormat()));
        assertTrue(problems.stream().anyMatch(p -> p.startsWith("vertex element sb_Entity is missing")), problems.toString());
    }

    @Test
    void theProfileHostsAreSodiumsBindings() {
        DrawProfileInfo profile = DrawProfiles.get().profile(SodiumPipelines.PROFILE).orElseThrow();
        Map<String, UniformType> sodium = new java.util.LinkedHashMap<>();
        for (BindGroupLayout.UniformDescription u : BindGroupLayout.flattenUniforms(List.of(ShaderChunkRenderer.BIND_GROUP, ShaderChunkRenderer.LIGHT_GROUP))) {
            sodium.put(u.name(), u.type());
        }
        assertEquals(Map.of("u_BlockTex", UniformType.COMBINED_IMAGE_SAMPLER, "u_Globals", UniformType.UNIFORM_BUFFER, "u_SectionTimeInfo",
            UniformType.TEXEL_BUFFER, "u_LightTex", UniformType.COMBINED_IMAGE_SAMPLER), sodium);
        assertEquals(List.of("u_Globals"), profile.blocks());
        for (String block : profile.blocks()) {
            assertEquals(UniformType.UNIFORM_BUFFER, sodium.get(block), block);
        }
        assertEquals(List.of("u_BlockTex", "u_LightTex"), profile.samplers().stream().map(DrawProfileInfo.Sampler::name).toList());
        for (DrawProfileInfo.Sampler sampler : profile.samplers()) {
            assertEquals(UniformType.COMBINED_IMAGE_SAMPLER, sodium.get(sampler.name()), sampler.name());
        }
        assertEquals(List.of("gtexture"), profile.sampler("u_BlockTex").orElseThrow().provides());
        assertEquals(List.of("lightmap"), profile.sampler("u_LightTex").orElseThrow().provides());
    }
}
