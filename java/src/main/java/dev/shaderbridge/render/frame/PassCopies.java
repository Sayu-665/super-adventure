package dev.shaderbridge.render.frame;

import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.model.ResourceRef;
import java.util.HashMap;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.function.Function;

/**
 * Copies of the render targets a geometry pass samples while drawing into them
 * ({@link FeedbackReads}), taken right before the pass. One copy texture per resource, of the
 * base level only, recreated when the target's format or size changes. Render thread only,
 * outside any render pass.
 */
final class PassCopies implements AutoCloseable {
    private static final int USAGE = GpuTexture.USAGE_TEXTURE_BINDING | GpuTexture.USAGE_COPY_DST;

    private record Copy(GpuTexture texture, GpuTextureView view) {
        void close() {
            view.close();
            texture.close();
        }
    }

    private final GpuDevice device;
    private final Map<ResourceRef, Copy> copies = new HashMap<>();

    /** @param device the GPU device */
    PassCopies(GpuDevice device) {
        this.device = device;
    }

    /**
     * Copies the current contents of resources.
     *
     * @param encoder   a command encoder
     * @param resources the resources to copy ({@link FeedbackReads#key} forms)
     * @param sources   the texture the pass attaches for a resource, if it has one
     * @return the copies by resource ({@link FeedbackReads#key} forms)
     */
    Map<ResourceRef, GpuTextureView> take(CommandEncoder encoder, Set<ResourceRef> resources, Function<ResourceRef, Optional<GpuTexture>> sources) {
        Map<ResourceRef, GpuTextureView> out = new HashMap<>();
        for (ResourceRef resource : resources) {
            sources.apply(resource).ifPresent(source -> {
                Copy copy = copyFor(resource, source);
                encoder.copyTextureToTexture(source, copy.texture(), 0, 0, 0, 0, 0, source.getWidth(0), source.getHeight(0));
                out.put(resource, copy.view());
            });
        }
        return out;
    }

    private Copy copyFor(ResourceRef resource, GpuTexture source) {
        Copy copy = copies.get(resource);
        if (copy != null && copy.texture().getFormat() == source.getFormat() && copy.texture().getWidth(0) == source.getWidth(0)
            && copy.texture().getHeight(0) == source.getHeight(0)) {
            return copy;
        }
        if (copy != null) {
            copy.close();
        }
        GpuTexture texture = device.createTexture("ShaderBridge pass copy of " + source.getLabel(), USAGE, source.getFormat(), source.getWidth(0),
            source.getHeight(0), 1, 1);
        Copy created = new Copy(texture, device.createTextureView(texture));
        copies.put(resource, created);
        return created;
    }

    @Override
    public void close() {
        copies.values().forEach(Copy::close);
        copies.clear();
    }
}
