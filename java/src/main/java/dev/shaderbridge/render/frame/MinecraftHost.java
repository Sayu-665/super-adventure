package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.textures.FilterMode;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.render.targets.HostTextures;
import dev.shaderbridge.render.targets.TextureBinding;
import java.util.Map;
import java.util.Optional;
import net.minecraft.client.Minecraft;
import net.minecraft.data.AtlasIds;

/**
 * The game's textures for pack programs that sample them by pack name: the block atlas (or the
 * albedo the current draw binds as {@code Sampler0}), the lightmap, the entity overlay,
 * Minecraft's main depth, and Distant Horizons' {@code dhDepthTex0/1} and block atlas while LODs
 * are drawn ({@link DistantFrame}), and the copies of the targets the current pass draws into
 * ({@link PassCopies}). PBR atlases are not provided (normals and specular fall back to neutral
 * textures; DH depth falls back to the far plane without LODs). Render thread only.
 */
final class MinecraftHost implements HostTextures {
    private final TextureBinding albedo;
    private final DistantFrame distant;
    private final Map<ResourceRef, GpuTextureView> passCopies;

    private MinecraftHost(TextureBinding albedo, DistantFrame distant, Map<ResourceRef, GpuTextureView> passCopies) {
        this.albedo = albedo;
        this.distant = distant;
        this.passCopies = passCopies;
    }

    /**
     * @param distant the frame's Distant Horizons textures
     * @return the host textures with the block atlas as albedo
     */
    static MinecraftHost blockAtlas(DistantFrame distant) {
        GpuTextureView atlas = Minecraft.getInstance().getAtlasManager().getAtlasOrThrow(AtlasIds.BLOCKS).getTextureView();
        return new MinecraftHost(new TextureBinding(atlas, RenderSystem.getSamplerCache().getRepeat(FilterMode.NEAREST)), distant, Map.of());
    }

    /**
     * @param albedo the texture a draw binds as {@code Sampler0}
     * @return host textures whose atlas is that texture
     */
    MinecraftHost withAlbedo(TextureBinding albedo) {
        return new MinecraftHost(albedo, distant, passCopies);
    }

    /**
     * @param copies the copies of the targets the current pass draws into ({@link FeedbackReads#key} forms)
     * @return host textures that hand out those copies
     */
    MinecraftHost withPassCopies(Map<ResourceRef, GpuTextureView> copies) {
        return new MinecraftHost(albedo, distant, Map.copyOf(copies));
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
        return distant.depthTexture(index);
    }

    @Override
    public Optional<TextureBinding> dhBlockAtlas() {
        return distant.blockAtlas();
    }

    @Override
    public Optional<GpuTextureView> passCopy(ResourceRef resource) {
        return FeedbackReads.key(resource).map(passCopies::get);
    }
}
