package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.DepthMode;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import org.junit.jupiter.api.Test;

/** {@link CenterDepth}: the read-back centre texel as the GL window depth packs expect for {@code centerDepthSmooth}. */
class CenterDepthTest {
    private static ByteBuffer d32(float value) {
        return ByteBuffer.allocate(8).order(ByteOrder.LITTLE_ENDIAN).putFloat(0, value);
    }

    @Test
    void reversedDepthIsFlippedToGlWindowDepth() {
        assertEquals(0.0f, CenterDepth.decode(d32(1.0f), GpuFormat.D32_FLOAT, DepthMode.REVERSED_ZERO_TO_ONE), "the near plane");
        assertEquals(1.0f, CenterDepth.decode(d32(0.0f), GpuFormat.D32_FLOAT, DepthMode.REVERSED_ZERO_TO_ONE), "the sky (cleared to 0)");
        assertEquals(0.75f, CenterDepth.decode(d32(0.25f), GpuFormat.D32_FLOAT, DepthMode.REVERSED_ZERO_TO_ONE), 1e-6f);
    }

    @Test
    void forwardDepthIsAlreadyWindowDepth() {
        assertEquals(0.25f, CenterDepth.decode(d32(0.25f), GpuFormat.D32_FLOAT, DepthMode.FORWARD_ZERO_TO_ONE), 1e-6f);
        assertEquals(0.25f, CenterDepth.decode(d32(0.25f), GpuFormat.D32_FLOAT, DepthMode.GL_NEG_ONE_TO_ONE), 1e-6f);
    }

    @Test
    void unormDepthAndBadValues() {
        ByteBuffer d16 = ByteBuffer.allocate(8).order(ByteOrder.LITTLE_ENDIAN).putShort(0, (short) 0xFFFF);
        assertEquals(1.0f, CenterDepth.decode(d16, GpuFormat.D16_UNORM, DepthMode.FORWARD_ZERO_TO_ONE));
        assertEquals(0.0f, CenterDepth.decode(d16, GpuFormat.D16_UNORM, DepthMode.REVERSED_ZERO_TO_ONE));
        assertTrue(Float.isNaN(CenterDepth.decode(d32(Float.NaN), GpuFormat.D32_FLOAT, DepthMode.REVERSED_ZERO_TO_ONE)));
        assertEquals(0.0f, CenterDepth.decode(d32(1.5f), GpuFormat.D32_FLOAT, DepthMode.REVERSED_ZERO_TO_ONE), "clamped");
        assertTrue(Float.isNaN(CenterDepth.decode(d32(0.5f), GpuFormat.D24_UNORM_S8_UINT, DepthMode.REVERSED_ZERO_TO_ONE)));
    }

    @Test
    void onlyDepthOnlyFormatsAreRead() {
        assertTrue(CenterDepth.readable(GpuFormat.D32_FLOAT));
        assertTrue(CenterDepth.readable(GpuFormat.D16_UNORM));
        assertFalse(CenterDepth.readable(GpuFormat.D32_FLOAT_S8_UINT), "a buffer copy must pick one aspect");
        assertFalse(CenterDepth.readable(GpuFormat.D24_UNORM_S8_UINT));
        assertFalse(CenterDepth.readable(GpuFormat.RGBA8_UNORM));
    }
}
