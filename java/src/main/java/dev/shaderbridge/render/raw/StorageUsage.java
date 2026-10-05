package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.api.textures.GpuTexture;
import java.util.Collection;
import java.util.Collections;
import java.util.Set;
import java.util.WeakHashMap;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.BooleanSupplier;
import java.util.function.Supplier;
import org.lwjgl.vulkan.VK10;

/**
 * Adds {@code VK_IMAGE_USAGE_STORAGE_BIT} to the render targets the pack binds as storage images
 * ({@code colorimgN}, {@code shadowcolorimgN}), which Minecraft never sets. The targets are
 * created through Mojang's texture API; the raw path requests their labels here and two mixins
 * on Mojang's Vulkan texture creation consult it: {@code VulkanDevice.createTexture} is wrapped
 * with {@link #create} and the image create info's usage passes through {@link #imageUsage}. Only
 * formats the device can store to get the bit, and a texture counts as a storage texture only if
 * the usage hook really ran ({@link #hasStorage}); otherwise the raw path binds a stand-in.
 */
public final class StorageUsage {
    private static final Set<String> REQUESTED = ConcurrentHashMap.newKeySet();
    private static final Set<GpuTexture> STORAGE = Collections.synchronizedSet(Collections.newSetFromMap(new WeakHashMap<>()));
    private static final ThreadLocal<Creation> CREATING = new ThreadLocal<>();

    /** A texture creation that asked for the storage bit, and whether the usage hook added it. */
    private static final class Creation {
        boolean applied;
    }

    private StorageUsage() {
    }

    /**
     * Requests the storage bit for textures created with these labels, until the returned handle
     * is closed.
     *
     * @param labels texture labels
     * @return the request; closing it withdraws the labels
     */
    public static AutoCloseable request(Collection<String> labels) {
        Set<String> owned = Set.copyOf(labels);
        REQUESTED.addAll(owned);
        return () -> REQUESTED.removeAll(owned);
    }

    /**
     * Creates a texture, with the storage bit if its label was requested and the device can store
     * to its format. Called by the {@code VulkanDevice.createTexture} wrapper.
     *
     * @param label         the texture's label (may be null)
     * @param formatStorage whether the device supports storage images of the texture's format
     * @param create        creates the texture (the wrapped method)
     * @return the texture
     */
    public static GpuTexture create(String label, BooleanSupplier formatStorage, Supplier<GpuTexture> create) {
        if (label == null || !REQUESTED.contains(label) || !formatStorage.getAsBoolean()) {
            return create.get();
        }
        Creation creation = new Creation();
        Creation outer = CREATING.get();
        CREATING.set(creation);
        try {
            GpuTexture texture = create.get();
            if (creation.applied) {
                STORAGE.add(texture);
            }
            return texture;
        } finally {
            CREATING.set(outer);
        }
    }

    /**
     * Called with the Vulkan usage of every image Mojang creates.
     *
     * @param vkUsage the image's {@code VkImageUsageFlags}
     * @return the usage, with the storage bit while {@link #create} creates a requested texture
     */
    public static int imageUsage(int vkUsage) {
        Creation creation = CREATING.get();
        if (creation == null) {
            return vkUsage;
        }
        creation.applied = true;
        return vkUsage | VK10.VK_IMAGE_USAGE_STORAGE_BIT;
    }

    /**
     * @param texture a texture
     * @return whether it was created with the storage bit
     */
    public static boolean hasStorage(GpuTexture texture) {
        return STORAGE.contains(texture);
    }
}
