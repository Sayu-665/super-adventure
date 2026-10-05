package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.render.pipeline.PipelineDiagnostics;
import dev.shaderbridge.render.targets.HostTextures;
import dev.shaderbridge.render.targets.PackTargets;
import dev.shaderbridge.render.targets.TextureResolver;
import java.util.concurrent.Executor;
import java.util.function.Supplier;

/**
 * What the raw path of one dimension pipeline works with, owned by the pack's resources.
 *
 * @param dim         the dimension pipeline
 * @param depthMode   the pack's depth convention (comparison samplers)
 * @param targets     its render targets
 * @param resolver    resolves the pack's sampled resources to Minecraft textures and samplers
 * @param host        the game's textures for the current frame ({@code depthtex0}, lightmap, ...)
 * @param mainColor   the format of Minecraft's main color target, which {@code final} draws into
 * @param diagnostics receives skipped programs and stand-in bindings
 * @param compiler    runs pipeline creation off the render thread
 */
public record RawContext(DimensionPipeline dim, DepthMode depthMode, PackTargets targets, TextureResolver resolver, Supplier<HostTextures> host,
                         Supplier<GpuFormat> mainColor, PipelineDiagnostics diagnostics, Executor compiler) {
}
