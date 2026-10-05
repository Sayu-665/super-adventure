package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.backend.vulkan.VulkanConst;
import dev.shaderbridge.model.BlendMode;
import java.nio.IntBuffer;
import java.nio.LongBuffer;
import java.util.List;
import java.util.Optional;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VkDevice;
import org.lwjgl.vulkan.VkGraphicsPipelineCreateInfo;
import org.lwjgl.vulkan.VkPipelineColorBlendAttachmentState;
import org.lwjgl.vulkan.VkPipelineColorBlendStateCreateInfo;
import org.lwjgl.vulkan.VkPipelineDynamicStateCreateInfo;
import org.lwjgl.vulkan.VkPipelineInputAssemblyStateCreateInfo;
import org.lwjgl.vulkan.VkPipelineMultisampleStateCreateInfo;
import org.lwjgl.vulkan.VkPipelineRasterizationStateCreateInfo;
import org.lwjgl.vulkan.VkPipelineRenderingCreateInfoKHR;
import org.lwjgl.vulkan.VkPipelineShaderStageCreateInfo;
import org.lwjgl.vulkan.VkPipelineVertexInputStateCreateInfo;
import org.lwjgl.vulkan.VkPipelineViewportStateCreateInfo;

/**
 * Graphics pipelines of raw fullscreen programs, for dynamic rendering (as Minecraft's own,
 * {@code VK_KHR_dynamic_rendering}): no vertex input (the {@code fullscreen} profile generates its
 * quad from {@code gl_VertexIndex}), triangle list, no culling, clockwise front faces (the host
 * convention), no depth attachment, dynamic viewport and scissor, and one color blend state per
 * slot. Slots without attachment have format {@code UNDEFINED} and the state of the first slot
 * with one, so that without {@code independentBlend} all states agree whenever the slots with
 * attachments do ({@link ColorStates#problem}).
 */
final class GraphicsPipelines {
    private GraphicsPipelines() {
    }

    /**
     * A shader stage of the pipeline.
     *
     * @param flag       its {@code VkShaderStageFlagBits}
     * @param module     its {@code VkShaderModule}
     * @param entryPoint its entry point
     */
    record Stage(int flag, long module, String entryPoint) {
    }

    /**
     * @param device the device
     * @param layout the pipeline layout
     * @param stages the shader stages
     * @param slots  the color attachment states
     * @return the pipeline
     * @throws RawVulkanException if Vulkan fails
     */
    static long create(VkDevice device, long layout, List<Stage> stages, List<ColorStates.Slot> slots) {
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkPipelineShaderStageCreateInfo.Buffer shaderStages = VkPipelineShaderStageCreateInfo.calloc(stages.size(), stack);
            for (int i = 0; i < stages.size(); i++) {
                Stage s = stages.get(i);
                shaderStages.get(i).sType$Default().stage(s.flag()).module(s.module()).pName(stack.UTF8(s.entryPoint()));
            }
            IntBuffer formats = stack.mallocInt(slots.size());
            VkPipelineColorBlendAttachmentState.Buffer blends = VkPipelineColorBlendAttachmentState.calloc(slots.size(), stack);
            ColorStates.Slot common = slots.stream().filter(s -> s.format().isPresent()).findFirst().orElse(null);
            for (int i = 0; i < slots.size(); i++) {
                ColorStates.Slot slot = slots.get(i);
                formats.put(i, slot.format().map(VulkanConst::toVk).orElse(VK10.VK_FORMAT_UNDEFINED));
                blend(blends.get(i), slot.format().isPresent() || common == null ? slot : common);
            }
            VkPipelineRenderingCreateInfoKHR rendering = VkPipelineRenderingCreateInfoKHR.calloc(stack).sType$Default()
                .pColorAttachmentFormats(formats)
                .depthAttachmentFormat(VK10.VK_FORMAT_UNDEFINED);
            VkGraphicsPipelineCreateInfo.Buffer info = VkGraphicsPipelineCreateInfo.calloc(1, stack).sType$Default()
                .pNext(rendering)
                .pStages(shaderStages)
                .pVertexInputState(VkPipelineVertexInputStateCreateInfo.calloc(stack).sType$Default())
                .pInputAssemblyState(VkPipelineInputAssemblyStateCreateInfo.calloc(stack).sType$Default().topology(VK10.VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST))
                .pViewportState(VkPipelineViewportStateCreateInfo.calloc(stack).sType$Default().viewportCount(1).scissorCount(1))
                .pRasterizationState(VkPipelineRasterizationStateCreateInfo.calloc(stack).sType$Default()
                    .polygonMode(VK10.VK_POLYGON_MODE_FILL)
                    .cullMode(VK10.VK_CULL_MODE_NONE)
                    .frontFace(VK10.VK_FRONT_FACE_CLOCKWISE)
                    .lineWidth(1))
                .pMultisampleState(VkPipelineMultisampleStateCreateInfo.calloc(stack).sType$Default().rasterizationSamples(VK10.VK_SAMPLE_COUNT_1_BIT))
                .pColorBlendState(VkPipelineColorBlendStateCreateInfo.calloc(stack).sType$Default().pAttachments(blends))
                .pDynamicState(VkPipelineDynamicStateCreateInfo.calloc(stack).sType$Default()
                    .pDynamicStates(stack.ints(VK10.VK_DYNAMIC_STATE_VIEWPORT, VK10.VK_DYNAMIC_STATE_SCISSOR)))
                .layout(layout);
            LongBuffer pPipeline = stack.mallocLong(1);
            RawVulkanException.check(VK10.vkCreateGraphicsPipelines(device, VK10.VK_NULL_HANDLE, info, null, pPipeline), "vkCreateGraphicsPipelines");
            return pPipeline.get(0);
        }
    }

    private static void blend(VkPipelineColorBlendAttachmentState state, ColorStates.Slot slot) {
        state.colorWriteMask(slot.write() ? VK10.VK_COLOR_COMPONENT_R_BIT | VK10.VK_COLOR_COMPONENT_G_BIT | VK10.VK_COLOR_COMPONENT_B_BIT
            | VK10.VK_COLOR_COMPONENT_A_BIT : 0);
        Optional<BlendMode> mode = slot.blend();
        if (mode.isPresent()) {
            state.blendEnable(true)
                .srcColorBlendFactor(ColorStates.vkFactor(mode.get().srcColor()))
                .dstColorBlendFactor(ColorStates.vkFactor(mode.get().dstColor()))
                .colorBlendOp(VK10.VK_BLEND_OP_ADD)
                .srcAlphaBlendFactor(ColorStates.vkFactor(mode.get().srcAlpha()))
                .dstAlphaBlendFactor(ColorStates.vkFactor(mode.get().dstAlpha()))
                .alphaBlendOp(VK10.VK_BLEND_OP_ADD);
        }
    }
}
