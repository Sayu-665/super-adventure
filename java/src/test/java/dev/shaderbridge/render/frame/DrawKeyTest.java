package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.model.AlphaFunc;
import dev.shaderbridge.model.AlphaTest;
import dev.shaderbridge.model.BlendFactor;
import dev.shaderbridge.model.BlendMode;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.uniforms.DrawState;
import dev.shaderbridge.uniforms.FrameState;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import org.joml.Matrix4f;
import org.junit.jupiter.api.Test;

/** {@link DrawKey}, {@link DrawSlots} and {@link RenderStages}: the per-draw block values and their slots. */
class DrawKeyTest {
    private static final Program TERRAIN = RenderFixture.load(RenderFixture.TUTORIAL4).dim().programFor(GeometryProgram.TERRAIN_SOLID).orElseThrow();

    private static Program with(BlendMode blend, AlphaTest alphaTest) {
        Program p = TERRAIN;
        return new Program(p.name(), p.kind(), p.drawProfile(), p.requiresRawVulkan(), p.stages(), p.drawBuffers(), p.outputSlots(), p.outputTypes(),
            blend, p.blendPerBuffer(), alphaTest, p.viewport(), p.mipmapTargets(), p.bindingsUsed(), p.vertexInputs(), p.pushConstantSize(),
            p.compute(), p.cull(), p.synthesizedFrom());
    }

    private static FrameState frame() {
        FrameState f = new FrameState();
        f.viewRotation.rotateY(0.5f);
        f.projection.setPerspective(1.2f, 1.6f, 0.05f, 512f, true);
        f.sunAngleAttribute = 30;
        f.update();
        return f;
    }

    @Test
    void keysCarryTheProgramsBlendAndAlphaTestAsGlValues() {
        DrawKey key = DrawKey.of("p", with(new BlendMode(BlendFactor.SRC_ALPHA, BlendFactor.ONE_MINUS_SRC_ALPHA, BlendFactor.ONE, BlendFactor.ZERO),
            new AlphaTest(AlphaFunc.GREATER, 0.1f)), RenderStages.TERRAIN_SOLID, false, AlbedoSize.NONE);
        assertEquals(List.of(0x0302, 0x0303, 1, 0), key.blendFunc());
        assertEquals(0.1f, key.alphaTestRef());
        DrawState draw = new DrawState();
        FrameState frame = frame();
        key.apply(frame, draw);
        assertEquals(RenderStages.TERRAIN_SOLID, draw.renderStage);
        assertArrayEquals(new int[] {0x0302, 0x0303, 1, 0}, draw.blendFunc);
        assertEquals(new Matrix4f(frame.gbufferModelView()), draw.modelViewMatrix);
        DrawKey.of("p", with(null, null), RenderStages.TERRAIN_SOLID, true, AlbedoSize.NONE).apply(frame, draw);
        assertEquals(new Matrix4f(frame.shadowModelView()), draw.modelViewMatrix);
        assertEquals(new Matrix4f(frame.shadowProjection()), draw.projectionMatrix);
        assertArrayEquals(new int[4], draw.blendFunc);
        assertEquals(0, draw.alphaTestRef);
    }

    @Test
    void slotsOfNewKindsAreWrittenWhenFirstDrawnInAFrame() {
        List<DrawState> written = new ArrayList<>();
        List<GpuBufferSlice> slices = new ArrayList<>();
        DrawSlots.BlockWriter writer = (frame, draw) -> {
            DrawState copy = new DrawState();
            copy.renderStage = draw.renderStage;
            copy.modelViewMatrix.set(draw.modelViewMatrix);
            written.add(copy);
            GpuBufferSlice slice = new GpuBufferSlice(null, slices.size() * 256L, 256);
            slices.add(slice);
            return slice;
        };
        DrawSlots slots = new DrawSlots();
        DrawKey water = DrawKey.of("water", TERRAIN, RenderStages.TERRAIN_TRANSLUCENT, false, AlbedoSize.NONE);
        DrawKey shadow = DrawKey.of("shadow", TERRAIN, RenderStages.TERRAIN_SOLID, true, AlbedoSize.NONE);
        assertThrows(IllegalStateException.class, () -> slots.slice(water));
        FrameState frame = frame();
        slots.prepare(frame, writer);
        assertEquals(2, written.size(), "the camera and shadow defaults");
        GpuBufferSlice waterSlice = slots.slice(water);
        assertEquals(3, written.size(), "a new kind gets its block at once, in the frame it is first drawn");
        assertSame(slices.get(2), waterSlice);
        assertEquals(RenderStages.TERRAIN_TRANSLUCENT, written.get(2).renderStage);
        assertSame(waterSlice, slots.slice(water), "later draws of the kind reuse the block");
        GpuBufferSlice shadowSlice = slots.slice(shadow);
        assertSame(slices.get(3), shadowSlice);
        assertEquals(new Matrix4f(frame.shadowModelView()), written.get(3).modelViewMatrix);
        assertEquals(4, written.size());
        slots.prepare(frame, writer);
        assertEquals(6, written.size(), "a new frame writes the defaults again and nothing else until a kind is drawn");
        assertNotSame(waterSlice, slots.slice(water), "each frame writes its own blocks");
        assertEquals(7, written.size());
    }

