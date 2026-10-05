package dev.shaderbridge.render.targets;

import com.mojang.blaze3d.systems.SamplerCache;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.render.pipeline.DepthStates;
import java.util.Optional;
import java.util.function.Consumer;

/**
 * Turns a pack resource ({@code BindingPlan.Source.Pack}) into the texture view and sampler to
 * bind: render targets (main or alt per the program's flip state), depth copies, shadow maps,
 * pack textures, and host textures. Resources that do not exist bind a neutral texture (black for
 * colors, the far plane for depth, white for custom textures, as the headless executor does) and
 * are reported once. A sampler the pack did not name (GL texture unit 0) is the albedo in geometry
 * programs and {@code colortex0} elsewhere. Render thread only.
 */
public final class TextureResolver {
    private final DimensionPipeline dim;
    private final PackTargets targets;
    private final PackTextures textures;
    private final SamplerCache samplers;
    private final DepthMode depthMode;
    private final Consumer<String> warnings;
    private final SamplerChoice.Targets specs;

    /**
     * @param dim       the dimension pipeline
     * @param targets   its render targets
     * @param textures  its textures
     * @param samplers  Mojang's sampler cache
     * @param depthMode the pack's depth convention (which neutral texture means "far")
     * @param warnings  receives messages about missing resources (deduplicate them)
     */
    public TextureResolver(DimensionPipeline dim, PackTargets targets, PackTextures textures, SamplerCache samplers, DepthMode depthMode,
                           Consumer<String> warnings) {
        this.dim = dim;
        this.targets = targets;
        this.textures = textures;
        this.samplers = samplers;
        this.depthMode = depthMode;
        this.warnings = warnings;
        this.specs = new SamplerChoice.Targets() {
            @Override
            public Optional<TargetSpec> color(int index) {
                return targets.color(index).map(ColorPair::spec);
            }

            @Override
            public Optional<TargetSpec> shadowColor(int index) {
                return targets.shadowColor(index).map(ColorPair::spec);
            }
        };
    }

    /**
     * @param ref     the resource
     * @param useAlt  for a ping-ponged color target: read the alternate texture
     *                ({@code BindingUse.use_alt} for colortex; the caller's shadowcomp flip
     *                tracking for shadowcolor)
     * @param program the program that samples it
     * @param host    the game's textures for the current draw
     * @return what to bind
     */
    public TextureBinding resolve(ResourceRef ref, boolean useAlt, Program program, HostTextures host) {
        SamplerSpec sampler = samplerSpec(ref, program);
        Optional<GpuTextureView> copy = host.passCopy(ref);
        if (copy.isPresent()) {
            return bind(copy.get(), sampler);
        }
        return switch (ref) {
            case ResourceRef.ColorTex c -> color(c.index(), useAlt, sampler);
            case ResourceRef.ColorImage c -> color(c.index(), useAlt, sampler);
            case ResourceRef.ShadowColor c -> shadowColor(c.index(), useAlt, sampler);
            case ResourceRef.ShadowColorImage c -> shadowColor(c.index(), useAlt, sampler);
            case ResourceRef.DepthTex d -> bind(d.index() == 0 ? host.mainDepth() : targets.depthCopyView(Math.clamp(d.index(), 1, 2)), sampler);
            case ResourceRef.ShadowTex s -> bind(targets.shadowDepthView(Math.clamp(s.index(), 0, 1)), sampler);
            case ResourceRef.ShadowTexHw s -> bind(targets.shadowDepthView(Math.clamp(s.index(), 0, 1)), sampler);
            case ResourceRef.Noise n -> bind(textures.noise(), sampler);
            case ResourceRef.White w -> bind(textures.white(), sampler);
            case ResourceRef.Atlas a -> host.atlas();
            case ResourceRef.Lightmap l -> host.lightmap();
            case ResourceRef.Overlay o -> host.overlay();
            case ResourceRef.Normals n -> host.normals().orElseGet(() -> bind(textures.flatNormal(), SamplerSpec.NEAREST_CLAMP));
            case ResourceRef.Specular s -> host.specular().orElseGet(() -> bind(textures.noSpecular(), SamplerSpec.NEAREST_CLAMP));
            case ResourceRef.DhDepthTex d -> host.dhDepth(Math.clamp(d.index(), 0, 1)).map(v -> bind(v, sampler)).orElseGet(this::farDepth);
            case ResourceRef.DhBlockAtlas a -> host.dhBlockAtlas().orElseGet(() -> bind(textures.white(), sampler));
            case ResourceRef.CustomTexture t -> custom(t.id(), sampler, host);
            case ResourceRef.Unknown u -> program.kind() instanceof ProgramKind.Geometry ? host.atlas() : color(0, useAlt, sampler);
            case ResourceRef.Image i -> missing("custom image " + i.name() + " is a storage image, which only the raw Vulkan path provides");
            case ResourceRef.Ssbo s -> missing("SSBO " + s.index() + " cannot be bound as a sampler");
            case ResourceRef.UniformBlock b -> missing("uniform block " + b.name() + " cannot be bound as a sampler");
        };
    }

    /**
     * @param ref     a sampled resource
     * @param program the program that samples it
     * @return the filtering and addressing the pack asks for it (the raw Vulkan path builds its
     *     comparison samplers from it)
     */
    public SamplerSpec samplerSpec(ResourceRef ref, Program program) {
        return SamplerChoice.of(ref, dim, program, specs);
    }

    private TextureBinding color(int index, boolean alt, SamplerSpec sampler) {
        return targets.color(index).map(p -> bind(p.sampleView(alt), sampler))
            .orElseGet(() -> missing("colortex" + index + " is sampled but has no render target"));
    }

    private TextureBinding shadowColor(int index, boolean alt, SamplerSpec sampler) {
        return targets.shadowColor(index).map(p -> bind(p.sampleView(alt), sampler))
            .orElseGet(() -> missing("shadowcolor" + index + " is sampled but has no render target"));
    }

    private TextureBinding custom(String id, SamplerSpec sampler, HostTextures host) {
        return switch (textures.custom(id).orElse(null)) {
            case PackTextures.Loaded loaded -> bind(loaded.view(), sampler);
            case PackTextures.Custom.HostLightmap lightmap -> host.lightmap();
            case null -> {
                warnings.accept("custom texture " + id + " is not declared by the pack; white is bound instead");
                yield bind(textures.white(), SamplerSpec.NEAREST_CLAMP);
            }
        };
    }

    /** A depth texture at the far plane, in the pack's depth convention. */
    private TextureBinding farDepth() {
        return bind(DepthStates.reversed(depthMode) ? textures.black() : textures.white(), SamplerSpec.NEAREST_CLAMP);
    }

    private TextureBinding missing(String message) {
        warnings.accept(message + "; black is bound instead");
        return bind(textures.black(), SamplerSpec.NEAREST_CLAMP);
    }

    private TextureBinding bind(GpuTextureView view, SamplerSpec sampler) {
        return new TextureBinding(view, sampler.sampler(samplers));
    }
}
