package dev.shaderbridge.mixin;

import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.frontend.FrontendRenderPass;
import dev.shaderbridge.render.draw.ActivePasses;
import dev.shaderbridge.render.draw.RenderPassUniforms;
import java.util.HashMap;
import org.spongepowered.asm.mixin.Final;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.ModifyVariable;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Pipeline substitution: when vanilla code binds a pipeline in one of ShaderBridge's render passes
 * ({@link ActivePasses}), the pipeline actually bound is the pack program that replaces it (or the
 * vanilla pipeline adapted to the pass's attachments), and once it is bound the pack program's
 * descriptors ({@code sb_Frame}, {@code sb_Draw}, pack samplers) are bound too. Passes that are not
 * ShaderBridge's are untouched.
 *
 * <p>Not required: without it the pass attachments do not match vanilla pipelines, the first draw
 * fails, and the pack is disabled with a message.
 */
@Mixin(value = FrontendRenderPass.class, remap = false)
abstract class FrontendRenderPassMixin {
    @Shadow
    @Final
    protected HashMap<String, Object> uniforms;

    @Unique
    private ActivePasses.Substitution shaderbridge$pending;

    @ModifyVariable(method = "setPipeline", at = @At("HEAD"), argsOnly = true, require = 0)
    private CompiledRenderPipeline shaderbridge$substitute(CompiledRenderPipeline requested) {
        ActivePasses.Substitution substitution = ActivePasses.substitute(this, requested);
        shaderbridge$pending = substitution;
        return substitution == null ? requested : substitution.pipeline();
    }

    @Inject(method = "setPipeline", at = @At("RETURN"), require = 0)
    private void shaderbridge$bindPackDescriptors(CompiledRenderPipeline pipeline, CallbackInfo ci) {
        ActivePasses.Substitution substitution = shaderbridge$pending;
        shaderbridge$pending = null;
        if (substitution != null) {
            substitution.bind().accept(new RenderPassUniforms((RenderPass) (Object) this, uniforms));
        }
    }
}
