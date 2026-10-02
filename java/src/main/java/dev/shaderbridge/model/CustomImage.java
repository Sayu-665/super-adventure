package dev.shaderbridge.model;

/**
 * A custom storage image ({@code image.<name>}).
 *
 * @param name        image name
 * @param samplerName sampler name to read it through, or null
 * @param format      texture format
 * @param pixelFormat GL pixel format name
 * @param pixelType   GL pixel type name
 * @param clear       cleared every frame
 * @param size        image size
 */
public record CustomImage(
    String name,
    String samplerName,
    TextureFormat format,
    String pixelFormat,
    String pixelType,
    boolean clear,
    ImageSize size
) {
    public CustomImage {
        Copies.required(name, "name");
        Copies.required(size, "size");
    }
}
