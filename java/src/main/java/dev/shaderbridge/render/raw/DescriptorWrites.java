package dev.shaderbridge.render.raw;

import java.util.List;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VkDescriptorBufferInfo;
import org.lwjgl.vulkan.VkDescriptorImageInfo;
import org.lwjgl.vulkan.VkDevice;
import org.lwjgl.vulkan.VkWriteDescriptorSet;

/**
 * Writes resolved bindings into freshly allocated descriptor sets. Every element of a descriptor
 * array gets the same resource, as in the headless executor; images are bound in {@code GENERAL},
 * the only layout Minecraft's and the raw path's images are ever in.
 */
final class DescriptorWrites {
    private DescriptorWrites() {
    }

    /**
     * One binding to write.
     *
     * @param set     the descriptor set
     * @param binding the plan's binding
     * @param bound   what to bind
     */
    record Write(long set, DescriptorPlan.Binding binding, RawBindings.Bound bound) {
    }

    /**
     * @param device the device
     * @param writes the writes
     */
    static void apply(VkDevice device, List<Write> writes) {
        if (writes.isEmpty()) {
            return;
        }
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkWriteDescriptorSet.Buffer infos = VkWriteDescriptorSet.calloc(writes.size(), stack);
            for (int i = 0; i < writes.size(); i++) {
                Write w = writes.get(i);
                int count = w.binding().count();
                VkWriteDescriptorSet info = infos.get(i).sType$Default()
                    .dstSet(w.set())
                    .dstBinding(w.binding().binding())
                    .descriptorType(w.binding().type())
                    .descriptorCount(count);
                switch (w.bound()) {
                    case RawBindings.Bound.Buffer b -> {
                        VkDescriptorBufferInfo.Buffer buffers = VkDescriptorBufferInfo.calloc(count, stack);
                        for (int k = 0; k < count; k++) {
                            buffers.get(k).buffer(b.buffer()).offset(b.offset()).range(b.range());
                        }
                        info.pBufferInfo(buffers);
                    }
                    case RawBindings.Bound.Image img -> {
                        VkDescriptorImageInfo.Buffer images = VkDescriptorImageInfo.calloc(count, stack);
                        for (int k = 0; k < count; k++) {
                            images.get(k).set(img.sampler(), img.view(), VK10.VK_IMAGE_LAYOUT_GENERAL);
                        }
                        info.pImageInfo(images);
                    }
                }
            }
            VK10.vkUpdateDescriptorSets(device, infos, null);
        }
    }
}
