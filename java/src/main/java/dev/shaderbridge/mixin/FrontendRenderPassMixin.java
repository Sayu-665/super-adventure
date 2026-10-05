package dev.shaderbridge.mixin;

import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.pipeline.CompiledRenderPipeline;
import com.mojang.renderpearl.api.textures.GpuSampler;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import com.mojang.renderpearl.backend.api.RenderPassBackend;
import com.mojang.renderpearl.frontend.FrontendRenderPass;
import dev.shaderbridge.render.draw.ActivePasses;
import dev.shaderbridge.render.draw.PassBackend;
import dev.shaderbridge.render.draw.RenderPassUniforms;
import java.util.HashMap;
import java.util.function.Consumer;
import org.spongepowered.asm.mixin.Final;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Pipeline substitution in ShaderBridge's render passes ({@link ActivePasses}); passes that are
 * not ShaderBridge's are untouched:
 *
 * <ul>
 *   <li>when vanilla code binds a pipeline, the pipeline actually bound is the pack program that
 *   replaces it (or the vanilla pipeline adapted to the pass's attachments), and once it is bound
 *   the pack program's descriptors ({@code sb_Frame}, {@code sb_Draw}, pack samplers) are bound
 *   too;</li>
 *   <li>a pipeline that cannot be drawn in the pass at all (another mod's pipeline that fits
 *   neither the pass nor an adapted clone) is not bound, and its draws and push constants are
 *   dropped until the next pipeline: the frame goes on without them;</li>
 *   <li>when vanilla code then binds another albedo ({@code Sampler0}), the descriptors that
 *   depend on it are bound again;</li>
 *   <li>uniform buffers a pass replaces are swapped as vanilla code binds them (the shadow pass's
 *   features get their shadow-camera {@code DynamicTransforms});</li>
 *   <li>when the pass is closed with debug groups still open (vanilla code failed between
 *   pushing and popping one, e.g. because a pipeline could not be bound), they are popped, so
 *   that the pass still ends and the failure reaches ShaderBridge's frame handling instead of
 *   leaving Mojang's command encoder inside a pass that can never be closed.</li>
 * </ul>
 *
 * <p>Not required: without the substitution the pass attachments do not match vanilla pipelines,
 * the first draw fails, and the pack is disabled with a message.
 */
@Mixin(value = FrontendRenderPass.class, remap = false)
abstract class FrontendRenderPassMixin implements PassBackend {
    @Shadow
    @Final
    protected HashMap<String, Object> uniforms;

    @Shadow
    @Final
    private RenderPassBackend backend;

    @Shadow
    private boolean isClosed;

    @Shadow
    private int pushedDebugGroups;

    /** The descriptors of the pack pipeline bound in this pass, or null for a vanilla pipeline. */
    @Unique
    private ActivePasses.PackBinding shaderbridge$packBinding;

    /** Set while ShaderBridge binds, so its own albedo binding does not trigger a rebind. */
    @Unique
    private boolean shaderbridge$binding;

    /** The last pipeline vanilla code bound is skipped: its draws are dropped. */
    @Unique
    private boolean shaderbridge$skipping;

    @WrapMethod(method = "setPipeline(Lcom/mojang/renderpearl/api/pipeline/CompiledRenderPipeline;)V", require = 0)
    private void shaderbridge$setPipeline(CompiledRenderPipeline requested, Operation<Void> original) {
        ActivePasses.Substitution substitution = ActivePasses.substitute(this, requested);
        shaderbridge$packBinding = null;
        shaderbridge$skipping = false;
        if (substitution == null) {
            original.call(requested);
            return;
        }
        if (substitution.skipped()) {
            shaderbridge$skipping = true;
            return;
        }
        original.call(substitution.pipeline());
        shaderbridge$packBinding = substitution.binding().orElse(null);
        if (shaderbridge$packBinding != null) {
            shaderbridge$run(shaderbridge$packBinding::bind);
        }
    }

    @Inject(method = {"drawIndexed(IIIII)V", "multiDrawIndexed(Ljava/nio/IntBuffer;III)V",
        "multiDrawIndexed(Lorg/lwjgl/PointerBuffer;Ljava/nio/IntBuffer;Ljava/nio/IntBuffer;I)V",
        "drawIndexedIndirect(Lcom/mojang/renderpearl/api/buffers/GpuBufferSlice;I)V",
        "drawMultipleIndexed(Ljava/util/Collection;Lcom/mojang/renderpearl/api/buffers/GpuBuffer;Lcom/mojang/renderpearl/api/pipeline/IndexType;Ljava/util/Collection;Ljava/lang/Object;)V",
        "draw(IIII)V", "multiDraw(Ljava/nio/IntBuffer;III)V", "multiDraw(Ljava/nio/IntBuffer;Ljava/nio/IntBuffer;I)V",
        "drawIndirect(Lcom/mojang/renderpearl/api/buffers/GpuBufferSlice;I)V", "pushConstants(Ljava/nio/ByteBuffer;)V"},
        at = @At("HEAD"), cancellable = true, require = 0)
    private void shaderbridge$dropSkippedDraws(CallbackInfo ci) {
        if (shaderbridge$skipping) {
            ci.cancel();
        }
    }

    @WrapMethod(method = "setUniform(Ljava/lang/String;Lcom/mojang/renderpearl/api/buffers/GpuBufferSlice;)V", require = 0)
    private void shaderbridge$replaceUniform(String name, GpuBufferSlice value, Operation<Void> original) {
        original.call(name, ActivePasses.uniform(this, name, value));
    }

    @Inject(method = "setUniform(Ljava/lang/String;Lcom/mojang/renderpearl/api/textures/GpuTextureView;Lcom/mojang/renderpearl/api/textures/GpuSampler;)V",
        at = @At("RETURN"), require = 0)
    private void shaderbridge$albedoChanged(String name, GpuTextureView view, GpuSampler sampler, CallbackInfo ci) {
        if (shaderbridge$packBinding != null && !shaderbridge$binding && ActivePasses.ALBEDO_SAMPLER.equals(name)) {
            shaderbridge$run(shaderbridge$packBinding::albedoChanged);
        }
    }

    @Inject(method = "close", at = @At("HEAD"), require = 0)
    private void shaderbridge$popOpenDebugGroups(CallbackInfo ci) {
        if (!isClosed && pushedDebugGroups > 0 && ActivePasses.owns(this)) {
            for (; pushedDebugGroups > 0; pushedDebugGroups--) {
                backend.popDebugGroup();
            }
        }
    }

    @Override
    public RenderPassBackend shaderbridge$backend() {
        return backend;
    }

    @Unique
    private void shaderbridge$run(Consumer<RenderPassUniforms> binding) {
        shaderbridge$binding = true;
        try {
            binding.accept(new RenderPassUniforms((RenderPass) (Object) this, uniforms));
        } finally {
            shaderbridge$binding = false;
        }
    }
}
