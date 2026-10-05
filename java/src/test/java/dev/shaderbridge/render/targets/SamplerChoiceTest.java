package dev.shaderbridge.render.targets;

import static org.junit.jupiter.api.Assertions.assertEquals;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.CustomTexture;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.render.RenderFixture;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.function.Function;
import java.util.stream.Collectors;
import org.junit.jupiter.api.Test;

class SamplerChoiceTest {
    private static final RenderFixture GLIMMER = RenderFixture.load(RenderFixture.GLIMMER);
    private static final DimensionPipeline DIM = GLIMMER.dim();
    private static final Map<Integer, TargetSpec> COLOR = TargetPlanner.colorTargets(DIM, 1920, 1080, f -> 16384).stream()
        .collect(Collectors.toMap(TargetSpec::index, Function.identity()));
    private static final Map<Integer, TargetSpec> SHADOW = TargetPlanner.shadowColorTargets(DIM, f -> 16384).stream()
        .collect(Collectors.toMap(TargetSpec::index, Function.identity()));
    private static final SamplerChoice.Targets TARGETS = new SamplerChoice.Targets() {
        @Override
        public Optional<TargetSpec> color(int index) {
            return Optional.ofNullable(COLOR.get(index));
        }

        @Override
        public Optional<TargetSpec> shadowColor(int index) {
            return Optional.ofNullable(SHADOW.get(index));
        }
    };

    private static SamplerSpec of(ResourceRef ref, Program program) {
        return SamplerChoice.of(ref, DIM, program, TARGETS);
    }

    @Test
    void renderTargetsAreClampedLinearAndMipmappedOnlyWhereRequested() {
        Program composite90 = GLIMMER.program("world0/composite90", "fullscreen");
        Program final_ = GLIMMER.program("world0/final", "fullscreen");
        assertEquals(new SamplerSpec(true, true, false), of(new ResourceRef.ColorTex(0), composite90), "composite90 requests colortex0 mipmaps");
        assertEquals(new SamplerSpec(true, false, false), of(new ResourceRef.ColorTex(0), final_));
        assertEquals(SamplerSpec.NEAREST_CLAMP, of(new ResourceRef.ColorTex(31), final_), "a missing target");
        assertEquals(new SamplerSpec(true, false, false), of(new ResourceRef.ShadowColor(1), final_));
    }

    @Test
    void depthNoiseAndHostTextures() {
        Program final_ = GLIMMER.program("world0/final", "fullscreen");
        Program terrain = GLIMMER.program("world0/gbuffers_textured_lit", "vanilla_terrain");
        assertEquals(SamplerSpec.NEAREST_CLAMP, of(new ResourceRef.DepthTex(1), final_));
        assertEquals(SamplerSpec.NEAREST_CLAMP, of(new ResourceRef.DhDepthTex(0), final_));
        assertEquals(new SamplerSpec(true, false, false), of(new ResourceRef.ShadowTex(0), final_), "shadowtex0Nearest is off");
        assertEquals(SamplerChoice.NOISE, of(new ResourceRef.Noise(), final_));
        assertEquals(SamplerChoice.ATLAS, of(new ResourceRef.Atlas(), terrain));
        assertEquals(SamplerSpec.LINEAR_CLAMP, of(new ResourceRef.Lightmap(), terrain));
        assertEquals(SamplerChoice.ATLAS, of(new ResourceRef.Unknown("voxelMap"), terrain), "unit 0 is the atlas in gbuffers");
        assertEquals(new SamplerSpec(true, false, false), of(new ResourceRef.Unknown("voxelMap"), final_), "and colortex0 in fullscreen passes");
    }

    @Test
    void customTexturesFollowBlurAndClamp() {
        Program final_ = GLIMMER.program("world0/final", "fullscreen");
        assertEquals(new SamplerSpec(true, false, true), of(new ResourceRef.CustomTexture("custom.perlinNoiseTex"), final_));
        assertEquals(new SamplerSpec(false, false, true), of(new ResourceRef.CustomTexture("custom.blueNoiseTex"), final_));
        assertEquals(new SamplerSpec(true, false, false), of(new ResourceRef.CustomTexture("custom.tonyMcMapfaceTex"), final_));
        assertEquals(SamplerSpec.NEAREST_CLAMP, of(new ResourceRef.CustomTexture("custom.missing"), final_));
    }

    @Test
    void customTextureIdsMatchSbUniforms() {
        List<String> ids = DIM.targets().customTextures().stream().map(CustomTextureIds::id).toList();
        assertEquals(List.of("custom.perlinNoiseTex", "custom.blueNoiseTex", "custom.tonyMcMapfaceTex", "custom.causticsTex"), ids);
        CustomTexture raw = new CustomTexture("colortex6", "deferred", DIM.targets().customTextures().get(2).source(), true, true);
        assertEquals("deferred.colortex6.3d", CustomTextureIds.id(raw));
        // Every custom texture the binding table refers to is declared.
        DIM.bindings().entries().stream().filter(e -> e.resource() instanceof ResourceRef.CustomTexture)
            .forEach(e -> assertEquals(1, ids.stream().filter(id -> id.equals(((ResourceRef.CustomTexture) e.resource()).id())).count(), e.name()));
    }

    @Test
    void integerTargetsAreReadAtTheirBaseLevelWithNearestFiltering() {
        Program composite90 = GLIMMER.program("world0/composite90", "fullscreen");
        TargetSpec integer = new TargetSpec(0, false, GpuFormat.RGBA16_UINT, 64, 64, TargetPlanner.fullMipChain(64, 64), true, COLOR.get(0).clearColor());
        SamplerChoice.Targets targets = new SamplerChoice.Targets() {
            @Override
            public Optional<TargetSpec> color(int index) {
                return Optional.of(integer);
            }

            @Override
            public Optional<TargetSpec> shadowColor(int index) {
                return Optional.of(integer);
            }
        };
        assertEquals(SamplerSpec.NEAREST_CLAMP, SamplerChoice.of(new ResourceRef.ColorTex(0), DIM, composite90, targets),
            "their mipmaps are not generated, even when the program asks for them");
        assertEquals(SamplerSpec.NEAREST_CLAMP, SamplerChoice.of(new ResourceRef.ShadowColor(0), DIM, composite90, targets));
    }
}
