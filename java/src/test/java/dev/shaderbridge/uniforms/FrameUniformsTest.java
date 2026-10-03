package dev.shaderbridge.uniforms;

import static org.junit.jupiter.api.Assertions.assertEquals;

import dev.shaderbridge.model.BlockLayout;
import dev.shaderbridge.model.BlockMember;
import dev.shaderbridge.model.GlslType;
import dev.shaderbridge.model.UniformSource;
import dev.shaderbridge.model.CompiledPack;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.ModelJson;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.List;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** Fills the sb_Frame and sb_Draw layouts of the model sample. */
class FrameUniformsTest {
    static DimensionPipeline pipeline;

    @BeforeAll
    static void load() throws Exception {
        try (InputStream in = FrameUniformsTest.class.getResourceAsStream("/dev/shaderbridge/model/compiled_pack_sample.json")) {
            CompiledPack pack = ModelJson.parse(new String(in.readAllBytes(), StandardCharsets.UTF_8), CompiledPack.class);
            pipeline = pack.dimension("world0").orElseThrow();
        }
    }

    @Test
    void fillsBuiltinsDefaultsAndLeavesCustomsToTheEvaluator() {
        FrameState frame = new FrameState();
        frame.configure(UniformSettings.of(pipeline));
        frame.cameraPosition.set(1, 2, 3);
        frame.hideGui = true;
        frame.update();
        try (FrameUniforms uniforms = new FrameUniforms(pipeline.uniforms().frame(), null)) {
            ByteBuffer block = uniforms.fill(frame, 0.016f);
            assertEquals(112, block.remaining());
            assertEquals(ByteOrder.LITTLE_ENDIAN, block.order());
            assertEquals(1.0f, block.getFloat(0), "gbufferModelView is the identity");
            assertEquals(1.0f, block.getFloat(64));
            assertEquals(3.0f, block.getFloat(72));
            assertEquals(0.0f, block.getFloat(80), "screenDark is custom; without an evaluator it stays 0");
            assertEquals(1, block.getInt(84), "hideGUI__int: bool builtin written as int");
            assertEquals(0.25f, block.getFloat(88), "packTint keeps its initializer");
            assertEquals(0.75f, block.getFloat(92));
            assertEquals(0.0f, block.getFloat(96), "weights is unset");
        }
    }

    @Test
    void drawBlocksKeepConstantInitializers() {
        BlockLayout layout = new BlockLayout("sb_Draw", 0, 1, 32, List.of(
            new BlockMember("entityId", GlslType.INT, 0, new UniformSource.Builtin("entityId"), null),
            new BlockMember("tint", GlslType.VEC4, 16, new UniformSource.Unset(), List.of(0.5f, 0.25f, 1f, 1f))));
        FrameState frame = new FrameState();
        frame.update();
        DrawState draw = new DrawState();
        draw.reset(frame);
        draw.entityId = 3;
        try (DrawUniforms uniforms = new DrawUniforms(layout, 64)) {
            ByteBuffer block = ByteBuffer.allocate(32).order(ByteOrder.LITTLE_ENDIAN);
            block.putFloat(16, 9f);
            uniforms.fill(frame, draw, block);
            assertEquals(3, block.getInt(0));
            assertEquals(0.5f, block.getFloat(16), "every draw starts from the initializers, not from zero or stale data");
            assertEquals(0.25f, block.getFloat(20));
        }
    }

    @Test
    void drawBlocksUseTheDrawState() {
        FrameState frame = new FrameState();
        frame.update();
        DrawState draw = new DrawState();
        draw.reset(frame);
        draw.entityId = 7;
        draw.alphaTestRef = 0.1f;
        draw.entityColor.set(1, 0, 0, 0.5f);
        BlockLayout layout = pipeline.uniforms().draw();
        try (DrawUniforms uniforms = new DrawUniforms(layout, 256)) {
            ByteBuffer block = ByteBuffer.allocate(layout.size()).order(ByteOrder.LITTLE_ENDIAN);
            uniforms.fill(frame, draw, block);
            assertEquals(1.0f, block.getFloat(0));
            assertEquals(0.5f, block.getFloat(12));
            assertEquals(7, block.getInt(16));
            assertEquals(0.1f, block.getFloat(20));
        }
    }
}
