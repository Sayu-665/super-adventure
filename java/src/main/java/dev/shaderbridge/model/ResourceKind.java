package dev.shaderbridge.model;

/** Descriptor kind of a binding ({@code #[serde(tag = "type")]}). */
public sealed interface ResourceKind {
    /**
     * Combined image sampler, as the translated shaders declare it: rectangle samplers are 2D
     * samplers, and comparison samplers the translator emulates (see
     * {@link BindingUse#shadowEmulated()}) are plain samplers.
     *
     * @param dim        {@code 1d}, {@code 1d_array}, {@code 2d}, {@code 2d_array}, {@code 3d}, {@code cube},
     *                   {@code cube_array}, {@code buffer}, {@code 2d_ms}, {@code 2d_ms_array}
     * @param shadow     depth-comparison sampler: bind a sampler with comparison enabled; false means a
     *                   plain sampler, also for emulated comparisons
     * @param sampleType {@code float}, {@code int} or {@code uint}
     */
    record Sampler(String dim, boolean shadow, String sampleType) implements ResourceKind {
    }

    /**
     * Storage image.
     *
     * @param dim        image dimensionality, as for samplers
     * @param format     GLSL format qualifier, or null if undeclared
     * @param sampleType {@code float}, {@code int} or {@code uint}
     * @param readonly   declared {@code readonly}
     * @param writeonly  declared {@code writeonly}
     */
    record StorageImage(String dim, String format, String sampleType, boolean readonly, boolean writeonly)
        implements ResourceKind {
    }

    /** Shader storage buffer. */
    record StorageBuffer() implements ResourceKind {
    }

    /** Uniform buffer. */
    record UniformBuffer() implements ResourceKind {
    }
}
