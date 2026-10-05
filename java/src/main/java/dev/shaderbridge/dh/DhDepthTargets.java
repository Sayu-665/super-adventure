package dev.shaderbridge.dh;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.api.textures.GpuTextureView;

/**
 * The depth textures of {@link DhMode#NATIVE} LODs: the depth attachment the LOD passes draw
 * into, separate from Minecraft's, and the two copies packs sample: {@code dhDepthTex1} (opaque
 * LODs only, copied after {@code dh_terrain}) and {@code dhDepthTex0} (copied after
 * {@code dh_water}). All are 32-bit float depth, screen-sized, and follow the window. Render
 * thread only.
 */
public final class DhDepthTargets implements AutoCloseable {
    /** Format of the LOD depth textures. */
    public static final GpuFormat FORMAT = GpuFormat.D32_FLOAT;
    private static final int USAGE = GpuTexture.USAGE_RENDER_ATTACHMENT | GpuTexture.USAGE_TEXTURE_BINDING | GpuTexture.USAGE_COPY_SRC
        | GpuTexture.USAGE_COPY_DST;

    private final GpuDevice device;
    /** [attachment, dhDepthTex0, dhDepthTex1]. */
    private final GpuTexture[] textures = new GpuTexture[3];
    private final GpuTextureView[] views = new GpuTextureView[3];

    /** @param device the GPU device */
    public DhDepthTargets(GpuDevice device) {
        this.device = device;
    }

    /**
     * Starts a frame: (re)creates the textures at the screen size and clears the attachment to
     * the far plane (the copies keep the previous frame's LODs until they are copied again, as in
     * Iris; new textures are cleared too).
     *
     * @param encoder a command encoder (outside any render pass)
     * @param width   screen width
     * @param height  screen height
     * @param far     the depth of the far plane in the pack's depth convention
     */
    public void beginFrame(CommandEncoder encoder, int width, int height, double far) {
        if (textures[0] == null || textures[0].getWidth(0) != width || textures[0].getHeight(0) != height) {
            close();
            String[] labels = {"ShaderBridge DH depth", "ShaderBridge dhDepthTex0", "ShaderBridge dhDepthTex1"};
            for (int i = 0; i < 3; i++) {
                textures[i] = device.createTexture(labels[i], USAGE, FORMAT, width, height, 1, 1);
                views[i] = device.createTextureView(textures[i]);
                encoder.clearDepthTexture(textures[i], far);
            }
            return;
        }
        encoder.clearDepthTexture(textures[0], far);
    }

    /** @return the view of the LOD depth attachment */
    public GpuTextureView attachment() {
        return views[0];
    }

    /**
     * Copies the LOD depth into {@code dhDepthTex<index>}.
     *
     * @param encoder a command encoder (outside any render pass)
     * @param index   0 (after {@code dh_water}) or 1 (after {@code dh_terrain})
     */
    public void copy(CommandEncoder encoder, int index) {
        GpuTexture source = textures[0];
        encoder.copyTextureToTexture(source, textures[1 + index], 0, 0, 0, 0, 0, source.getWidth(0), source.getHeight(0));
    }

    /**
     * @param index 0 or 1
     * @return the sampling view of {@code dhDepthTex<index>}, or null before the first frame
     */
    public GpuTextureView sampled(int index) {
        return views[1 + index];
    }

    @Override
    public void close() {
        for (int i = 0; i < 3; i++) {
            if (views[i] != null) {
                views[i].close();
                textures[i].close();
                views[i] = null;
                textures[i] = null;
            }
        }
    }
}
