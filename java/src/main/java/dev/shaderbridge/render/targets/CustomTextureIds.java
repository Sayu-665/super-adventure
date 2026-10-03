package dev.shaderbridge.render.targets;

import dev.shaderbridge.model.CustomTexture;
import dev.shaderbridge.model.TextureSource;

/**
 * The ids {@code ResourceRef.CustomTexture} carries for custom textures (sb-uniforms
 * {@code custom_texture_id} / {@code raw_texture_id}): {@code <stage>.<sampler>} for image
 * textures and every {@code customTexture.<name>} (stage {@value #CUSTOM_STAGE}), and
 * {@code <stage>.<sampler>.<dim>} for raw {@code texture.<stage>.<sampler>} entries, whose
 * dimension ({@code 1d}, {@code 2d}, {@code 3d}, {@code 2d_rect}) tells same-named entries apart.
 */
public final class CustomTextureIds {
    /** Stage of {@code customTexture.<name>} entries. */
    public static final String CUSTOM_STAGE = "custom";

    private CustomTextureIds() {
    }

    /**
     * @param texture a custom texture
     * @return the id bindings refer to it by
     */
    public static String id(CustomTexture texture) {
        String base = texture.stage() + "." + texture.sampler();
        if (texture.source() instanceof TextureSource.Raw raw && !CUSTOM_STAGE.equals(texture.stage())) {
            return base + "." + raw.target();
        }
        return base;
    }

    /**
     * @param texture a custom texture
     * @param id      a {@code ResourceRef.CustomTexture} id
     * @return whether the id refers to the texture
     */
    public static boolean matches(CustomTexture texture, String id) {
        return id(texture).equals(id);
    }
}
