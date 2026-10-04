package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.textures.FilterMode;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.render.targets.HostTextures;
import dev.shaderbridge.render.targets.TextureBinding;
import java.util.Optional;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.texture.TextureAtlas;

/**
 * The game's textures for pack programs that sample them by pack name: the block atlas (or the
 * albedo the current draw binds as {@code Sampler0}), the lightmap, the entity overlay and
 * Minecraft's main depth. PBR atlases and Distant Horizons textures are not provided (normals and
 * specular fall back to neutral textures, DH depth to the far plane). Render thread only.
 */
final class MinecraftHost implements HostTextures {
    private final TextureBinding albedo;

    private MinecraftHost(TextureBinding albedo) {
        this.albedo = albedo;
    }

    /** @return the host textures with the block atlas as albedo */
    static MinecraftHost blockAtlas() {
        GpuTextureView atlas = Minecraft.getInstance().getTextureManager().getTexture(TextureAtlas.LOCATION_BLOCKS).getTextureView();
        return new MinecraftHost(new TextureBinding(atlas, RenderSystem.getSamplerCache().getRepeat(FilterMode.NEAREST)));
    }

    /**
     * @param albedo the texture a draw binds as {@code Sampler0}
     * @return host textures whose atlas is that texture
     */
    MinecraftHost withAlbedo(TextureBinding albedo) {
        return new MinecraftHost(albedo);
    }

    @Override
    public TextureBinding atlas() {
        return albedo;
    }

    @Override
    public TextureBinding lightmap() {
        return new TextureBinding(Minecraft.getInstance().gameRenderer.lightmap(), RenderSystem.getSamplerCache().getClampToEdge(FilterMode.LINEAR));
    }

    @Override
    public TextureBinding overlay() {
        return new TextureBinding(Minecraft.getInstance().gameRenderer.overlayTexture().getTextureView(),
            RenderSystem.getSamplerCache().getClampToEdge(FilterMode.LINEAR));
    }

    @Override
    public Optional<TextureBinding> normals() {
        return Optional.empty();
    }

    @Override
    public Optional<TextureBinding> specular() {
        return Optional.empty();
    }

    @Override
    public GpuTextureView mainDepth() {
        return Minecraft.getInstance().gameRenderer.mainRenderTarget().getDepthTextureView();
    }

    @Override
    public Optional<GpuTextureView> dhDepth(int index) {
        return Optional.empty();
    }

    @Override
    public Optional<TextureBinding> dhBlockAtlas() {
        return Optional.empty();
    }
}
