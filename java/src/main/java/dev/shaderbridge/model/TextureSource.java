package dev.shaderbridge.model;

import java.util.List;

/** Source of a custom or noise texture ({@code #[serde(tag = "type")]}). */
public sealed interface TextureSource {
    /**
     * An image inside the pack.
     *
     * @param path path relative to {@code shaders/}
     */
    record PackImage(String path) implements TextureSource {
    }

    /**
     * A resource-pack or vanilla texture.
     *
     * @param location resource location, e.g. {@code minecraft:textures/...}
     */
    record Resource(String location) implements TextureSource {
    }

    /**
     * A dynamic vanilla texture.
     *
     * @param name e.g. {@code minecraft:dynamic/lightmap_1}
     */
    record Dynamic(String name) implements TextureSource {
    }

    /**
     * A raw binary texture.
     *
     * @param path        path relative to {@code shaders/}
     * @param target      {@code 1d}, {@code 2d}, {@code 3d} or {@code 2d_rect}
     * @param dimensions  number of dimensions
     * @param format      texture format
     * @param size        width, height, depth
     * @param pixelFormat GL pixel format name
     * @param pixelType   GL pixel type name
     */
    record Raw(
        String path,
        String target,
        int dimensions,
        TextureFormat format,
        List<Integer> size,
        String pixelFormat,
        String pixelType
    ) implements TextureSource {
        public Raw {
            size = List.copyOf(size == null ? List.of() : size);
        }
    }
}
