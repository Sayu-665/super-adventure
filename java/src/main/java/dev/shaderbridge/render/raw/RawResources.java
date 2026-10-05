package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.backend.vulkan.VulkanConst;
import dev.shaderbridge.model.CustomImage;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.ImageSize;
import dev.shaderbridge.model.StorageBuffer;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import dev.shaderbridge.render.pipeline.TextureFormats;
import java.nio.ByteBuffer;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.function.Consumer;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.system.MemoryUtil;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VkBufferCopy;
import org.lwjgl.vulkan.VkBufferImageCopy;
import org.lwjgl.vulkan.VkClearColorValue;
import org.lwjgl.vulkan.VkCommandBuffer;
import org.lwjgl.vulkan.VkImageSubresourceRange;

/**
 * The resources of a dimension pipeline only the raw path provides, allocated through Minecraft's
 * VMA allocator: the pack's custom images ({@code image.<name>}, 1D/2D/3D), shader storage
 * buffers ({@code bufferObject.<n>}) and the raw textures Minecraft cannot upload
 * ({@link RawTextureData}), plus 1x1 stand-ins for descriptors without a usable resource and a
 * zero buffer for missing storage buffers. Screen-relative ones are recreated when the screen size
 * changes (losing their contents, as in Iris). Every resource is initialized (moved to
 * {@code GENERAL} and cleared, or filled with its initial data through a staging buffer) right
 * before its first use, through {@link ResourceInit}. Render thread only.
 */
final class RawResources implements AutoCloseable {
    private final VulkanContext ctx;
    private final DimensionPipeline dim;
    private final Map<Integer, Long> declaredBlocks;
    private final Map<Integer, byte[]> initialData;
    private final List<RawTextureData.Upload> pendingTextures;
    private final Consumer<String> warnings;
    private final ResourceInit<Object> init = new ResourceInit<>();
    private final Map<String, VmaImage> images = new HashMap<>();
    private final Map<String, Texture> textures = new HashMap<>();
    private final Map<VmaImage, ByteBuffer> texels = new HashMap<>();
    private final Map<Integer, VmaBuffer> buffers = new HashMap<>();
    private final Map<String, VmaImage> standIns = new HashMap<>();
    private VmaBuffer zero;
    private int width = -1;
    private int height = -1;

    /**
     * A raw texture the raw path uploaded itself.
     *
     * @param image  the image
     * @param format its format
     * @param linear sampled with linear filtering
     * @param repeat sampled with repeat addressing
     */
    record Texture(VmaImage image, GpuFormat format, boolean linear, boolean repeat) {
    }

    /**
     * @param ctx            the Vulkan context
     * @param dim            the dimension pipeline
     * @param declaredBlocks per storage buffer index, the largest block a program declares for it
     * @param initialData    per storage buffer index, its initial contents ({@code bufferObject} file)
     * @param textures       the raw textures to provide
     * @param warnings       receives resources that cannot be created as declared
     */
    RawResources(VulkanContext ctx, DimensionPipeline dim, Map<Integer, Long> declaredBlocks, Map<Integer, byte[]> initialData,
                 List<RawTextureData.Upload> textures, Consumer<String> warnings) {
        this.ctx = ctx;
        this.dim = dim;
        this.declaredBlocks = Map.copyOf(declaredBlocks);
        this.initialData = Map.copyOf(initialData);
        this.pendingTextures = new ArrayList<>(textures);
        this.warnings = warnings;
    }

    /** @return the first-use and per-frame initialization of the resources */
    ResourceInit<Object> init() {
        return init;
    }

    /**
     * Creates the resources on first use and recreates the screen-relative ones when the screen
     * size changed.
     *
     * @param screenWidth  screen width
     * @param screenHeight screen height
     */
    void update(int screenWidth, int screenHeight) {
        if (screenWidth == width && screenHeight == height) {
            return;
        }
        boolean first = width < 0;
        width = screenWidth;
        height = screenHeight;
        pendingTextures.forEach(this::createTexture);
        pendingTextures.clear();
        for (CustomImage image : dim.targets().images()) {
            if (first || image.size() instanceof ImageSize.Relative) {
                ResourceSizes.image(image, width, height, ctx.images(), warnings)
                    .ifPresentOrElse(this::replaceImage, () -> discard(images.remove(image.name())));
            }
        }
        for (StorageBuffer buffer : dim.targets().buffers()) {
            if (first || buffer.relative() != null) {
                long size = ResourceSizes.storageBuffer(buffer, width, height, declaredBlocks.getOrDefault(buffer.index(), 0L),
                    ctx.maxBufferRange(), warnings);
                VmaBuffer created = VmaBuffer.create(ctx, size, VmaBuffer.STORAGE_USAGE);
                discard(buffers.put(buffer.index(), created));
                init.created(created, false);
            }
        }
    }

