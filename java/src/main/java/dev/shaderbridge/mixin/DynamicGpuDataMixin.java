package dev.shaderbridge.mixin;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import dev.shaderbridge.render.shadow.ShadowTransforms;
import net.minecraft.client.renderer.DynamicGpuData;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/**
 * Records the {@code DynamicTransforms} blocks Minecraft writes while it prepares a pack frame's
 * features ({@link ShadowTransforms}), so the shadow pass can draw the same prepared features with
 * the shadow camera's model-view. Every transform write goes through these two methods (the other
 * {@code writeTransform} overloads delegate to the first). Not required: without it nothing is
 * recorded, and the shadow pass draws no features (entities cast no shadows) rather than drawing
 * them with the camera's model-view.
 */
@Mixin(DynamicGpuData.class)
abstract class DynamicGpuDataMixin {
    @Inject(method = "writeTransform(Lnet/minecraft/client/renderer/DynamicGpuData$Transform;)Lcom/mojang/renderpearl/api/buffers/GpuBufferSlice;",
        at = @At("RETURN"), require = 0)
    private void shaderbridge$recordTransform(DynamicGpuData.Transform transform, CallbackInfoReturnable<GpuBufferSlice> cir) {
        if (ShadowTransforms.recording()) {
            ShadowTransforms.record(transform, cir.getReturnValue());
        }
    }

    @Inject(method = "writeTransforms([Lnet/minecraft/client/renderer/DynamicGpuData$Transform;)[Lcom/mojang/renderpearl/api/buffers/GpuBufferSlice;",
        at = @At("RETURN"), require = 0)
    private void shaderbridge$recordTransforms(DynamicGpuData.Transform[] transforms, CallbackInfoReturnable<GpuBufferSlice[]> cir) {
        GpuBufferSlice[] slices = cir.getReturnValue();
        if (!ShadowTransforms.recording() || transforms == null || slices == null) {
            return;
        }
        for (int i = 0; i < Math.min(transforms.length, slices.length); i++) {
            ShadowTransforms.record(transforms[i], slices[i]);
        }
    }
}
