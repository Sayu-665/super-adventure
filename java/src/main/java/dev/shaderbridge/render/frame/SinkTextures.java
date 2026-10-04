package dev.shaderbridge.render.frame;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import java.util.HashMap;
import java.util.Map;

/**
 * Throwaway render targets for slot 0 of a pass whose first output has no texture: Mojang's render
 * passes take their size from slot 0 and need a texture there. One per format and size, created on
 * first use. Render thread only.
 */
final class SinkTextures implements AutoCloseable {
    private record Key(GpuFormat format, int width, int height) {
    }

    private record Sink(GpuTexture texture, GpuTextureView view) {
    }

    private final GpuDevice device;
    private final Map<Key, Sink> sinks = new HashMap<>();

    SinkTextures(GpuDevice device) {
        this.device = device;
    }

    /**
     * @param format the attachment format the pipeline expects
     * @param width  the pass width
     * @param height the pass height
     * @return a view of a texture of that format and size
     */
    GpuTextureView view(GpuFormat format, int width, int height) {
        return sinks.computeIfAbsent(new Key(format, width, height), k -> {
            GpuTexture texture = device.createTexture("ShaderBridge sink " + format, GpuTexture.USAGE_RENDER_ATTACHMENT, format, width, height, 1, 1);
            return new Sink(texture, device.createTextureView(texture));
        }).view();
    }

    @Override
    public void close() {
        for (Sink sink : sinks.values()) {
            sink.view().close();
            sink.texture().close();
        }
        sinks.clear();
    }
}