    private void replaceImage(ResourceSizes.ImageSpec spec) {
        int vkFormat = VulkanConst.toVk(TextureFormats.renderable(spec.format()));
        if (!ctx.supports(vkFormat, VK10.VK_FORMAT_FEATURE_STORAGE_IMAGE_BIT | VK10.VK_FORMAT_FEATURE_SAMPLED_IMAGE_BIT)) {
            warnings.accept("custom image " + spec.name() + ": the device cannot store to " + spec.format() + " images; it is not created");
            discard(images.remove(spec.name()));
            return;
        }
        VmaImage created = VmaImage.create(ctx, vkFormat, spec.dimensions(), spec.width(), spec.height(), spec.depth(), VmaImage.USAGE);
        discard(images.put(spec.name(), created));
        init.created(created, spec.clear());
    }

    private void createTexture(RawTextureData.Upload upload) {
        String name = "custom texture " + upload.id();
        if (!ctx.images().fits(upload.dimensions(), upload.width(), upload.height(), upload.depth())) {
            warnings.accept(name + " of " + upload.width() + "x" + upload.height() + "x" + upload.depth() + " texels exceeds the device's "
                + ctx.images().max(upload.dimensions()) + " per dimension; it is not created");
            return;
        }
        int vkFormat = VulkanConst.toVk(upload.format());
        if (!ctx.supports(vkFormat, VK10.VK_FORMAT_FEATURE_SAMPLED_IMAGE_BIT)) {
            warnings.accept(name + ": the device cannot sample " + upload.format() + " images; it is not created");
            return;
        }
        VmaImage image;
        try {
            image = VmaImage.create(ctx, vkFormat, upload.dimensions(), upload.width(), upload.height(), upload.depth(), VmaImage.TEXTURE_USAGE);
        } catch (RawVulkanException e) {
            warnings.accept(name + " cannot be created: " + e.getMessage());
            return;
        }
        textures.put(upload.id(), new Texture(image, upload.format(), upload.linear(), upload.repeat()));
        texels.put(image, upload.texels());
        init.created(image, false);
    }

    /**
     * @param name a custom image name
     * @return the image, empty if the pack declares none of that name or it could not be created
     */
    Optional<VmaImage> image(String name) {
        return Optional.ofNullable(images.get(name));
    }

    /**
     * @param id a {@code ResourceRef.CustomTexture} id
     * @return the raw texture of that id, empty if the raw path does not provide it
     */
    Optional<Texture> texture(String id) {
        return Optional.ofNullable(textures.get(id));
    }

    /**
     * @param index a storage buffer index
     * @return the buffer, empty if the pack declares none of that index
     */
    Optional<VmaBuffer> buffer(int index) {
        return Optional.ofNullable(buffers.get(index));
    }

    /**
     * A 1x1 image standing in for a texture or storage image that does not exist or does not fit
     * the descriptor: opaque black for float formats (an incomplete texture in GL), zero for
     * integer ones.
     *
     * @param dimensions 1, 2 or 3
     * @param vkFormat   its format
     * @return the stand-in
     */
    VmaImage standIn(int dimensions, int vkFormat) {
        return standIns.computeIfAbsent(dimensions + "/" + vkFormat, k -> {
            VmaImage image = VmaImage.create(ctx, vkFormat, dimensions, 1, 1, 1, VmaImage.USAGE);
            init.created(image, false);
            return image;
        });
    }

    /**
     * @param texel the texel class a descriptor reads
     * @return the stand-in format of that class: RGBA8 for floats, RGBA32 integers otherwise (all
     *     storage-capable on every device)
     */
    static int standInFormat(ScalarClass texel) {
        return switch (texel) {
            case INT -> VK10.VK_FORMAT_R32G32B32A32_SINT;
            case UINT -> VK10.VK_FORMAT_R32G32B32A32_UINT;
            default -> VK10.VK_FORMAT_R8G8B8A8_UNORM;
        };
    }

    /**
     * @param minSize the smallest size the binding needs
     * @return a zero-filled buffer of at least that size (at most the device's storage buffer range)
     */
    VmaBuffer zero(long minSize) {
        if (zero == null || zero.size() < Math.min(minSize, ctx.maxBufferRange())) {
            discard(zero);
            long size = Math.max(ResourceSizes.MIN_BUFFER, Long.highestOneBit(Math.max(1, minSize - 1)) << 1);
            zero = VmaBuffer.create(ctx, Math.min(size, ctx.maxBufferRange()), VmaBuffer.STORAGE_USAGE);
            init.created(zero, false);
        }
        return zero;
    }

