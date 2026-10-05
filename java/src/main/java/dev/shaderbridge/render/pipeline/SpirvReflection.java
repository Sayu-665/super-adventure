package dev.shaderbridge.render.pipeline;

import java.util.List;
import java.util.Optional;

/**
 * What one SPIR-V module declares, as SPIRV-Cross reports it to Mojang's {@code PipelineBuilder}:
 * descriptors named like SPIRV-Cross names them (the block type name for buffer blocks, the
 * variable name otherwise), and the non-builtin stage inputs and outputs. Only variables of the
 * entry point's interface count (SPIR-V 1.4 and later list every global the entry point uses).
 *
 * @param stage              the execution model of the module's first entry point
 * @param descriptors        resources bound through descriptor sets
 * @param inputs             non-builtin stage inputs
 * @param outputs            non-builtin stage outputs
 * @param pushConstantBlocks number of push-constant blocks
 */
public record SpirvReflection(
    Stage stage,
    List<Descriptor> descriptors,
    List<InterfaceVariable> inputs,
    List<InterfaceVariable> outputs,
    int pushConstantBlocks
) {
    public SpirvReflection {
        descriptors = List.copyOf(descriptors);
        inputs = List.copyOf(inputs);
        outputs = List.copyOf(outputs);
    }

    /**
     * @param name a descriptor name
     * @return the descriptor of that name, if the module declares it
     */
    public Optional<Descriptor> descriptor(String name) {
        return descriptors.stream().filter(d -> d.name().equals(name)).findFirst();
    }

    /** SPIR-V execution models. */
    public enum Stage {
        VERTEX,
        TESS_CONTROL,
        TESS_EVALUATION,
        GEOMETRY,
        FRAGMENT,
        COMPUTE,
        OTHER;

        static Stage of(int executionModel) {
            return switch (executionModel) {
                case 0 -> VERTEX;
                case 1 -> TESS_CONTROL;
                case 2 -> TESS_EVALUATION;
                case 3 -> GEOMETRY;
                case 4 -> FRAGMENT;
                case 5 -> COMPUTE;
                default -> OTHER;
            };
        }
    }

    /** Descriptor kinds, in SPIRV-Cross' resource classification. */
    public enum DescriptorType {
        /** A {@code Block} in the {@code Uniform} storage class. */
        UNIFORM_BUFFER,
        /** A {@code BufferBlock}, or a block in the {@code StorageBuffer} storage class. */
        STORAGE_BUFFER,
        /** A combined image sampler ({@code sampler2D}, {@code samplerBuffer}, ...). */
        SAMPLED_IMAGE,
        /** A storage image ({@code image2D}, ...). */
        STORAGE_IMAGE,
        /** A separate sampled image ({@code texture2D}, {@code textureBuffer}). */
        SEPARATE_IMAGE,
        /** A separate sampler ({@code sampler}). */
        SEPARATE_SAMPLER,
        /** Anything else (subpass inputs, acceleration structures). */
        OTHER
    }

    /** {@code SpvDim} of an image type. */
    public enum ImageDim {
        D1,
        D2,
        D3,
        CUBE,
        RECT,
        BUFFER,
        SUBPASS_DATA,
        /** Not an image. */
        NONE;

        static ImageDim of(int spvDim) {
            return switch (spvDim) {
                case 0 -> D1;
                case 1 -> D2;
                case 2 -> D3;
                case 3 -> CUBE;
                case 4 -> RECT;
                case 5 -> BUFFER;
                case 6 -> SUBPASS_DATA;
                default -> NONE;
            };
        }
    }

    /** Numeric class of a scalar, vector or matrix type. */
    public enum ScalarClass {
        FLOAT,
        INT,
        UINT,
        /** Booleans, 64-bit and smaller-than-32-bit types, structs. */
        OTHER
    }

    /**
     * One descriptor.
     *
     * @param name         the name Mojang's pipeline builder matches against the bind group layout
     * @param type         the descriptor kind
     * @param dim          image dimensionality, {@link ImageDim#NONE} for buffers and samplers
     * @param arrayed      the image is an array image
     * @param multisampled the image is multisampled
     * @param arraySize    1 for a single descriptor, the element count of a descriptor array, 0 for a
     *                     runtime-sized array
     * @param sampled      numeric class of the sampled/stored texel, {@link ScalarClass#OTHER} for buffers
     * @param set          the {@code DescriptorSet} decoration, -1 if absent
     * @param binding      the {@code Binding} decoration, -1 if absent
     */
    public record Descriptor(String name, DescriptorType type, ImageDim dim, boolean arrayed, boolean multisampled, int arraySize,
                             ScalarClass sampled, int set, int binding) {
        /**
         * @return whether the descriptor has both {@code DescriptorSet} and {@code Binding}
         *     decorations, which Mojang's pipeline builder rewrites in place and the raw path binds by
         */
        public boolean decorated() {
            return set >= 0 && binding >= 0;
        }
    }

    /**
     * One stage input or output.
     *
     * @param name          variable name
     * @param location      {@code Location} decoration, or -1 if absent
     * @param scalar        numeric class of the components
     * @param vectorSize    components per location (1 for scalars, the column size for matrices)
     * @param locationCount locations the variable occupies (array elements times matrix columns)
     * @param flat          decorated {@code Flat}
     * @param component     decorated {@code Component}
     * @param struct        the variable is (an array of) a struct or interface block
     */
    public record InterfaceVariable(String name, int location, ScalarClass scalar, int vectorSize, int locationCount, boolean flat,
                                    boolean component, boolean struct) {
    }
}
