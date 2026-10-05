package dev.shaderbridge.render.targets;

import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.api.textures.GpuTextureView;

/**
 * The main and alternate texture of a color target (Iris' ping-pong buffers), each with a view of
 * every mip level for sampling and a view of the base level for rendering (views of single mip
 * levels, for generating mipmaps, are created on demand). Which of the two a pass reads is the
 * pass's flip state ({@code Pass.flip_state}, {@code BindingUse.use_alt}).
 */
public final class ColorPair implements AutoCloseable {
    /** Usage of every color target: attachment, sampled, copy source and destination. */
    public static final int USAGE = GpuTexture.USAGE_RENDER_ATTACHMENT | GpuTexture.USAGE_TEXTURE_BINDING | GpuTexture.USAGE_COPY_SRC
        | GpuTexture.USAGE_COPY_DST;

    private final GpuDevice device;
    private final TargetSpec spec;
    private final GpuTexture[] textures = new GpuTexture[2];
    private final GpuTextureView[] sampleViews = new GpuTextureView[2];
    private final GpuTextureView[] attachmentViews = new GpuTextureView[2];
    /** Single-level views by texture and level (level 0 is the attachment view). */
    private final GpuTextureView[][] levelViews;

    /**
     * Creates both textures.
     *
     * @param device the GPU device
     * @param spec   the target
     */
    public ColorPair(GpuDevice device, TargetSpec spec) {
        this.device = device;
        this.spec = spec;
        this.levelViews = new GpuTextureView[2][spec.mipLevels()];
        for (int k = 0; k < 2; k++) {
            textures[k] = device.createTexture(label(spec.name(), k == 1), USAGE, spec.format(), spec.width(), spec.height(), 1, spec.mipLevels());
            sampleViews[k] = device.createTextureView(textures[k]);
            attachmentViews[k] = spec.mipLevels() == 1 ? sampleViews[k] : device.createTextureView(textures[k], 0, 1);
        }
    }

    /**
     * @param name the target's {@linkplain TargetSpec#name() name}
     * @param alt  the alternate texture
     * @return the label the texture is created with
     */
    public static String label(String name, boolean alt) {
        return "ShaderBridge " + name + (alt ? " (alt)" : "");
    }

    /** @return what the pair was created for */
    public TargetSpec spec() {
        return spec;
    }

    /**
     * @param alt the alternate texture
     * @return the texture
     */
    public GpuTexture texture(boolean alt) {
        return textures[alt ? 1 : 0];
    }

    /**
     * @param alt the alternate texture
     * @return a view of every mip level, for sampling
     */
    public GpuTextureView sampleView(boolean alt) {
        return sampleViews[alt ? 1 : 0];
    }

    /**
     * @param alt the alternate texture
     * @return a view of the base level, for render pass attachments
     */
    public GpuTextureView attachmentView(boolean alt) {
        return attachmentViews[alt ? 1 : 0];
    }

    /**
     * @param alt   the alternate texture
     * @param level a mip level
     * @return a view of that level alone (to render into it, or to sample it while rendering the
     *     next one)
     */
    public GpuTextureView levelView(boolean alt, int level) {
        if (level == 0) {
            return attachmentView(alt);
        }
        GpuTextureView[] views = levelViews[alt ? 1 : 0];
        if (views[level] == null) {
            views[level] = device.createTextureView(texture(alt), level, 1);
        }
        return views[level];
    }

    @Override
    public void close() {
        for (int k = 0; k < 2; k++) {
            for (int level = 1; level < levelViews[k].length; level++) {
                if (levelViews[k][level] != null) {
                    levelViews[k][level].close();
                }
            }
            if (attachmentViews[k] != sampleViews[k]) {
                attachmentViews[k].close();
            }
            sampleViews[k].close();
            textures[k].close();
        }
    }
}
