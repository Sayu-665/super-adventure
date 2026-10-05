package dev.shaderbridge.render.raw;

import java.util.List;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VkCommandBuffer;
import org.lwjgl.vulkan.VkImageMemoryBarrier;
import org.lwjgl.vulkan.VkMemoryBarrier;

/**
 * Records a {@link CommandPlan} into a command buffer. Barriers use the stage and access masks of
 * Minecraft's own global barrier ({@code VulkanCommandEncoder.memoryBarrier}: all commands, memory
 * read and write); first uses of the raw path's images add their {@code UNDEFINED -> GENERAL}
 * transition to the leading barrier.
 */
final class CommandRecorder {
    /** {@code VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT}. */
    private static final int ALL_MEMORY = VK10.VK_ACCESS_MEMORY_READ_BIT | VK10.VK_ACCESS_MEMORY_WRITE_BIT;

    private CommandRecorder() {
    }

    /** What the plan's resource steps and its run step do. */
    interface Steps {
        /**
         * @param cb       the command buffer
         * @param resource a resource to clear or load
         * @param initial  its first use
         */
        void fill(VkCommandBuffer cb, Object resource, boolean initial);

        /** @param cb the command buffer; records the dispatch or draw */
        void run(VkCommandBuffer cb);
    }

    /**
     * @param cb    the command buffer, recording
     * @param plan  the plan
     * @param steps records the fills and the run
     */
    static void record(VkCommandBuffer cb, List<CommandPlan.Step<Object>> plan, Steps steps) {
        for (CommandPlan.Step<Object> step : plan) {
            switch (step) {
                case CommandPlan.Step.Barrier<Object> barrier -> barrier(cb, barrier.transitions());
                case CommandPlan.Step.Fill<Object> fill -> steps.fill(cb, fill.resource(), fill.initial());
                case CommandPlan.Step.Run<Object> run -> steps.run(cb);
            }
        }
    }

    private static void barrier(VkCommandBuffer cb, List<Object> transitions) {
        List<VmaImage> images = transitions.stream().filter(VmaImage.class::isInstance).map(VmaImage.class::cast).toList();
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkMemoryBarrier.Buffer memory = VkMemoryBarrier.calloc(1, stack).sType$Default().srcAccessMask(ALL_MEMORY).dstAccessMask(ALL_MEMORY);
            VkImageMemoryBarrier.Buffer layouts = images.isEmpty() ? null : VkImageMemoryBarrier.calloc(images.size(), stack);
            for (int i = 0; i < images.size(); i++) {
                VkImageMemoryBarrier b = layouts.get(i).sType$Default()
                    .srcAccessMask(0)
                    .dstAccessMask(ALL_MEMORY)
                    .oldLayout(VK10.VK_IMAGE_LAYOUT_UNDEFINED)
                    .newLayout(VK10.VK_IMAGE_LAYOUT_GENERAL)
                    .srcQueueFamilyIndex(VK10.VK_QUEUE_FAMILY_IGNORED)
                    .dstQueueFamilyIndex(VK10.VK_QUEUE_FAMILY_IGNORED)
                    .image(images.get(i).image());
                b.subresourceRange().set(VK10.VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1);
            }
            VK10.vkCmdPipelineBarrier(cb, VK10.VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK10.VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, memory, null, layouts);
        }
    }
}
