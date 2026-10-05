package dev.shaderbridge.render.targets;

import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.model.ResourceRef;
import java.util.Optional;

/**
 * Textures that belong to the game or another mod rather than to the pack, supplied by the frame
 * orchestration for the current frame and draw. Pack programs usually receive these through their
 * draw profile's host samplers ({@code Sampler0}, {@code Sampler2}, DH's {@code uLightMap});
 * this is for programs that sample them by pack name ({@code lightmap} in a composite pass,
 * {@code depthtex0}, {@code dhDepthTex0}, ...).
 */
public interface HostTextures {
    /** @return the block atlas (or the albedo texture of the current draw) */
    TextureBinding atlas();

    /** @return Minecraft's lightmap */
    TextureBinding lightmap();

    /** @return Minecraft's entity overlay texture (hurt flash, creeper swell) */
    TextureBinding overlay();

    /** @return the normal atlas of a PBR resource pack, if one is loaded */
    Optional<TextureBinding> normals();

    /** @return the specular atlas of a PBR resource pack, if one is loaded */
    Optional<TextureBinding> specular();

    /** @return Minecraft's main depth texture ({@code depthtex0}) */
    GpuTextureView mainDepth();

    /**
     * @param index 0 ({@code dhDepthTex0}) or 1 ({@code dhDepthTex1})
     * @return the Distant Horizons depth texture, if Distant Horizons renders
     */
    Optional<GpuTextureView> dhDepth(int index);

    /** @return Distant Horizons' block atlas, if Distant Horizons renders */
    Optional<TextureBinding> dhBlockAtlas();

    /**
     * A copy of a render target that the current render pass draws into, taken just before the
     * pass: programs that sample a target of their own pass read the copy (Vulkan leaves reading
     * an attachment that is being written undefined).
     *
     * @param resource a sampled resource
     * @return the copy to bind instead, if the resource is attached to the current pass and copied
     */
    default Optional<GpuTextureView> passCopy(ResourceRef resource) {
        return Optional.empty();
    }
}
