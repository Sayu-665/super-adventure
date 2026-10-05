package dev.shaderbridge.mixin;

import com.llamalad7.mixinextras.injector.ModifyReturnValue;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import dev.shaderbridge.render.chunk.ChunkMeshFormat;
import net.minecraft.client.renderer.chunk.ChunkSectionLayer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;

/**
 * The vertex format and pipelines of chunk section layers follow ShaderBridge's chunk mesh format
 * ({@link ChunkMeshFormat}): while a pack renders, layers are meshed in the extended terrain vertex
 * format and drawn with the extended clones of the terrain pipelines. Minecraft derives everything
 * format-dependent from these two methods: the section builders' format
 * ({@code SectionCompiler.getOrBeginLayer}), the vertex size of the section buffer heaps
 * ({@code SectionRenderDispatcher}) and the base vertex of every draw
 * ({@code LevelRenderer.extractSectionDrawGroups}). With no pack both return Minecraft's values.
 * Not required: without them {@link ChunkMeshFormat} detects the missing hook and keeps the
 * vanilla format.
 */
@Mixin(ChunkSectionLayer.class)
abstract class ChunkSectionLayerMixin {
    @ModifyReturnValue(method = "pipeline(Z)Lcom/mojang/renderpearl/api/pipeline/RenderPipeline;", at = @At("RETURN"), require = 0)
    private RenderPipeline shaderbridge$extendedPipeline(RenderPipeline pipeline) {
        return ChunkMeshFormat.pipeline(pipeline);
    }

    @ModifyReturnValue(method = "vertexFormat()Lcom/mojang/renderpearl/api/vertex/VertexFormat;", at = @At("RETURN"), require = 0)
    private VertexFormat shaderbridge$extendedFormat(VertexFormat format) {
        return ChunkMeshFormat.vertexFormat(format);
    }
}
