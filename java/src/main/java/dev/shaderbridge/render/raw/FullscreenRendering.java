package dev.shaderbridge.render.raw;

import dev.shaderbridge.render.pipeline.ViewportRect;
import java.util.List;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.vulkan.KHRDynamicRendering;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VkCommandBuffer;
import org.lwjgl.vulkan.VkRect2D;
import org.lwjgl.vulkan.VkRenderingAttachmentInfo;
import org.lwjgl.vulkan.VkRenderingInfo;
import org.lwjgl.vulkan.VkViewport;

/**
 * Records a fullscreen draw in a render pass of its own, with dynamic rendering as Minecraft's
 * passes ({@code VK_KHR_dynamic_rendering}): each color attachment loaded and stored in
 * {@code GENERAL} (slots without a view discard their writes), the program's viewport (positive
 * height, the host convention; {@code scale.<program>} makes it smaller than the extent) and a
 * scissor over the whole extent, and the {@code fullscreen} profile's six vertices.
 */
final class FullscreenRendering {
    /** Vertices of the {@code fullscreen} profile's quad, two triangles generated from the vertex index. */
    static final int VERTICES = 6;

    private FullscreenRendering() {
    }

    /**
     * @param cb     the command buffer, with the pipeline and descriptor sets bound
     * @param views  the {@code VkImageView} of each color attachment, {@code VK_NULL_HANDLE} where there is none
     * @param width    width of the attachments
     * @param height   height of the attachments
     * @param viewport the draw's viewport
     */
    static void draw(VkCommandBuffer cb, List<Long> views, int width, int height, ViewportRect viewport) {
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkRenderingAttachmentInfo.Buffer attachments = VkRenderingAttachmentInfo.calloc(views.size(), stack);
            for (int i = 0; i < views.size(); i++) {
                VkRenderingAttachmentInfo a = attachments.get(i).sType$Default().imageView(views.get(i));
                if (views.get(i) != VK10.VK_NULL_HANDLE) {
                    a.imageLayout(VK10.VK_IMAGE_LAYOUT_GENERAL).loadOp(VK10.VK_ATTACHMENT_LOAD_OP_LOAD).storeOp(VK10.VK_ATTACHMENT_STORE_OP_STORE);
                } else {
                    a.imageLayout(VK10.VK_IMAGE_LAYOUT_UNDEFINED).loadOp(VK10.VK_ATTACHMENT_LOAD_OP_DONT_CARE)
                        .storeOp(VK10.VK_ATTACHMENT_STORE_OP_DONT_CARE);
                }
            }
            VkRect2D area = VkRect2D.calloc(stack);
            area.extent().set(width, height);
            VkRenderingInfo info = VkRenderingInfo.calloc(stack).sType$Default().renderArea(area).layerCount(1).pColorAttachments(attachments);
            KHRDynamicRendering.vkCmdBeginRenderingKHR(cb, info);
            VkViewport.Buffer vp = VkViewport.calloc(1, stack);
            vp.get(0).x(viewport.x()).y(viewport.y()).width(viewport.width()).height(viewport.height()).minDepth(0).maxDepth(1);
            VK10.vkCmdSetViewport(cb, 0, vp);
            VkRect2D.Buffer scissor = VkRect2D.calloc(1, stack);
            scissor.get(0).extent().set(width, height);
            VK10.vkCmdSetScissor(cb, 0, scissor);
            VK10.vkCmdDraw(cb, VERTICES, 1, 0, 0);
            KHRDynamicRendering.vkCmdEndRenderingKHR(cb);
        }
    }
}
