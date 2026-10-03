package dev.shaderbridge.mixin;

import com.mojang.renderpearl.api.pipeline.ShaderSource;
import com.mojang.renderpearl.api.pipeline.ShaderType;
import com.mojang.renderpearl.backend.api.SpvModule;
import com.mojang.renderpearl.frontend.shaders.GlslCompiler;
import com.mojang.renderpearl.frontend.shaders.SPIRVModule;
import dev.shaderbridge.render.pipeline.SpirvModules;
import java.nio.ByteBuffer;
import net.minecraft.client.renderer.ShaderDefines;
import org.lwjgl.system.MemoryUtil;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/**
 * SPIR-V injection: for shader ids of ShaderBridge modules ({@code shaderbridge:spv/<n>}),
 * {@code GlslCompiler#compileToSpv} returns the precompiled module instead of running shaderc on
 * the placeholder source. The module is a {@code memAlloc}'d copy because {@link SPIRVModule}
 * rewrites binding decorations in place and frees the buffer with {@code memFree} on close.
 *
 * <p>The injection is not required: if it does not apply (a renderpearl change), pack pipelines
 * fail to compile with the placeholder's {@code #error}, the pipeline cache reports the inactive
 * hook and shaders are disabled with a message, instead of the game crashing at start-up.
 */
@Mixin(value = GlslCompiler.class, remap = false)
abstract class GlslCompilerMixin {
    @Inject(
        method = "compileToSpv(Ljava/lang/String;Ljava/lang/String;Lcom/mojang/renderpearl/api/pipeline/ShaderType;"
            + "Lnet/minecraft/client/renderer/ShaderDefines;Lcom/mojang/renderpearl/api/pipeline/ShaderSource;)"
            + "Lcom/mojang/renderpearl/backend/api/SpvModule;",
        at = @At("HEAD"),
        cancellable = true,
        require = 0
    )
    private void shaderbridge$servePrecompiled(String name, String source, ShaderType type, ShaderDefines defines, ShaderSource shaderSource,
                                               CallbackInfoReturnable<SpvModule> cir) {
        if (!SpirvModules.isModuleId(name)) {
            return;
        }
        ByteBuffer spirv = SpirvModules.global().copyForCompiler(name, MemoryUtil::memAlloc);
        if (spirv != null) {
            cir.setReturnValue(new SPIRVModule(spirv, type));
        }
    }
}
