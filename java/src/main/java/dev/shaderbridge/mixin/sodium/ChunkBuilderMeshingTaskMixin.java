package dev.shaderbridge.mixin.sodium;

import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import dev.shaderbridge.compat.sodium.SodiumTargets;
import dev.shaderbridge.compat.sodium.SodiumTerrain;
import net.caffeinemc.mods.sodium.client.render.chunk.compile.ChunkBuildBuffers;
import net.caffeinemc.mods.sodium.client.render.chunk.compile.pipeline.BlockRenderer;
import net.caffeinemc.mods.sodium.client.render.chunk.compile.pipeline.FluidRenderer;
import net.caffeinemc.mods.sodium.client.render.chunk.compile.tasks.ChunkBuilderMeshingTask;
import net.caffeinemc.mods.sodium.client.render.chunk.translucent_sorting.TranslucentGeometryCollector;
import net.caffeinemc.mods.sodium.client.world.LevelSlice;
import net.minecraft.client.renderer.block.dispatch.BlockStateModel;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.FluidState;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;

/**
 * Tells the extended vertex encoder which block Sodium is meshing on this worker thread: around
 * Sodium's call that meshes a block's model, and around its call that meshes a block's fluid
 * (whose quads carry the id of the fluid's own block, e.g. water in a waterlogged stair, and the
 * fluid flag), within the block loop of {@code ChunkBuilderMeshingTask.execute}.
 */
@Mixin(value = ChunkBuilderMeshingTask.class, remap = false)
abstract class ChunkBuilderMeshingTaskMixin {
    @WrapOperation(method = SodiumTargets.EXECUTE, at = @At(value = "INVOKE", target = SodiumTargets.RENDER_MODEL), require = 0)
    private void shaderbridge$meshBlock(BlockRenderer renderer, BlockStateModel model, BlockState state, BlockPos pos, BlockPos origin,
                                        Operation<Void> original) {
        Object context = SodiumTerrain.enterBlock(state, state, origin, false);
        try {
            original.call(renderer, model, state, pos, origin);
        } finally {
            SodiumTerrain.exitBlock(context);
        }
    }

    @WrapOperation(method = SodiumTargets.EXECUTE, at = @At(value = "INVOKE", target = SodiumTargets.RENDER_FLUID), require = 0)
    private void shaderbridge$meshFluid(FluidRenderer renderer, LevelSlice slice, BlockState state, FluidState fluid, BlockPos pos, BlockPos origin,
                                        TranslucentGeometryCollector collector, ChunkBuildBuffers buffers, Operation<Void> original) {
        Object context = SodiumTerrain.enterBlock(fluid.createLegacyBlock(), state, origin, true);
        try {
            original.call(renderer, slice, state, fluid, pos, origin, collector, buffers);
        } finally {
            SodiumTerrain.exitBlock(context);
        }
    }
}
