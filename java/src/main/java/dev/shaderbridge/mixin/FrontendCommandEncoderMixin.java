package dev.shaderbridge.mixin;

import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.commands.RenderPassDescriptor;
import com.mojang.renderpearl.frontend.FrontendCommandEncoder;
import dev.shaderbridge.render.draw.PassRedirect;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/**
 * Lets {@link PassRedirect} hand ShaderBridge's render passes to code that creates its own while
 * a redirection is armed (Distant Horizons' generic object renderer, replayed into the pack's
 * gbuffers attachments). Every other {@code createRenderPass} overload delegates to this one.
 * Outside a redirection nothing changes. Not required: without it Distant Horizons' generic
 * objects draw into its own textures, which are not shown while a pack renders.
 */
@Mixin(value = FrontendCommandEncoder.class, remap = false)
abstract class FrontendCommandEncoderMixin {
    @Inject(method = "createRenderPass(Lcom/mojang/renderpearl/api/commands/RenderPassDescriptor;)Lcom/mojang/renderpearl/api/commands/RenderPass;",
        at = @At("HEAD"), cancellable = true, require = 0)
    private void shaderbridge$redirect(RenderPassDescriptor descriptor, CallbackInfoReturnable<RenderPass> cir) {
        if (PassRedirect.armed()) {
            RenderPass pass = PassRedirect.redirect(descriptor);
            if (pass != null) {
                cir.setReturnValue(pass);
            }
        }
    }
}
