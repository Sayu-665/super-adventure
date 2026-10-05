package dev.shaderbridge.render.chunk;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import dev.shaderbridge.render.mapping.VanillaPipelineTable;
import java.util.List;
import net.minecraft.client.renderer.RenderPipelines;
import org.junit.jupiter.api.Test;

class ExtendedTerrainPipelinesTest {
    /** Every pipeline Minecraft 26.3 draws chunk sections with. */
    static final List<RenderPipeline> TERRAIN = List.of(RenderPipelines.SOLID_TERRAIN, RenderPipelines.SOLID_TERRAIN_MULTIDRAW,
        RenderPipelines.CUTOUT_TERRAIN, RenderPipelines.CUTOUT_TERRAIN_MULTIDRAW, RenderPipelines.TRANSLUCENT_TERRAIN,
        RenderPipelines.TRANSLUCENT_TERRAIN_MULTIDRAW, RenderPipelines.WIREFRAME, RenderPipelines.WIREFRAME_MULTIDRAW);

    @Test
    void clonesDrawTheExtendedFormatAndKeepEverythingElse() {
        for (RenderPipeline vanilla : TERRAIN) {
            assertTrue(ExtendedTerrainPipelines.drawsBlockVertices(vanilla), vanilla.toString());
            RenderPipeline clone = ExtendedTerrainPipelines.extended(vanilla);
            String where = vanilla.toString();
            assertEquals(VanillaPipelineTable.extendedTerrainLocation(vanilla.getLocation()), clone.getLocation(), where);
            assertSame(TerrainVertexFormat.EXTENDED, clone.getVertexFormatBinding(0), where);
            assertEquals(vanilla.getVertexFormatBindings().size(), clone.getVertexFormatBindings().size(), where);
            assertEquals(vanilla.getVertexFormatBindings().subList(1, vanilla.getVertexFormatBindings().size()),
                clone.getVertexFormatBindings().subList(1, clone.getVertexFormatBindings().size()), where + ": instance data");
            assertEquals(vanilla.getShaders(), clone.getShaders(), where);
            assertSame(vanilla.getShaderDefines(), clone.getShaderDefines(), where);
            assertEquals(vanilla.getBindGroupLayouts(), clone.getBindGroupLayouts(), where);
            assertEquals(vanilla.getColorTargetStates(), clone.getColorTargetStates(), where);
            assertEquals(vanilla.getDepthStencilState(), clone.getDepthStencilState(), where);
            assertEquals(vanilla.getPolygonMode(), clone.getPolygonMode(), where);
            assertEquals(vanilla.isCull(), clone.isCull(), where);
            assertEquals(vanilla.getPrimitiveTopology(), clone.getPrimitiveTopology(), where);
            assertEquals(vanilla.pushConstantSize(), clone.pushConstantSize(), where);
            assertSame(clone, ExtendedTerrainPipelines.extended(vanilla), where + ": one clone per pipeline");
            assertSame(clone, ExtendedTerrainPipelines.extended(clone), where + ": idempotent");
        }
    }

    @Test
    void otherPipelinesAreLeftAlone() {
        for (RenderPipeline other : List.of(RenderPipelines.ENTITY_SOLID, RenderPipelines.SKY, RenderPipelines.LINES)) {
            assertFalse(ExtendedTerrainPipelines.drawsBlockVertices(other), other.toString());
            assertSame(other, ExtendedTerrainPipelines.extended(other));
        }
        assertNull(ExtendedTerrainPipelines.extended(null));
    }
}
