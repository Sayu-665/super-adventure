package dev.shaderbridge.model;

import java.util.List;

/**
 * Render target configuration from const directives and {@code shaders.properties}.
 *
 * @param colortex                {@code colortex0..N}
 * @param shadowcolor             {@code shadowcolor0..7}
 * @param shadow                  shadow map settings
 * @param usesDepthtex1           a program samples {@code depthtex1}
 * @param usesDepthtex2           a program samples {@code depthtex2}
 * @param noiseTextureResolution  {@code noiseTextureResolution}
 * @param noiseTexture            {@code texture.noise} override, or null
 * @param customTextures          custom textures
 * @param images                  custom images
 * @param buffers                 shader storage buffers
 */
public record RenderTargets(
    List<ColorTarget> colortex,
    List<ColorTarget> shadowcolor,
    ShadowSettings shadow,
    boolean usesDepthtex1,
    boolean usesDepthtex2,
    int noiseTextureResolution,
    TextureSource noiseTexture,
    List<CustomTexture> customTextures,
    List<CustomImage> images,
    List<StorageBuffer> buffers
) {
    public RenderTargets {
        colortex = Copies.list(colortex);
        shadowcolor = Copies.list(shadowcolor);
        Copies.required(shadow, "shadow");
        customTextures = Copies.list(customTextures);
        images = Copies.list(images);
        buffers = Copies.list(buffers);
    }
}
