package dev.shaderbridge.render.frame;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.DepthMode;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;

/**
 * Decoding of the depth texel at the centre of Minecraft's depth buffer into the depth a pack
 * expects for {@code centerDepthSmooth}: GL window depth in [0, 1], 0 at the near plane, as Iris'
 * {@code CenterDepthSampler} samples {@code depthtex0}. The packs ShaderBridge renders share
 * Minecraft's reversed-Z depth, which stores 1 at the near plane.
 */
public final class CenterDepth {
    private CenterDepth() {
    }

    /**
     * @param format a depth format
     * @return whether a texel of it can be read back: formats with a depth aspect only (a
     *     buffer copy of a combined depth/stencil format must pick one aspect, which the
     *     backend's copy does not)
     */
    public static boolean readable(GpuFormat format) {
        return format == GpuFormat.D32_FLOAT || format == GpuFormat.D16_UNORM;
    }

    /**
     * @param texel     the texel as copied into a buffer (little-endian), at position 0
     * @param format    the depth format
     * @param depthMode the depth convention of the pack (and of the buffer it shares)
     * @return the GL window depth, clamped to [0, 1], or NaN for an unreadable format or value
     */
    public static float decode(ByteBuffer texel, GpuFormat format, DepthMode depthMode) {
        ByteBuffer le = texel.duplicate().order(ByteOrder.LITTLE_ENDIAN);
        float stored = switch (format) {
            case D32_FLOAT -> le.getFloat(0);
            case D16_UNORM -> (le.getShort(0) & 0xFFFF) / 65535.0f;
            default -> Float.NaN;
        };
        if (!Float.isFinite(stored)) {
            return Float.NaN;
        }
        float window = depthMode == DepthMode.REVERSED_ZERO_TO_ONE ? 1.0f - stored : stored;
        return Math.clamp(window, 0.0f, 1.0f);
    }
}
