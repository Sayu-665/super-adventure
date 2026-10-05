package dev.shaderbridge.mixin;

import dev.shaderbridge.dh.CameraFarPlane;
import net.minecraft.client.Camera;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.ModifyArg;

/**
 * Extends the far plane of Minecraft's level projection to the Distant Horizons far plane while a
 * pack renders synthesized LODs, so that vanilla terrain and LODs share one projection and one
 * depth space ({@link CameraFarPlane}). The camera's culling frustum and {@code depthFar} keep
 * Minecraft's value. Not required: without it no synthesized LODs are drawn
 * ({@link CameraFarPlane#extendedTo} stays false).
 */
@Mixin(Camera.class)
abstract class CameraMixin {
    @ModifyArg(method = "update", at = @At(value = "INVOKE", target = "Lnet/minecraft/client/Camera;setupPerspective(FFFFF)V"), index = 1,
        require = 0)
    private float shaderbridge$farPlane(float far) {
        return CameraFarPlane.apply(far);
    }
}
