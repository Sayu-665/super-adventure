package dev.shaderbridge.mixin;

import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import com.mojang.blaze3d.vertex.BufferBuilder;
import com.mojang.blaze3d.vertex.ByteBufferBuilder;
import com.mojang.renderpearl.api.pipeline.PrimitiveTopology;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import dev.shaderbridge.render.chunk.ChunkMeshFormat;
import net.minecraft.client.renderer.block.BlockAndTintGetter;
import net.minecraft.client.renderer.block.FluidRenderer;
import net.minecraft.client.renderer.chunk.RenderSectionRegion;
import net.minecraft.client.renderer.chunk.SectionCompiler;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.FluidState;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Redirect;

/**
 * Chunk section meshing in the extended terrain vertex format ({@link ChunkMeshFormat}):
 *
 * <ul>
 *   <li>the builder of a layer meshed in the extended format records the block of every quad and
 *   fills the extension attributes when the layer is built;</li>
 *   <li>{@code compile} tells it which block it meshes: the block state read at each position,
 *   and the fluid while the block's fluid is tesselated. Every block quad emitter (Minecraft's
 *   block renderer, Fabric's renderer API) runs between these points.</li>
 * </ul>
 *
 * With no pack the builder is Minecraft's and the block hooks do nothing. Not required: without
 * the builder hook extended meshes keep undefined extension attributes, without the block hooks
 * they carry no block ids.
 */
@Mixin(SectionCompiler.class)
abstract class SectionCompilerMixin {
    @Redirect(method = "getOrBeginLayer", at = @At(value = "NEW", target = "com/mojang/blaze3d/vertex/BufferBuilder"), require = 0)
    private BufferBuilder shaderbridge$layerBuilder(ByteBufferBuilder buffer, PrimitiveTopology topology, VertexFormat format) {
        return ChunkMeshFormat.newBuilder(buffer, topology, format);
    }

    @WrapOperation(
        method = "compile",
        at = @At(value = "INVOKE", target = "Lnet/minecraft/client/renderer/chunk/RenderSectionRegion;getBlockState("
            + "Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;"),
        require = 0
    )
    private BlockState shaderbridge$blockContext(RenderSectionRegion region, BlockPos pos, Operation<BlockState> original) {
        BlockState state = original.call(region, pos);
        ChunkMeshFormat.enterBlock(state, pos);
        return state;
    }

    @WrapOperation(
        method = "compile",
        at = @At(value = "INVOKE", target = "Lnet/minecraft/client/renderer/block/FluidRenderer;tesselate("
            + "Lnet/minecraft/client/renderer/block/BlockAndTintGetter;Lnet/minecraft/core/BlockPos;"
            + "Lnet/minecraft/client/renderer/block/FluidRenderer$Output;Lnet/minecraft/world/level/block/state/BlockState;"
            + "Lnet/minecraft/world/level/material/FluidState;)V"),
        require = 0
    )
    private void shaderbridge$fluidContext(FluidRenderer renderer, BlockAndTintGetter level, BlockPos pos, FluidRenderer.Output output,
                                           BlockState state, FluidState fluid, Operation<Void> original) {
        boolean entered = ChunkMeshFormat.enterFluid(fluid);
        try {
            original.call(renderer, level, pos, output, state, fluid);
        } finally {
            if (entered) {
                ChunkMeshFormat.exitFluid();
            }
        }
    }
}
