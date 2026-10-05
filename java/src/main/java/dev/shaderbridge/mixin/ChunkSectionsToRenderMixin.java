package dev.shaderbridge.mixin;

import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import dev.shaderbridge.render.chunk.ChunkMeshFormat;
import net.minecraft.client.renderer.chunk.ChunkSectionsToRender;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.ModifyArg;

/**
 * The pipelines that replace the layers' own when chunk sections are drawn (the wireframe debug
 * view in {@code renderGroup}, order-independent transparency in {@code renderOit}) draw the
 * meshes' vertex format too: while chunk sections are meshed in the extended terrain vertex
 * format, their extended clones are drawn instead ({@link ChunkMeshFormat#pipeline}). Not
 * required: without it those two views draw extended meshes with the {@code BLOCK} vertex stride.
 */
@Mixin(ChunkSectionsToRender.class)
abstract class ChunkSectionsToRenderMixin {
    @ModifyArg(
        method = {"renderGroup", "renderOit"},
        at = @At(value = "INVOKE", target = "Lnet/minecraft/client/renderer/chunk/ChunkSectionsToRender;renderLayers("
            + "[Lnet/minecraft/client/renderer/chunk/ChunkSectionLayer;Lcom/mojang/renderpearl/api/textures/GpuSampler;"
            + "Lcom/mojang/renderpearl/api/commands/RenderPass;Lcom/mojang/renderpearl/api/textures/GpuTextureView;"
            + "Lcom/mojang/renderpearl/api/textures/GpuTextureView;Lcom/mojang/renderpearl/api/pipeline/RenderPipeline;"
            + "Lcom/mojang/renderpearl/api/pipeline/RenderPipeline;)V"),
        index = 5,
        require = 0
    )
    private RenderPipeline shaderbridge$extendedOverride(RenderPipeline override) {
        return ChunkMeshFormat.pipeline(override);
    }

    @ModifyArg(
        method = {"renderGroup", "renderOit"},
        at = @At(value = "INVOKE", target = "Lnet/minecraft/client/renderer/chunk/ChunkSectionsToRender;renderLayers("
            + "[Lnet/minecraft/client/renderer/chunk/ChunkSectionLayer;Lcom/mojang/renderpearl/api/textures/GpuSampler;"
            + "Lcom/mojang/renderpearl/api/commands/RenderPass;Lcom/mojang/renderpearl/api/textures/GpuTextureView;"
            + "Lcom/mojang/renderpearl/api/textures/GpuTextureView;Lcom/mojang/renderpearl/api/pipeline/RenderPipeline;"
            + "Lcom/mojang/renderpearl/api/pipeline/RenderPipeline;)V"),
        index = 6,
        require = 0
    )
    private RenderPipeline shaderbridge$extendedMultidrawOverride(RenderPipeline override) {
        return ChunkMeshFormat.pipeline(override);
    }
}
