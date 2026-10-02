package dev.shaderbridge.model;

/**
 * A custom texture ({@code texture.<stage>.<name>} or {@code customTexture.<name>}).
 *
 * @param sampler sampler name in GLSL
 * @param stage   texture stage ({@code gbuffers}, {@code composite}, ...) or {@code custom}
 * @param source  where the texture comes from
 * @param blur    linear filtering
 * @param clamp   clamp-to-edge addressing
 */
public record CustomTexture(String sampler, String stage, TextureSource source, boolean blur, boolean clamp) {
    public CustomTexture {
        Copies.required(sampler, "sampler");
        Copies.required(source, "source");
    }
}
