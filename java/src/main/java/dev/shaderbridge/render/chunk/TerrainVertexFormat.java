package dev.shaderbridge.render.chunk;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import com.mojang.renderpearl.api.vertex.VertexFormatElement;

/**
 * ShaderBridge's extended chunk vertex: Mojang's {@code BLOCK} elements (position, color, atlas
 * coordinates, lightmap) followed by the attributes shader packs read on terrain, 52 bytes in all.
 * It is the layout of the {@code vanilla_terrain} and {@code vanilla_terrain_section_ext} draw
 * profiles (the {@code vanilla_terrain} binding 0 of {@code HostVertexLayouts} and
 * {@code crates/sb-runtime/src/scene/formats.rs}), and the same size and element order as Iris'
 * extended terrain format:
 *
 * <table>
 *   <caption>Elements</caption>
 *   <tr><th>Offset</th><th>Element</th><th>Format</th><th>Content</th></tr>
 *   <tr><td>0</td><td>{@code Position}</td><td>{@code RGB32_FLOAT}</td><td>section-relative position</td></tr>
 *   <tr><td>12</td><td>{@code Color}</td><td>{@code RGBA8_UNORM}</td><td>tint and ambient occlusion</td></tr>
 *   <tr><td>16</td><td>{@code UV0}</td><td>{@code RG32_FLOAT}</td><td>atlas coordinates</td></tr>
 *   <tr><td>24</td><td>{@code UV2}</td><td>{@code RG16_SINT}</td><td>lightmap (block, sky)</td></tr>
 *   <tr><td>28</td><td>{@code sb_Normal}</td><td>{@code RGBA8_SNORM}</td><td>face normal, w = 0</td></tr>
 *   <tr><td>32</td><td>{@code sb_Entity}</td><td>{@code RG16_SINT}</td><td>{@code block.properties} id (-1 if unmapped),
 *   render type (0 block, 1 fluid)</td></tr>
 *   <tr><td>36</td><td>{@code sb_MidTexCoord}</td><td>{@code RG32_FLOAT}</td><td>centre of the quad's atlas coordinates</td></tr>
 *   <tr><td>44</td><td>{@code sb_Tangent}</td><td>{@code RGBA8_SNORM}</td><td>tangent, w = handedness</td></tr>
 *   <tr><td>48</td><td>{@code sb_MidBlock}</td><td>{@code RGBA8_SINT}</td><td>(block centre - vertex) x 64, w = light emission</td></tr>
 * </table>
 */
public final class TerrainVertexFormat {
    /** Element read as {@code gl_Normal}. */
    public static final String NORMAL = "sb_Normal";
    /** Element read as {@code mc_Entity}. */
    public static final String ENTITY = "sb_Entity";
    /** Element read as {@code mc_midTexCoord}. */
    public static final String MID_TEX_COORD = "sb_MidTexCoord";
    /** Element read as {@code at_tangent}. */
    public static final String TANGENT = "sb_Tangent";
    /** Element read as {@code at_midBlock}. */
    public static final String MID_BLOCK = "sb_MidBlock";

    /** The extended chunk vertex format (one instance; pipelines and builders compare it by identity). */
    public static final VertexFormat EXTENDED = VertexFormat.builder(0)
        .addAttribute("Position", GpuFormat.RGB32_FLOAT)
        .addAttribute("Color", GpuFormat.RGBA8_UNORM)
        .addAttribute("UV0", GpuFormat.RG32_FLOAT)
        .addAttribute("UV2", GpuFormat.RG16_SINT)
        .addAttribute(NORMAL, GpuFormat.RGBA8_SNORM)
        .addAttribute(ENTITY, GpuFormat.RG16_SINT)
        .addAttribute(MID_TEX_COORD, GpuFormat.RG32_FLOAT)
        .addAttribute(TANGENT, GpuFormat.RGBA8_SNORM)
        .addAttribute(MID_BLOCK, GpuFormat.RGBA8_SINT)
        .build();

    /** Where the encoder finds and writes each element of {@link #EXTENDED}. */
    public static final TerrainVertexEncoder.Layout LAYOUT = layoutOf(EXTENDED);

    private TerrainVertexFormat() {
    }

    /**
     * @param format a vertex format with the elements of {@link #EXTENDED}
     * @return the byte offsets of the elements the encoder reads and writes
     * @throws IllegalArgumentException if an element is missing
     */
    public static TerrainVertexEncoder.Layout layoutOf(VertexFormat format) {
        return new TerrainVertexEncoder.Layout(format.getVertexSize(), offset(format, "Position"), offset(format, "UV0"), offset(format, NORMAL),
            offset(format, ENTITY), offset(format, MID_TEX_COORD), offset(format, TANGENT), offset(format, MID_BLOCK));
    }

    private static int offset(VertexFormat format, String name) {
        VertexFormatElement element = format.getElement(name);
        if (element == null) {
            throw new IllegalArgumentException("vertex format " + format + " has no element " + name);
        }
        return element.offset();
    }
}
