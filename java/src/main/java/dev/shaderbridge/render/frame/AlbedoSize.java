package dev.shaderbridge.render.frame;

import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import java.util.Optional;
import java.util.function.Predicate;

/**
 * The size of the texture a draw binds as its albedo ({@code Sampler0}), for the
 * {@code gtextureSize} and {@code atlasSize} uniforms: Iris reports the bound texture's size as
 * {@code gtextureSize}, and the same size as {@code atlasSize} when that texture is a texture
 * atlas (zero otherwise).
 *
 * @param width  width of the base level, 0 without a texture
 * @param height height of the base level, 0 without a texture
 * @param atlas  the texture is one of Minecraft's texture atlases
 */
public record AlbedoSize(int width, int height, boolean atlas) {
    /** No albedo texture (fullscreen passes, Distant Horizons LODs). */
    public static final AlbedoSize NONE = new AlbedoSize(0, 0, false);

    /**
     * @param albedo  the view bound as {@code Sampler0}, if any
     * @param isAtlas whether a texture is a texture atlas
     * @return its size
     */
    public static AlbedoSize of(Optional<GpuTextureView> albedo, Predicate<GpuTexture> isAtlas) {
        return albedo.map(v -> new AlbedoSize(v.getWidth(0), v.getHeight(0), isAtlas.test(v.texture()))).orElse(NONE);
    }
}
