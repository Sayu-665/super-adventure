package dev.shaderbridge.render.raw;

import dev.shaderbridge.model.CustomImage;
import dev.shaderbridge.model.ImageSize;
import dev.shaderbridge.model.StorageBuffer;
import dev.shaderbridge.model.TargetSize;
import dev.shaderbridge.model.TextureFormat;
import java.util.Optional;
import java.util.function.Consumer;

/**
 * Sizes of the resources only the raw path provides, with the headless executor's rules: custom
 * images ({@code image.<name>}, screen-relative ones rounded up like render targets) and shader
 * storage buffers ({@code bufferObject.<n>}, screen-relative ones as bytes per pixel), each storage
 * buffer grown to the largest block a program declares for it (GL tolerates a smaller buffer;
 * Vulkan needs the bound range to cover the block) and clamped to the device's range limit.
 */
public final class ResourceSizes {
    /** Smallest storage buffer created. */
    public static final long MIN_BUFFER = 16;

    private ResourceSizes() {
    }

    /**
     * A custom image to create.
     *
     * @param name       the image name
     * @param dimensions 1, 2 or 3
     * @param width      width in texels
     * @param height     height in texels (1 for 1D images)
     * @param depth      depth in texels (1 unless 3D)
     * @param format     the pack's texture format
     * @param clear      cleared at the start of every frame
     * @param relative   sized relative to the screen (recreated when it changes)
     */
    public record ImageSpec(String name, int dimensions, int width, int height, int depth, TextureFormat format, boolean clear, boolean relative) {
    }

    /**
     * Largest image extents of the device.
     *
     * @param max1D {@code maxImageDimension1D}
     * @param max2D {@code maxImageDimension2D}
     * @param max3D {@code maxImageDimension3D}
     */
    public record ImageLimits(int max1D, int max2D, int max3D) {
        /**
         * @param dimensions 1, 2 or 3
         * @return the largest extent per dimension of images with that many dimensions
         */
        public int max(int dimensions) {
            return switch (dimensions) {
                case 1 -> max1D;
                case 2 -> max2D;
                default -> max3D;
            };
        }

        /**
         * @param dimensions 1, 2 or 3
         * @param width      width in texels
         * @param height     height in texels
         * @param depth      depth in texels
         * @return whether the device can create an image of that extent
         */
        public boolean fits(int dimensions, int width, int height, int depth) {
            int max = max(dimensions);
            return width >= 1 && height >= 1 && depth >= 1 && width <= max && height <= max && depth <= max;
        }
    }

    /**
     * @param image    a custom image
     * @param width    screen width
     * @param height   screen height
     * @param limits   the device's image limits
     * @param warnings receives why an image is not created
     * @return the image to create, empty if its size is unusable
     */
    public static Optional<ImageSpec> image(CustomImage image, int width, int height, ImageLimits limits, Consumer<String> warnings) {
        ImageSpec spec = switch (image.size()) {
            case ImageSize.Relative r -> {
                int[] size = new TargetSize.Relative(r.x(), r.y()).resolve(width, height);
                yield new ImageSpec(image.name(), 2, size[0], size[1], 1, image.format(), image.clear(), true);
            }
            case ImageSize.Absolute1D a -> new ImageSpec(image.name(), 1, a.width(), 1, 1, image.format(), image.clear(), false);
            case ImageSize.Absolute2D a -> new ImageSpec(image.name(), 2, a.width(), a.height(), 1, image.format(), image.clear(), false);
            case ImageSize.Absolute3D a -> new ImageSpec(image.name(), 3, a.width(), a.height(), a.depth(), image.format(), image.clear(), false);
        };
        if (!limits.fits(spec.dimensions(), spec.width(), spec.height(), spec.depth())) {
            warnings.accept("custom image " + image.name() + " of " + spec.width() + "x" + spec.height() + "x" + spec.depth()
                + " texels exceeds the device's " + limits.max(spec.dimensions()) + " per dimension; it is not created");
            return Optional.empty();
        }
        return Optional.of(spec);
    }

    /**
     * @param buffer   a storage buffer of the pack
     * @param width    screen width
     * @param height   screen height
     * @param declared the largest block any program declares for it (0 if none)
     * @param maxRange the device's {@code maxStorageBufferRange}
     * @param warnings receives size adjustments
     * @return the size to create it with
     */
    public static long storageBuffer(StorageBuffer buffer, int width, int height, long declared, long maxRange, Consumer<String> warnings) {
        long size = buffer.size();
        if (buffer.relative() != null && buffer.relative().size() == 2) {
            int[] pixels = new TargetSize.Relative(buffer.relative().get(0), buffer.relative().get(1)).resolve(width, height);
            size = saturatingMultiply(saturatingMultiply(size, pixels[0]), pixels[1]);
        }
        if (declared > size) {
            warnings.accept("storage buffer " + buffer.index() + " is " + size + " bytes but a program declares a " + declared
                + "-byte block; it is enlarged (zero-filled)");
            size = declared;
        }
        if (size > maxRange) {
            warnings.accept("storage buffer " + buffer.index() + " of " + size + " bytes exceeds the device limit; it is clamped to " + maxRange);
            size = maxRange;
        }
        return Math.max(MIN_BUFFER, size);
    }

    private static long saturatingMultiply(long a, long b) {
        try {
            return Math.multiplyExact(a, b);
        } catch (ArithmeticException overflow) {
            return Long.MAX_VALUE;
        }
    }
}
