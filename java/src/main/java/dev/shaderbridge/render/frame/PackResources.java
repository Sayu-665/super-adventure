package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.device.GpuDevice;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.pack.LoadedPack;
import dev.shaderbridge.pack.PackException;
import dev.shaderbridge.render.draw.DrawSubstitution;
import dev.shaderbridge.render.draw.UniformBinder;
import dev.shaderbridge.render.draw.VanillaClones;
import dev.shaderbridge.render.mapping.PipelineRouter;
import dev.shaderbridge.render.pipeline.CompiledVariants;
import dev.shaderbridge.render.pipeline.DrawProfiles;
import dev.shaderbridge.render.pipeline.OnDemandVariants;
import dev.shaderbridge.render.pipeline.PackPipelineCache;
import dev.shaderbridge.render.pipeline.PackPipelineFactory;
import dev.shaderbridge.render.pipeline.PackShaderSource;
import dev.shaderbridge.render.pipeline.PipelineCapabilities;
import dev.shaderbridge.render.pipeline.PipelineDiagnostics;
import dev.shaderbridge.render.pipeline.ProfileVertexFormats;
import dev.shaderbridge.render.pipeline.ProgramResolver;
import dev.shaderbridge.render.pipeline.RawPath;
import dev.shaderbridge.render.raw.RawBackend;
import dev.shaderbridge.render.raw.RawContext;
import dev.shaderbridge.render.pipeline.SessionVariantCompiler;
import dev.shaderbridge.render.pipeline.SpirvModules;
import dev.shaderbridge.render.targets.PackFiles;
import dev.shaderbridge.render.targets.PackTargets;
import dev.shaderbridge.render.targets.PackTextures;
import dev.shaderbridge.render.targets.TargetSpec;
import dev.shaderbridge.render.targets.TextureReader;
import dev.shaderbridge.render.targets.TextureResolver;
import dev.shaderbridge.uniforms.DrawUniforms;
import dev.shaderbridge.uniforms.FrameState;
import dev.shaderbridge.uniforms.FrameUniforms;
import dev.shaderbridge.uniforms.UniformEvaluator;
import dev.shaderbridge.uniforms.UniformSettings;
import java.io.IOException;
import java.util.ArrayDeque;
import java.util.Deque;
import java.util.HashMap;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import net.minecraft.client.Minecraft;
import net.minecraft.util.Util;

/**
 * Everything one dimension pipeline of an active pack renders with: render targets and textures,
 * pipeline compilation and program resolution, uniform buffers, the frame sequencer, the
 * Distant Horizons state, and the helpers that adapt vanilla draws. Created on the render thread when the pack becomes active in a
 * dimension and closed when it no longer is. Render thread only.
 */
final class PackResources implements AutoCloseable {
    final LoadedPack pack;
    final DimensionPipeline dim;
    final DepthMode depthMode;
    final GpuDevice device;
    final PipelineDiagnostics diagnostics = new PipelineDiagnostics();
    final PackTargets targets;
    final PackTextures textures;
    final TextureResolver textureResolver;
    final PackPipelineCache pipelines;
    final ProgramResolver programs;
    final RawPath raw;
    final VanillaClones clones;
    final DrawSubstitution substitution;
    final UniformBinder binder;
    final FrameState frameState = new FrameState();
    final FrameUniforms frameUniforms;
    final DrawUniforms drawUniforms;
    final DrawSlots drawSlots = new DrawSlots();
    final SinkTextures sinks;
    final FrameSequencer sequencer;
    final DistantFrame distant;
    private final ExecutorService variantCompiler;
    private final Deque<AutoCloseable> owned = new ArrayDeque<>();

