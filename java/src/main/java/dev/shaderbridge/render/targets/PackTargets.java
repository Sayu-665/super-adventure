package dev.shaderbridge.render.targets;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.render.pipeline.DepthStates;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;
import java.util.function.ToIntFunction;

/**
 * The render targets of a dimension pipeline: {@code colortexN} and {@code shadowcolorN} as
 * main/alt {@link ColorPair}s, the {@code depthtex1}/{@code depthtex2} copies of Minecraft's main
 * depth ({@code depthtex0} is the main depth itself), and the shadow maps {@code shadowtex0} (the
 * shadow pass depth attachment) and {@code shadowtex1} (its copy before translucent casters).
 * Screen-sized targets follow the window through {@link #resize}. Render thread only.
 */
public final class PackTargets implements AutoCloseable {
    /** Usage of the depth copies and the shadow maps. */
    static final int DEPTH_USAGE = GpuTexture.USAGE_RENDER_ATTACHMENT | GpuTexture.USAGE_TEXTURE_BINDING | GpuTexture.USAGE_COPY_SRC
        | GpuTexture.USAGE_COPY_DST;
    /** Format of the shadow maps. */
    public static final GpuFormat SHADOW_DEPTH_FORMAT = GpuFormat.D32_FLOAT;

    private final GpuDevice device;
    private final DimensionPipeline dim;
    private final GpuFormat depthFormat;
    private final ToIntFunction<GpuFormat> maxSize;
    private final Map<Integer, ColorPair> color = new TreeMap<>();
    private final Map<Integer, ColorPair> shadowColor = new TreeMap<>();
    private final DepthTexture[] depthCopies = new DepthTexture[2];
    private final DepthTexture[] shadowDepth = new DepthTexture[2];
    private int width;
    private int height;
    /** The screen-sized targets were cleared since they were created. */
    private boolean screenCleared;
    /** The shadow targets were cleared since they were created. */
    private boolean shadowCleared;

    /** A depth texture with its sampling view. */
    private record DepthTexture(GpuTexture texture, GpuTextureView view) implements AutoCloseable {
        static DepthTexture create(GpuDevice device, String label, GpuFormat format, int width, int height) {
            GpuTexture texture = device.createTexture(label, DEPTH_USAGE, format, width, height, 1, 1);
            return new DepthTexture(texture, device.createTextureView(texture));
        }

        @Override
        public void close() {
            view.close();
            texture.close();
        }
    }

    /**
     * Creates every target, clamped to the device's texture size limits.
     *
     * @param device      the GPU device
     * @param dim         the dimension pipeline
     * @param width       screen width in pixels
     * @param height      screen height in pixels
     * @param depthFormat format of Minecraft's main depth texture (the depth copies use it too)
     * @return the targets
     */
    public static PackTargets create(GpuDevice device, DimensionPipeline dim, int width, int height, GpuFormat depthFormat) {
        return new PackTargets(device, dim, width, height, depthFormat, device.getDeviceInfo().limits()::maxTextureSizeForFormat);
    }

    /**
     * Creates every target.
     *
     * @param device      the GPU device
     * @param dim         the dimension pipeline
     * @param width       screen width in pixels
     * @param height      screen height in pixels
     * @param depthFormat format of Minecraft's main depth texture (the depth copies use it too)
     * @param maxSize     largest texture extent per format
     */
    PackTargets(GpuDevice device, DimensionPipeline dim, int width, int height, GpuFormat depthFormat, ToIntFunction<GpuFormat> maxSize) {
        this.device = device;
        this.dim = dim;
        this.depthFormat = depthFormat;
        this.maxSize = maxSize;
        createScreenTargets(width, height);
        for (TargetSpec spec : TargetPlanner.shadowColorTargets(dim, maxSize)) {
            shadowColor.put(spec.index(), new ColorPair(device, spec));
        }
        int resolution = TargetPlanner.shadowResolution(dim.targets().shadow());
        for (int k = 0; k < 2; k++) {
            shadowDepth[k] = DepthTexture.create(device, "ShaderBridge shadowtex" + k, SHADOW_DEPTH_FORMAT, resolution, resolution);
        }
    }

    private void createScreenTargets(int width, int height) {
        this.width = width;
        this.height = height;
        for (TargetSpec spec : TargetPlanner.colorTargets(dim, width, height, maxSize)) {
            color.put(spec.index(), new ColorPair(device, spec));
        }
        for (int k = 0; k < 2; k++) {
            depthCopies[k] = DepthTexture.create(device, "ShaderBridge depthtex" + (k + 1), depthFormat, width, height);
        }
        screenCleared = false;
    }

    /**
     * Recreates the screen-sized targets when the window size changed. Their contents are lost;
     * the next {@link #clear} clears every target as on the first frame.
     *
     * @param newWidth  screen width in pixels
     * @param newHeight screen height in pixels
     * @return whether the targets were recreated (views obtained before are closed)
     */
    public boolean resize(int newWidth, int newHeight) {
        if (newWidth == width && newHeight == height) {
            return false;
        }
        closeScreenTargets();
        createScreenTargets(newWidth, newHeight);
        return true;
    }