    @Test
    void renderStagesFollowIrisPhases() {
        assertEquals(1, RenderStages.of(GeometryProgram.SKY_BASIC));
        assertEquals(8, RenderStages.of(GeometryProgram.TERRAIN_SOLID));
        assertEquals(10, RenderStages.of(GeometryProgram.SHADOW_CUTOUT));
        assertEquals(17, RenderStages.of(GeometryProgram.WATER));
        assertEquals(11, RenderStages.of(GeometryProgram.ENTITIES_TRANSLUCENT));
        assertEquals(21, RenderStages.of(GeometryProgram.WEATHER));
        for (GeometryProgram p : GeometryProgram.values()) {
            RenderStages.of(p);
        }
    }

    @Test
    void theAlbedoSetsGtextureSizeAndAtlasSizeOnlyForAtlases() {
        GpuTexture atlas = texture(1024, 512);
        GpuTexture skin = texture(64, 32);
        AlbedoSize atlasSize = AlbedoSize.of(Optional.of(view(atlas)), t -> t == atlas);
        AlbedoSize skinSize = AlbedoSize.of(Optional.of(view(skin)), t -> t == atlas);
        assertEquals(new AlbedoSize(1024, 512, true), atlasSize);
        assertEquals(new AlbedoSize(64, 32, false), skinSize);
        assertEquals(AlbedoSize.NONE, AlbedoSize.of(Optional.empty(), t -> true));
        DrawState draw = new DrawState();
        FrameState frame = frame();
        DrawKey.of("p", TERRAIN, RenderStages.TERRAIN_SOLID, false, atlasSize).apply(frame, draw);
        assertArrayEquals(new int[] {1024, 512, 1024, 512}, new int[] {draw.gtextureWidth, draw.gtextureHeight, draw.atlasWidth, draw.atlasHeight});
        DrawKey.of("p", TERRAIN, RenderStages.ENTITIES, false, skinSize).apply(frame, draw);
        assertArrayEquals(new int[] {64, 32, 0, 0}, new int[] {draw.gtextureWidth, draw.gtextureHeight, draw.atlasWidth, draw.atlasHeight});
        assertNotEquals(DrawKey.of("p", TERRAIN, RenderStages.ENTITIES, false, skinSize), DrawKey.of("p", TERRAIN, RenderStages.ENTITIES, false,
            AlbedoSize.NONE), "draws with other albedo sizes get blocks of their own");
    }

    private static GpuTexture texture(int width, int height) {
        return (GpuTexture) Proxy.newProxyInstance(GpuTexture.class.getClassLoader(), new Class<?>[] {GpuTexture.class}, (p, m, a) -> switch (m.getName()) {
            case "getWidth" -> width >> (int) a[0];
            case "getHeight" -> height >> (int) a[0];
            case "hashCode" -> System.identityHashCode(p);
            case "equals" -> p == a[0];
            default -> throw new UnsupportedOperationException(m.getName());
        });
    }

    private static GpuTextureView view(GpuTexture texture) {
        return (GpuTextureView) Proxy.newProxyInstance(GpuTextureView.class.getClassLoader(), new Class<?>[] {GpuTextureView.class},
            (p, m, a) -> switch (m.getName()) {
                case "texture" -> texture;
                case "getWidth" -> texture.getWidth((int) a[0]);
                case "getHeight" -> texture.getHeight((int) a[0]);
                default -> throw new UnsupportedOperationException(m.getName());
            });
    }
}
