package dev.shaderbridge.model;

/**
 * What the host must bind for a resource ({@code #[serde(tag = "type", content = "value")]}).
 * Variant names mirror the Rust variants; the wire tag is their serde snake_case form.
 */
public sealed interface ResourceRef {
    /** @param index colortex index */
    record ColorTex(int index) implements ResourceRef {
    }

    /** @param index depthtex index */
    record DepthTex(int index) implements ResourceRef {
    }

    /** @param index shadowtex index */
    record ShadowTex(int index) implements ResourceRef {
    }

    /** @param index shadowtex index of the hardware-filtering variant ({@code shadowtex0HW}) */
    record ShadowTexHw(int index) implements ResourceRef {
    }

    /** @param index shadowcolor index */
    record ShadowColor(int index) implements ResourceRef {
    }

    /** The noise texture. */
    record Noise() implements ResourceRef {
    }

    /** The block/item atlas (albedo). */
    record Atlas() implements ResourceRef {
    }

    /** The lightmap. */
    record Lightmap() implements ResourceRef {
    }

    /** The normal map of the atlas. */
    record Normals() implements ResourceRef {
    }

    /** The specular map of the atlas. */
    record Specular() implements ResourceRef {
    }

    /** The entity overlay texture. */
    record Overlay() implements ResourceRef {
    }

    /** @param index Distant Horizons depth texture index */
    record DhDepthTex(int index) implements ResourceRef {
    }

    /** The Distant Horizons block atlas. */
    record DhBlockAtlas() implements ResourceRef {
    }

    /** A constant 1x1 white texture. */
    record White() implements ResourceRef {
    }

    /** @param id custom texture id: {@code <stage>.<sampler>} or {@code <stage>.<sampler>.<dim>} */
    record CustomTexture(String id) implements ResourceRef {
    }

    /** @param name custom image name */
    record Image(String name) implements ResourceRef {
    }

    /** @param index colortex index bound as a storage image */
    record ColorImage(int index) implements ResourceRef {
    }

    /** @param index shadowcolor index bound as a storage image */
    record ShadowColorImage(int index) implements ResourceRef {
    }

    /** @param index shader storage buffer index */
    record Ssbo(int index) implements ResourceRef {
    }

    /** @param name uniform block name */
    record UniformBlock(String name) implements ResourceRef {
    }

    /** @param name unknown sampler name (GL texture unit 0 semantics) */
    record Unknown(String name) implements ResourceRef {
    }
}
