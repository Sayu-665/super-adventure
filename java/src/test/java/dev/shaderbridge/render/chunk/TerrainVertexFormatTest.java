package dev.shaderbridge.render.chunk;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;

import com.mojang.blaze3d.vertex.DefaultVertexFormat;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import com.mojang.renderpearl.api.vertex.VertexFormatElement;
import dev.shaderbridge.render.mapping.VanillaPipelineTable;
import dev.shaderbridge.render.pipeline.DrawProfileInfo;
import dev.shaderbridge.render.pipeline.DrawProfiles;
import dev.shaderbridge.render.pipeline.ProfileVertexFormats;
import java.util.List;
import org.junit.jupiter.api.Test;

class TerrainVertexFormatTest {
    private record Element(String name, Object format, int offset) {
        static List<Element> of(VertexFormat format) {
            return format.getElements().stream().map(e -> new Element(e.name(), e.format(), e.offset())).toList();
        }
    }

    @Test
    void extendedFormatIsTheVanillaTerrainProfileLayout() {
        assertEquals(52, TerrainVertexFormat.EXTENDED.getVertexSize());
        for (String profile : List.of(VanillaPipelineTable.TERRAIN_MULTIDRAW_EXTENDED, VanillaPipelineTable.TERRAIN_SECTION_EXTENDED)) {
            VertexFormat host = ProfileVertexFormats.get().bindings(profile).orElseThrow().getFirst();
            assertEquals(Element.of(host), Element.of(TerrainVertexFormat.EXTENDED), profile);
            assertEquals(host.getVertexSize(), TerrainVertexFormat.EXTENDED.getVertexSize(), profile);
        }
    }

    @Test
    void extendedFormatStartsWithTheBlockFormat() {
        // Minecraft's terrain shaders read these elements at these offsets from extended meshes.
        List<Element> block = Element.of(DefaultVertexFormat.BLOCK);
        assertEquals(block, Element.of(TerrainVertexFormat.EXTENDED).subList(0, block.size()));
    }

    @Test
    void everyExtendedProfileInputIsAnElement() {
        for (String profile : List.of(VanillaPipelineTable.TERRAIN_MULTIDRAW_EXTENDED, VanillaPipelineTable.TERRAIN_SECTION_EXTENDED)) {
            DrawProfileInfo info = DrawProfiles.get().profile(profile).orElseThrow();
            for (DrawProfileInfo.Input input : info.inputs()) {
                if (!input.instanced()) {
                    VertexFormatElement element = TerrainVertexFormat.EXTENDED.getElement(input.name());
                    assertNotNull(element, profile + ": " + input.name());
                }
            }
        }
    }

    @Test
    void encoderLayout() {
        assertEquals(new TerrainVertexEncoder.Layout(52, 0, 16, 28, 32, 36, 44, 48), TerrainVertexFormat.LAYOUT);
    }
}