    /**
     * Records a resource's initialization or per-frame clear. Called between two full barriers.
     *
     * @param cb       the command buffer
     * @param resource a resource of this object
     * @param initial  its first use (load initial data where it has some)
     */
    void fill(VkCommandBuffer cb, Object resource, boolean initial) {
        switch (resource) {
            case VmaImage image when texels.containsKey(image) -> upload(cb, image, texels.remove(image));
            case VmaImage image -> clear(cb, image);
            case VmaBuffer buffer -> {
                byte[] data = initial ? initialData(buffer) : null;
                long loaded = data == null ? 0 : upload(cb, buffer, data);
                if (loaded < buffer.size()) {
                    VK10.vkCmdFillBuffer(cb, buffer.buffer(), loaded, VK10.VK_WHOLE_SIZE, 0);
                }
            }
            default -> throw new IllegalArgumentException("not a raw resource: " + resource);
        }
    }

    private void clear(VkCommandBuffer cb, VmaImage image) {
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkClearColorValue color = VkClearColorValue.calloc(stack);
            if (standIns.containsValue(image) && image.vkFormat() == VK10.VK_FORMAT_R8G8B8A8_UNORM) {
                color.float32(3, 1);
            }
            VkImageSubresourceRange range = VkImageSubresourceRange.calloc(stack).set(VK10.VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1);
            VK10.vkCmdClearColorImage(cb, image.image(), VK10.VK_IMAGE_LAYOUT_GENERAL, color, range);
        }
    }

    private byte[] initialData(VmaBuffer buffer) {
        return buffers.entrySet().stream().filter(e -> e.getValue() == buffer).findFirst().map(e -> initialData.get(e.getKey())).orElse(null);
    }

    /**
     * Copies initial data, zero-padded to whole words, through a staging buffer destroyed once the
     * frame's submit completed.
     *
     * @return the bytes written from the start of the buffer, a multiple of 4
     */
    private long upload(VkCommandBuffer cb, VmaBuffer buffer, byte[] data) {
        long length = Math.min((data.length + 3L) & ~3L, buffer.size() & ~3L);
        if (length == 0) {
            return 0;
        }
        ByteBuffer bytes = MemoryUtil.memCalloc((int) length);
        try (MemoryStack stack = MemoryStack.stackPush()) {
            bytes.put(data, 0, (int) Math.min(data.length, length)).clear();
            VmaBuffer staging = VmaBuffer.staging(ctx, bytes);
            ctx.destroyLater(staging);
            VkBufferCopy.Buffer region = VkBufferCopy.calloc(1, stack).srcOffset(0).dstOffset(0).size(length);
            VK10.vkCmdCopyBuffer(cb, staging.buffer(), buffer.buffer(), region);
            return length;
        } finally {
            MemoryUtil.memFree(bytes);
        }
    }

    /** Copies a raw texture's texels into its image (in {@code GENERAL}) through a staging buffer. */
    private void upload(VkCommandBuffer cb, VmaImage image, ByteBuffer data) {
        VmaBuffer staging = VmaBuffer.staging(ctx, data);
        ctx.destroyLater(staging);
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkBufferImageCopy.Buffer region = VkBufferImageCopy.calloc(1, stack);
            region.get(0).imageSubresource().set(VK10.VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1);
            region.get(0).imageExtent().set(image.width(), image.height(), image.depth());
            VK10.vkCmdCopyBufferToImage(cb, staging.buffer(), image.image(), VK10.VK_IMAGE_LAYOUT_GENERAL, region);
        }
    }

    private void discard(Object resource) {
        if (resource != null) {
            init.destroyed(resource);
            ctx.destroyLater(switch (resource) {
                case VmaImage image -> image;
                case VmaBuffer buffer -> buffer;
                default -> throw new IllegalArgumentException("not a raw resource: " + resource);
            });
        }
    }

    @Override
    public void close() {
        images.values().forEach(this::discard);
        textures.values().forEach(t -> discard(t.image()));
        buffers.values().forEach(this::discard);
        standIns.values().forEach(this::discard);
        discard(zero);
        images.clear();
        textures.clear();
        texels.clear();
        pendingTextures.clear();
        buffers.clear();
        standIns.clear();
        zero = null;
    }
}
