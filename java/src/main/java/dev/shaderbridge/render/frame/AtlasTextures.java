package dev.shaderbridge.render.frame;

import com.mojang.renderpearl.api.textures.GpuTexture;
import java.util.Collections;
import java.util.IdentityHashMap;
import java.util.Set;
import net.minecraft.client.Minecraft;

/**
 * Which textures are Minecraft's texture atlases (blocks, items, particles, ...), for
 * {@link AlbedoSize#atlas()}. Atlases are recreated on resource reloads, so the set is read again
 * every frame. Render thread only.
 */
final class AtlasTextures {
    private final Set<GpuTexture> textures = Collections.newSetFromMap(new IdentityHashMap<>());

    /** Reads the current atlases. */
    void refresh() {
        textures.clear();
        Minecraft.getInstance().getAtlasManager().forEach((id, atlas) -> textures.add(atlas.getTexture()));
    }

    /**
     * @param texture a texture
     * @return whether it is one of the atlases at the last {@link #refresh}
     */
    boolean contains(GpuTexture texture) {
        return textures.contains(texture);
    }
}
