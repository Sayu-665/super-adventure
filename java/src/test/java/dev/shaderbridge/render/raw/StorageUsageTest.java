package dev.shaderbridge.render.raw;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.textures.GpuTexture;
import java.lang.reflect.Proxy;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;
import org.lwjgl.vulkan.VK10;

/** The storage usage hooks, as the two texture creation mixins drive them. */
class StorageUsageTest {
    private static final int MOJANG_USAGE = VK10.VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK10.VK_IMAGE_USAGE_SAMPLED_BIT;

    private static GpuTexture texture() {
        return (GpuTexture) Proxy.newProxyInstance(GpuTexture.class.getClassLoader(), new Class<?>[] {GpuTexture.class},
            (proxy, method, args) -> switch (method.getName()) {
                case "hashCode" -> System.identityHashCode(proxy);
                case "equals" -> proxy == args[0];
                default -> null;
            });
    }

    /** Creates a texture like {@code VulkanGpuTexture.<init>}: passes its usage through the hook. */
    private static GpuTexture create(String label, boolean formatStorage, AtomicInteger usage) {
        return StorageUsage.create(label, () -> formatStorage, () -> {
            usage.set(StorageUsage.imageUsage(MOJANG_USAGE));
            return texture();
        });
    }

    @Test
    void requestedLabelsGetTheStorageBit() throws Exception {
        AtomicInteger usage = new AtomicInteger();
        AutoCloseable request = StorageUsage.request(List.of("ShaderBridge colortex3", "ShaderBridge colortex3 (alt)"));
        try {
            GpuTexture main = create("ShaderBridge colortex3", true, usage);
            assertEquals(MOJANG_USAGE | VK10.VK_IMAGE_USAGE_STORAGE_BIT, usage.get());
            assertTrue(StorageUsage.hasStorage(main));
            GpuTexture other = create("ShaderBridge colortex4", true, usage);
            assertEquals(MOJANG_USAGE, usage.get(), "not requested");
            assertFalse(StorageUsage.hasStorage(other));
            GpuTexture unsupported = create("ShaderBridge colortex3 (alt)", false, usage);
            assertEquals(MOJANG_USAGE, usage.get(), "the device cannot store to the format");
            assertFalse(StorageUsage.hasStorage(unsupported));
        } finally {
            request.close();
        }
        GpuTexture withdrawn = create("ShaderBridge colortex3", true, usage);
        assertEquals(MOJANG_USAGE, usage.get(), "the request was closed");
        assertFalse(StorageUsage.hasStorage(withdrawn));
        assertEquals(MOJANG_USAGE, StorageUsage.imageUsage(MOJANG_USAGE), "textures created outside a requested creation keep their usage");
    }

    @Test
    void aTextureCountsAsStorageOnlyIfTheUsageHookRan() throws Exception {
        AutoCloseable request = StorageUsage.request(List.of("ShaderBridge colortex1"));
        try {
            GpuTexture texture = StorageUsage.create("ShaderBridge colortex1", () -> true, StorageUsageTest::texture);
            assertFalse(StorageUsage.hasStorage(texture), "VulkanGpuTextureMixin did not apply");
        } finally {
            request.close();
        }
    }

    @Test
    void aFailedCreationLeavesNoStateBehind() throws Exception {
        AutoCloseable request = StorageUsage.request(List.of("ShaderBridge colortex2"));
        try {
            assertThrows(IllegalStateException.class, () -> StorageUsage.create("ShaderBridge colortex2", () -> true, () -> {
                throw new IllegalStateException("out of memory");
            }));
            assertEquals(MOJANG_USAGE, StorageUsage.imageUsage(MOJANG_USAGE));
        } finally {
            request.close();
        }
    }
}
