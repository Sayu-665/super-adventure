package dev.shaderbridge.render.targets;

import dev.shaderbridge.model.CustomTexture;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.model.ShadowSettings;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import dev.shaderbridge.render.pipeline.TextureFormats;
import java.util.List;
import java.util.Optional;

/**
 * The sampler of each pack resource, with the rules of the headless executor ({@code sb-runtime}):
 * render targets are clamped and filtered linearly when their format allows it (integer formats
 * are sampled with nearest), mip levels are used only by programs that request the target's
 * mipmaps; depth textures are nearest; shadow maps and shadow color targets follow
 * {@code shadowtexNNearest} / {@code shadowcolorNNearest} / {@code shadowcolorNMipmap}; the noise
 * texture is linear and repeated; custom textures follow their {@code blur} and {@code clamp}
 * flags; the block atlas is nearest, mipmapped and repeated.
 */
public final class SamplerChoice {
    /** The block atlas and the textures sampled in its place (normals, specular). */
    public static final SamplerSpec ATLAS = new SamplerSpec(false, true, true);
    /** The noise texture. */
    public static final SamplerSpec NOISE = new SamplerSpec(true, false, true);

    private SamplerChoice() {
    }

    /** The render targets a sampler choice depends on. */
    public interface Targets {
        /**
         * @param index a colortex index
         * @return the target, if it exists
         */
        Optional<TargetSpec> color(int index);

        /**
         * @param index a shadowcolor index
         * @return the target, if it exists
         */
        Optional<TargetSpec> shadowColor(int index);
    }

    /**
     * @param ref     the resource
     * @param dim     its dimension pipeline
     * @param program the program that samples it
     * @param targets the render targets
     * @return the sampler parameters
     */
    public static SamplerSpec of(ResourceRef ref, DimensionPipeline dim, Program program, Targets targets) {
        ShadowSettings shadow = dim.targets().shadow();
        return switch (ref) {
            case ResourceRef.ColorTex c -> colorTarget(targets.color(c.index()), program.mipmapTargets().contains(c.index()));
            case ResourceRef.ColorImage c -> colorTarget(targets.color(c.index()), false);
            case ResourceRef.ShadowColor c -> shadowColor(targets.shadowColor(c.index()), shadow, c.index());
            case ResourceRef.ShadowColorImage c -> shadowColor(targets.shadowColor(c.index()), shadow, c.index());
            case ResourceRef.ShadowTex s -> new SamplerSpec(!flag(shadow.nearest(), s.index()), false, false);
            case ResourceRef.ShadowTexHw s -> new SamplerSpec(!flag(shadow.nearest(), s.index()), false, false);
            case ResourceRef.Noise n -> NOISE;
            case ResourceRef.Atlas a -> ATLAS;
            case ResourceRef.Normals n -> ATLAS;
            case ResourceRef.Specular s -> ATLAS;
            case ResourceRef.Lightmap l -> SamplerSpec.LINEAR_CLAMP;
            case ResourceRef.CustomTexture t -> customTexture(dim, t.id());
            case ResourceRef.Image i -> SamplerSpec.LINEAR_CLAMP;
            case ResourceRef.Unknown u -> isGeometry(program) ? ATLAS : colorTarget(targets.color(0), false);
            case ResourceRef.DepthTex d -> SamplerSpec.NEAREST_CLAMP;
            case ResourceRef.DhDepthTex d -> SamplerSpec.NEAREST_CLAMP;
            case ResourceRef.Overlay o -> SamplerSpec.NEAREST_CLAMP;
            case ResourceRef.DhBlockAtlas a -> SamplerSpec.NEAREST_CLAMP;
            case ResourceRef.White w -> SamplerSpec.NEAREST_CLAMP;
            case ResourceRef.Ssbo s -> SamplerSpec.NEAREST_CLAMP;
            case ResourceRef.UniformBlock b -> SamplerSpec.NEAREST_CLAMP;
        };
    }

    private static SamplerSpec colorTarget(Optional<TargetSpec> target, boolean programWantsMips) {
        return target.map(t -> new SamplerSpec(TextureFormats.numericClass(t.format()) == ScalarClass.FLOAT, programWantsMips && t.mipLevels() > 1, false))
            .orElse(SamplerSpec.NEAREST_CLAMP);
    }

    private static SamplerSpec shadowColor(Optional<TargetSpec> target, ShadowSettings shadow, int index) {
        return target.map(t -> new SamplerSpec(!flag(shadow.colorNearest(), index) && TextureFormats.numericClass(t.format()) == ScalarClass.FLOAT,
            t.mipLevels() > 1, false)).orElse(SamplerSpec.NEAREST_CLAMP);
    }

    /**
     * The sampler of a custom texture id ({@code <stage>.<sampler>}, {@code <stage>.<sampler>.<dim>}
     * for raw textures).
     */
    private static SamplerSpec customTexture(DimensionPipeline dim, String id) {
        return dim.targets().customTextures().stream().filter(t -> CustomTextureIds.matches(t, id)).findFirst()
            .map(SamplerChoice::customTexture).orElse(SamplerSpec.NEAREST_CLAMP);
    }

    private static SamplerSpec customTexture(CustomTexture t) {
        return new SamplerSpec(t.blur(), false, !t.clamp());
    }

    private static boolean flag(List<Boolean> flags, int index) {
        return index >= 0 && index < flags.size() && flags.get(index);
    }

    private static boolean isGeometry(Program program) {
        return program.kind() instanceof ProgramKind.Geometry;
    }
}