    /**
     * Clears the targets at the start of a frame, as the headless executor does: the color
     * targets the pack clears (and every target on its first frame, after creation or a resize;
     * main and alt alike) to their clear color, and the shadow map ({@code shadowtex0}, the
     * shadow pass's attachment) to the far value. The depth copies ({@code depthtex1},
     * {@code depthtex2}, {@code shadowtex1}) are cleared only on their first frame: as in Iris,
     * programs that run before this frame's copy (begin, shadow, prepare, opaque gbuffers) see the
     * previous frame's depth.
     *
     * @param encoder   the command encoder
     * @param fog       the current fog color (default clear of {@code colortex0})
     * @param depthMode the pack's depth convention
     */
    public void clear(CommandEncoder encoder, Rgba fog, DepthMode depthMode) {
        for (ColorPair pair : color.values()) {
            clear(encoder, pair, fog, screenCleared);
        }
        for (ColorPair pair : shadowColor.values()) {
            clear(encoder, pair, fog, shadowCleared);
        }
        double far = DepthStates.clearValue(depthMode);
        if (!screenCleared) {
            for (DepthTexture d : depthCopies) {
                encoder.clearDepthTexture(d.texture(), far);
            }
        }
        encoder.clearDepthTexture(shadowDepth[0].texture(), far);
        if (!shadowCleared) {
            encoder.clearDepthTexture(shadowDepth[1].texture(), far);
        }
        screenCleared = true;
        shadowCleared = true;
    }

    private static void clear(CommandEncoder encoder, ColorPair pair, Rgba fog, boolean clearedBefore) {
        if (pair.spec().clear() || !clearedBefore) {
            Rgba c = ClearColors.of(pair.spec(), fog);
            encoder.clearColorTexture(pair.texture(false), c.toVector());
            encoder.clearColorTexture(pair.texture(true), c.toVector());
        }
    }

    /**
     * Copies Minecraft's main depth into {@code depthtex1} (after the opaque geometry) or
     * {@code depthtex2} (before the hand).
     *
     * @param encoder   the command encoder
     * @param mainDepth Minecraft's main depth texture ({@code depthtex0})
     * @param index     1 or 2
     */
    public void copyMainDepth(CommandEncoder encoder, GpuTexture mainDepth, int index) {
        GpuTexture target = depthCopy(index).texture();
        encoder.copyTextureToTexture(mainDepth, target, 0, 0, 0, 0, 0, Math.min(width, mainDepth.getWidth(0)), Math.min(height, mainDepth.getHeight(0)));
    }

    /**
     * Copies {@code shadowtex0} into {@code shadowtex1}, after the opaque shadow casters.
     *
     * @param encoder the command encoder
     */
    public void copyShadowDepth(CommandEncoder encoder) {
        GpuTexture source = shadowDepth[0].texture();
        encoder.copyTextureToTexture(source, shadowDepth[1].texture(), 0, 0, 0, 0, 0, source.getWidth(0), source.getHeight(0));
    }

    /**
     * Copies a color target's alternate texture over its main one (the end-of-frame copies of
     * buffers flipped an odd number of times).
     *
     * @param encoder the command encoder
     * @param index   a colortex index
     */
    public void copyAltToMain(CommandEncoder encoder, int index) {
        ColorPair pair = color.get(index);
        if (pair != null) {
            for (int level = 0; level < pair.spec().mipLevels(); level++) {
                GpuTexture alt = pair.texture(true);
                encoder.copyTextureToTexture(alt, pair.texture(false), level, 0, 0, 0, 0, alt.getWidth(level), alt.getHeight(level));
            }
        }
    }

    /**
     * @param index a colortex index
     * @return its textures, if the pack uses it
     */
    public Optional<ColorPair> color(int index) {
        return Optional.ofNullable(color.get(index));
    }

    /**
     * @param index a shadowcolor index
     * @return its textures, if the pack uses it
     */
    public Optional<ColorPair> shadowColor(int index) {
        return Optional.ofNullable(shadowColor.get(index));
    }

    /** @return every colortex target, by index */
    public List<ColorPair> colorTargets() {
        return List.copyOf(color.values());
    }

    /** @return every shadowcolor target, by index */
    public List<ColorPair> shadowColorTargets() {
        return List.copyOf(shadowColor.values());
    }

    /**
     * @param index 1 ({@code depthtex1}) or 2 ({@code depthtex2})
     * @return the depth copy's sampling view
     */
    public GpuTextureView depthCopyView(int index) {
        return depthCopy(index).view();
    }

    private DepthTexture depthCopy(int index) {
        if (index != 1 && index != 2) {
            throw new IllegalArgumentException("depthtex" + index + " is not a copy");
        }
        return depthCopies[index - 1];
    }

    /**
     * @param index 0 ({@code shadowtex0}, the shadow pass attachment) or 1
     * @return the shadow map's texture
     */
    public GpuTexture shadowDepth(int index) {
        return shadowDepth[index].texture();
    }

    /**
     * @param index 0 or 1
     * @return the shadow map's view (sampling, and attachment of {@code shadowtex0})
     */
    public GpuTextureView shadowDepthView(int index) {
        return shadowDepth[index].view();
    }

    /** @return the screen size the screen-sized targets were created for, {@code [width, height]} */
    public int[] screenSize() {
        return new int[] {width, height};
    }

    private void closeScreenTargets() {
        color.values().forEach(ColorPair::close);
        color.clear();
        for (DepthTexture d : depthCopies) {
            d.close();
        }
    }

    @Override
    public void close() {
        closeScreenTargets();
        shadowColor.values().forEach(ColorPair::close);
        shadowColor.clear();
        for (DepthTexture d : shadowDepth) {
            d.close();
        }
    }
}
