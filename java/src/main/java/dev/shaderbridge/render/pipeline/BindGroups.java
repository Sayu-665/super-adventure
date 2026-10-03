package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.pipeline.BindGroupLayout;
import com.mojang.renderpearl.api.pipeline.UniformType;
import dev.shaderbridge.render.pipeline.SpirvReflection.Descriptor;
import dev.shaderbridge.render.pipeline.SpirvReflection.DescriptorType;
import dev.shaderbridge.render.pipeline.SpirvReflection.ImageDim;

/**
 * The bind group layout of a pack pipeline. It lists exactly the descriptors the SPIR-V declares:
 * Mojang's pipeline builder fails on a declared descriptor the layout lacks, and its strict draw
 * validation fails on a layout entry nobody binds. Descriptors the replaced vanilla pipeline also
 * declares keep the vanilla description (e.g. a texel buffer's format).
 */
public final class BindGroups {
    private BindGroups() {
    }

    /**
     * @param iface the program's reflected interface (must be {@link Eligibility eligible})
     * @param shape the draw it replaces
     * @return the layout
     * @throws IllegalArgumentException for a descriptor renderpearl cannot bind
     */
    public static BindGroupLayout layout(ProgramInterface iface, PipelineShape shape) {
        BindGroupLayout.Builder builder = BindGroupLayout.builder();
        for (Descriptor d : iface.descriptors().values()) {
            BindGroupLayout.UniformDescription host = shape.hostUniforms().get(d.name());
            if (host != null && host.type() == typeOf(d)) {
                if (host.type() == UniformType.TEXEL_BUFFER) {
                    builder.withUniform(d.name(), UniformType.TEXEL_BUFFER, host.gpuFormat());
                } else {
                    builder.withUniform(d.name(), host.type());
                }
            } else if (typeOf(d) == UniformType.TEXEL_BUFFER) {
                throw new IllegalArgumentException("texel buffer " + d.name() + " has no host format");
            } else {
                builder.withUniform(d.name(), typeOf(d));
            }
        }
        return builder.build();
    }

    /**
     * @param d a descriptor
     * @return its renderpearl uniform type
     * @throws IllegalArgumentException for a descriptor renderpearl cannot bind
     */
    static UniformType typeOf(Descriptor d) {
        if (d.type() == DescriptorType.UNIFORM_BUFFER) {
            return UniformType.UNIFORM_BUFFER;
        }
        if (d.type() == DescriptorType.SAMPLED_IMAGE) {
            return d.dim() == ImageDim.BUFFER ? UniformType.TEXEL_BUFFER : UniformType.COMBINED_IMAGE_SAMPLER;
        }
        throw new IllegalArgumentException("descriptor " + d.name() + " (" + d.type() + ") cannot be bound through renderpearl");
    }
}
