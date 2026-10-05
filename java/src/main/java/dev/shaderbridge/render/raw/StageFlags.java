package dev.shaderbridge.render.raw;

import dev.shaderbridge.model.ShaderStage;
import org.lwjgl.vulkan.VK10;

/** {@code VkShaderStageFlagBits} of pack shader stages. */
public final class StageFlags {
    private StageFlags() {
    }

    /**
     * @param stage a shader stage
     * @return its {@code VkShaderStageFlagBits}
     */
    public static int of(ShaderStage stage) {
        return switch (stage) {
            case VERTEX -> VK10.VK_SHADER_STAGE_VERTEX_BIT;
            case TESS_CONTROL -> VK10.VK_SHADER_STAGE_TESSELLATION_CONTROL_BIT;
            case TESS_EVAL -> VK10.VK_SHADER_STAGE_TESSELLATION_EVALUATION_BIT;
            case GEOMETRY -> VK10.VK_SHADER_STAGE_GEOMETRY_BIT;
            case FRAGMENT -> VK10.VK_SHADER_STAGE_FRAGMENT_BIT;
            case COMPUTE -> VK10.VK_SHADER_STAGE_COMPUTE_BIT;
        };
    }
}
