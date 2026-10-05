package dev.shaderbridge.compat.sodium;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import com.mojang.renderpearl.api.vertex.VertexFormatElement;
import dev.shaderbridge.render.pipeline.ProfileVertexFormats;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;

/**
 * The terrain vertex Sodium meshes while a pack is active: Sodium 0.9's 20-byte compact chunk
 * vertex, unchanged, followed by ShaderBridge's 16 bytes of extension attributes, which carry what
 * shader packs read from terrain vertices and Sodium does not store (36 bytes in all):
 *
 * <table>
 *   <caption>Layout</caption>
 *   <tr><th>Offset</th><th>Element</th><th>Format</th><th>Content</th></tr>
 *   <tr><td>0</td><td>{@code a_Position}</td><td>{@code RG32_UINT}</td><td>Sodium: 20-bit section-local position</td></tr>
 *   <tr><td>8</td><td>{@code a_Color}</td><td>{@code RGBA8_UNORM}</td><td>Sodium: color times ambient occlusion</td></tr>
 *   <tr><td>12</td><td>{@code a_TexCoord}</td><td>{@code RG16_UINT}</td><td>Sodium: 15-bit texture coordinate and bias sign</td></tr>
 *   <tr><td>16</td><td>{@code a_LightAndData}</td><td>{@code RGBA8_UINT}</td><td>Sodium: block and sky light, material, section index</td></tr>
 *   <tr><td>20</td><td>{@code sb_Entity}</td><td>{@code R32_UINT}</td><td>{@code ((block id + 1) << 1) | is fluid}</td></tr>
 *   <tr><td>24</td><td>{@code sb_Normal}</td><td>{@code RGBA8_SNORM}</td><td>face normal, w = 0</td></tr>
 *   <tr><td>28</td><td>{@code sb_MidTexCoord}</td><td>{@code RG16_UINT}</td><td>texture coordinate of the quad's centre times 32768</td></tr>
 *   <tr><td>32</td><td>{@code sb_MidBlock}</td><td>{@code RGBA8_SNORM}</td><td>(block centre minus vertex) times 64, w = light emission</td></tr>
 * </table>
 *
 * <p>The element names and formats are those of the {@value SodiumPipelines#PROFILE} draw
 * profile, so pack programs translated for it read this vertex. Sodium's own shader reads its four
 * elements by name and ignores the rest, so Sodium keeps drawing (with its own pipelines) from
 * the same buffers.
 */
public final class TerrainVertexLayout {
    /** Bytes of Sodium's compact chunk vertex. */
    public static final int SODIUM_VERTEX_SIZE = 20;
    /** Bytes of the extended vertex. */
    public static final int VERTEX_SIZE = 36;
    /** Offset of {@code sb_Entity}. */
    public static final int ENTITY_OFFSET = 20;
    /** Offset of {@code sb_Normal}. */
    public static final int NORMAL_OFFSET = 24;
    /** Offset of {@code sb_MidTexCoord}. */
    public static final int MID_TEX_COORD_OFFSET = 28;
    /** Offset of {@code sb_MidBlock}. */
    public static final int MID_BLOCK_OFFSET = 32;

    /**
     * One vertex element.
     *
     * @param name   element (and shader input) name
     * @param offset byte offset in the vertex
     * @param format element format
     */
    public record Element(String name, int offset, GpuFormat format) {
    }

    /**
     * Sodium 0.9's compact chunk vertex ({@code CompactChunkVertex.VERTEX_FORMAT}, verified in
     * 0.9.2 and 0.9.3-alpha.1 for Minecraft 26.3).
     */
    public static final List<Element> SODIUM = List.of(
        new Element("a_Position", 0, GpuFormat.RG32_UINT),
        new Element("a_Color", 8, GpuFormat.RGBA8_UNORM),
        new Element("a_TexCoord", 12, GpuFormat.RG16_UINT),
        new Element("a_LightAndData", 16, GpuFormat.RGBA8_UINT));

    /** ShaderBridge's extension attributes. */
    public static final List<Element> EXTENSION = List.of(
        new Element("sb_Entity", ENTITY_OFFSET, GpuFormat.R32_UINT),
        new Element("sb_Normal", NORMAL_OFFSET, GpuFormat.RGBA8_SNORM),
        new Element("sb_MidTexCoord", MID_TEX_COORD_OFFSET, GpuFormat.RG16_UINT),
        new Element("sb_MidBlock", MID_BLOCK_OFFSET, GpuFormat.RGBA8_SNORM));

    private static final VertexFormat FORMAT = build();

    private TerrainVertexLayout() {
    }

    /** @return the extended vertex format (one shared instance) */
    public static VertexFormat format() {
        return FORMAT;
    }

    private static VertexFormat build() {
        VertexFormat.Builder builder = VertexFormat.builder(0);
        for (Element element : SODIUM) {
            builder.addAttribute(element.name(), element.format());
        }
        for (Element element : EXTENSION) {
            builder.addAttribute(element.name(), element.format());
        }
        VertexFormat format = builder.build();
        List<String> problems = differences(format, elements(), VERTEX_SIZE);
        if (!problems.isEmpty()) {
            throw new IllegalStateException("the extended terrain vertex is laid out wrongly: " + problems);
        }
        return format;
    }

    /** @return every element of the extended vertex, in order */
    public static List<Element> elements() {
        List<Element> all = new ArrayList<>(SODIUM);
        all.addAll(EXTENSION);
        return all;
    }

    /**
     * Checks that the vertex format Sodium meshes with by default is the one the extended vertex
     * starts with: the extension's offsets and the encoder (which lets Sodium write the first 20
     * bytes) depend on it.
     *
     * @param compact Sodium's {@code ChunkMeshFormats.COMPACT} format
     * @return the differences, empty if it matches
     */
    public static List<String> sodiumProblems(VertexFormat compact) {
        return differences(compact, SODIUM, SODIUM_VERTEX_SIZE);
    }

    /**
     * Checks that the extended vertex feeds programs translated for the
     * {@value SodiumPipelines#PROFILE} profile (the same check the pipeline router makes).
     *
     * @param profile the profile's vertex layout, if known
     * @return the differences, empty if compatible
     */
    public static List<String> profileProblems(Optional<List<VertexFormat>> profile) {
        if (profile.isEmpty()) {
            return List.of("draw profile " + SodiumPipelines.PROFILE + " has no vertex layout");
        }
        return ProfileVertexFormats.compatibility(profile.get(), List.of(FORMAT));
    }

    private static List<String> differences(VertexFormat format, List<Element> expected, int size) {
        List<String> problems = new ArrayList<>();
        List<VertexFormatElement> actual = format.getElements();
        if (format.getVertexSize() != size) {
            problems.add("vertex size " + format.getVertexSize() + ", expected " + size);
        }
        if (format.getStepRate() != 0) {
            problems.add("step rate " + format.getStepRate() + ", expected 0");
        }
        if (actual.size() != expected.size()) {
            problems.add(actual.size() + " elements " + format + ", expected " + expected.size());
        }
        for (Element want : expected) {
            VertexFormatElement have = format.getElement(want.name());
            if (have == null) {
                problems.add("element " + want.name() + " is missing");
            } else if (have.offset() != want.offset() || have.format() != want.format()) {
                problems.add("element " + want.name() + " is " + have.format() + " at " + have.offset() + ", expected " + want.format() + " at "
                    + want.offset());
            }
        }
        return problems;
    }
}
