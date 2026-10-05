package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import java.util.List;
import java.util.Map;

/**
 * Byte-exact vertex layouts of the built-in draw profiles whose attributes are not Mojang's
 * standard elements: the vanilla terrain format extended with ShaderBridge's attributes, Distant
 * Horizons' LOD and generic-object vertices (DH 3.3 {@code BlazeDhTerrainRenderer} /
 * {@code BlazeDhGenericObjectRenderer}) and Sodium's compact chunk vertex with the extension
 * attributes. They mirror {@code crates/sb-runtime/src/scene/formats.rs}, the headless executor's
 * copy of the same contract.
 */
final class HostVertexLayouts {
    /** Distant Horizons LOD vertex: 16 bytes. */
    static final VertexFormat DH_TERRAIN = VertexFormat.builder(0)
        .addAttribute("vPosition", GpuFormat.RGB16_UINT)
        .addAttribute("meta", GpuFormat.R16_UINT)
        .addAttribute("vColor", GpuFormat.RGBA8_UNORM)
        .addAttribute("irisMaterial", GpuFormat.R8_UINT)
        .addAttribute("irisNormal", GpuFormat.R8_UINT)
        .addAttribute("textureTile", GpuFormat.R16_UINT)
        .build();

    /** Distant Horizons generic object vertex: 20 bytes, the material followed by three padding bytes. */
    static final VertexFormat DH_GENERIC = VertexFormat.builder(0)
        .addAttribute("vPosition", GpuFormat.RGB32_FLOAT)
        .addAttribute("aColor", GpuFormat.RGBA8_UNORM)
        .addAttribute("aMaterial", 4, GpuFormat.R8_UINT)
        .build();

    /** Mojang's {@code BLOCK} elements followed by ShaderBridge's terrain attributes: 52 bytes. */
    static final VertexFormat EXTENDED_TERRAIN = VertexFormat.builder(0)
        .addAttribute("Position", GpuFormat.RGB32_FLOAT)
        .addAttribute("Color", GpuFormat.RGBA8_UNORM)
        .addAttribute("UV0", GpuFormat.RG32_FLOAT)
        .addAttribute("UV2", GpuFormat.RG16_SINT)
        .addAttribute("sb_Normal", GpuFormat.RGBA8_SNORM)
        .addAttribute("sb_Entity", GpuFormat.RG16_SINT)
        .addAttribute("sb_MidTexCoord", GpuFormat.RG32_FLOAT)
        .addAttribute("sb_Tangent", GpuFormat.RGBA8_SNORM)
        .addAttribute("sb_MidBlock", GpuFormat.RGBA8_SINT)
        .build();

    /** Mojang's per-section instance data of the MultiDrawIndirect terrain path: 16 bytes. */
    static final VertexFormat CHUNK_INSTANCE = VertexFormat.builder(1)
        .addAttribute("ChunkPosition", GpuFormat.RGB32_SINT)
        .addAttribute("ChunkVisibility", GpuFormat.R32_FLOAT)
        .build();

    /** Sodium 0.9 compact chunk vertex with the extension attributes: 36 bytes. */
    static final VertexFormat SODIUM_TERRAIN = VertexFormat.builder(0)
        .addAttribute("a_Position", GpuFormat.RG32_UINT)
        .addAttribute("a_Color", GpuFormat.RGBA8_UNORM)
        .addAttribute("a_TexCoord", GpuFormat.RG16_UINT)
        .addAttribute("a_LightAndData", GpuFormat.RGBA8_UINT)
        .addAttribute("sb_Entity", GpuFormat.R32_UINT)
        .addAttribute("sb_Normal", GpuFormat.RGBA8_SNORM)
        .addAttribute("sb_MidTexCoord", GpuFormat.RG16_UINT)
        .addAttribute("sb_MidBlock", GpuFormat.RGBA8_SNORM)
        .build();

    /** Vertex buffer slots per profile. */
    static final Map<String, List<VertexFormat>> BY_PROFILE = Map.of(
        "vanilla_terrain", List.of(EXTENDED_TERRAIN, CHUNK_INSTANCE),
        "vanilla_terrain_section_ext", List.of(EXTENDED_TERRAIN),
        "dh_terrain", List.of(DH_TERRAIN),
        DrawProfiles.DH_SYNTH_PROFILE, List.of(DH_TERRAIN),
        "dh_generic", List.of(DH_GENERIC),
        "sodium_terrain", List.of(SODIUM_TERRAIN));

    /**
     * Mojang 26.3's element formats ({@code DefaultVertexFormat}) by element name. Profiles whose
     * inputs all have these names describe vanilla vertex data.
     */
    static final Map<String, GpuFormat> MOJANG_ELEMENTS = Map.of(
        "Position", GpuFormat.RGB32_FLOAT,
        "Color", GpuFormat.RGBA8_UNORM,
        "UV0", GpuFormat.RG32_FLOAT,
        "UV1", GpuFormat.RG16_SINT,
        "UV2", GpuFormat.RG16_SINT,
        "UV3", GpuFormat.RG32_FLOAT,
        "Normal", GpuFormat.RGBA8_SNORM,
        "LineWidth", GpuFormat.R32_FLOAT,
        "ChunkPosition", GpuFormat.RGB32_SINT,
        "ChunkVisibility", GpuFormat.R32_FLOAT);

    private HostVertexLayouts() {
    }
}