    /**
     * @param pack    the active pack
     * @param dim     the dimension pipeline to render
     * @param backend the raw Vulkan path's backend (what pipelines may do on this device, and the
     *                raw path itself, which declines everything on OpenGL)
     * @throws IOException   if the pack's files cannot be opened (custom textures)
     * @throws PackException if the pack's session is closed (custom uniforms)
     */
    PackResources(LoadedPack pack, DimensionPipeline dim, RawBackend backend) throws IOException, PackException {
        this.pack = pack;
        this.dim = dim;
        this.depthMode = pack.model().info().environment().depthMode();
        this.device = RenderSystem.getDevice();
        PipelineCapabilities capabilities = backend.capabilities();
        Minecraft minecraft = Minecraft.getInstance();
        RenderTarget main = minecraft.gameRenderer.mainRenderTarget();
        try {
            own(backend.requestStorage(dim));
            this.targets = own(PackTargets.create(device, dim, main.width, main.height, main.getDepthTexture().getFormat()));
            this.sinks = own(new SinkTextures(device));
            this.distant = own(new DistantFrame(device, dim.distantHorizons(), depthMode, diagnostics::report));
            Map<String, byte[]> rawFiles = new HashMap<>();
            try (PackFiles files = PackFiles.open(pack.session().path())) {
                this.textures = own(PackTextures.load(device, device.createCommandEncoder(), dim, TextureReader.of(files, minecraft.getResourceManager()),
                    diagnostics::report));
                for (String path : backend.packFiles(dim)) {
                    files.read(path).ifPresent(data -> rawFiles.put(path, data));
                }
            }
            this.textureResolver = new TextureResolver(dim, targets, textures, RenderSystem.getSamplerCache(), depthMode, diagnostics::report);
            this.raw = own(backend.open(new RawContext(dim, depthMode, targets, textureResolver, this::host,
                () -> minecraft.gameRenderer.mainRenderTarget().getColorTexture().getFormat(), diagnostics, Util.backgroundExecutor()),
                pack.blobs(), rawFiles));
            SpirvModules modules = SpirvModules.global();
            this.pipelines = own(new PackPipelineCache(device, new PackShaderSource(modules), Util.backgroundExecutor(), minecraft, modules));
            this.variantCompiler = Executors.newSingleThreadExecutor(r -> {
                Thread t = new Thread(r, "ShaderBridge variant compiler");
                t.setDaemon(true);
                return t;
            });
            owned.push(variantCompiler::shutdownNow);
            OnDemandVariants variants = new OnDemandVariants(new CompiledVariants(pack.blobs()), new SessionVariantCompiler(pack.session()),
                variantCompiler);
            PackPipelineFactory factory = new PackPipelineFactory(pack.model().info().sourceHash(), modules, capabilities, DrawProfiles.get());
            this.programs = own(new ProgramResolver(dim, pack.blobs(), depthMode, factory, pipelines, raw, variants, diagnostics));
            this.clones = new VanillaClones(RenderSystem::getCompiledPipelineNullable, capabilities.independentBlend());
            this.substitution = new DrawSubstitution(dim, new PipelineRouter(ProfileVertexFormats.get())::route, programs::geometry);
            this.binder = new UniformBinder(textureResolver::resolve, diagnostics::report);
            this.frameState.configure(UniformSettings.of(dim));
            this.frameUniforms = own(new FrameUniforms(dim.uniforms().frame(), UniformEvaluator.create(pack.session(), dim.folder()).orElse(null)));
            this.drawUniforms = own(new DrawUniforms(dim.uniforms().draw(), device.getDeviceInfo().limits().minUniformOffsetAlignment()));
            this.sequencer = new FrameSequencer(dim, i -> targets.color(i).isPresent(),
                targets.shadowColorTargets().stream().map(p -> p.spec()).filter(s -> !s.clear()).map(TargetSpec::index).toList());
        } catch (IOException | PackException | RuntimeException e) {
            close();
            throw e;
        }
    }

    /** @return the game's textures (block atlas as albedo, lightmap, depth, Distant Horizons textures) for the current frame */
    MinecraftHost host() {
        return MinecraftHost.blockAtlas(distant);
    }

    private <T extends AutoCloseable> T own(T resource) {
        owned.push(resource);
        return resource;
    }

    /** Closes everything, newest first; failures are suppressed (the GPU objects die with the device anyway). */
    @Override
    public void close() {
        while (!owned.isEmpty()) {
            try {
                owned.pop().close();
            } catch (Exception ignored) {
                // Best effort: a failure here must not keep the other resources alive.
            }
        }
    }
}
