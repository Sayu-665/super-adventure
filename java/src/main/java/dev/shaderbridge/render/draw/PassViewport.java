package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.backend.api.RenderPassBackend;
import com.mojang.renderpearl.backend.vulkan.VulkanRenderPass;
import dev.shaderbridge.render.pipeline.ViewportRect;
import java.lang.reflect.Field;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VkCommandBuffer;
import org.lwjgl.vulkan.VkViewport;

/**
 * Sets a viewport smaller than the attachments in a renderpearl render pass, which renderpearl's
 * API cannot: its Vulkan backend sets the viewport to the whole attachment when the pass begins
 * and keeps it dynamic, so a {@code vkCmdSetViewport} recorded into the pass's command buffer
 * holds for the draws that follow (binding a pipeline does not reset dynamic state). The command
 * buffer is the backend pass's private {@code commandBuffer} field, read by reflection: if it
 * moves in another Minecraft version, or on the OpenGL backend, the viewport is not set and the
 * caller reports it. Render thread only.
 */
public final class PassViewport {
    private static final Field COMMAND_BUFFER = commandBufferField();

    private PassViewport() {
    }

    private static Field commandBufferField() {
        try {
            Field field = VulkanRenderPass.class.getDeclaredField("commandBuffer");
            if (field.getType() != VkCommandBuffer.class) {
                return null;
            }
            field.setAccessible(true);
            return field;
        } catch (NoSuchFieldException | RuntimeException | LinkageError e) {
            return null;
        }
    }

    /** @return whether viewports can be set at all (the backend member exists) */
    public static boolean available() {
        return COMMAND_BUFFER != null;
    }

    /**
     * @param pass     an open render pass, before its draws
     * @param viewport the viewport
     * @return whether it was set (false on the OpenGL backend or without the pass mixin)
     */
    public static boolean set(RenderPass pass, ViewportRect viewport) {
        if (COMMAND_BUFFER == null || !(pass instanceof PassBackend backend)) {
            return false;
        }
        RenderPassBackend target = backend.shaderbridge$backend();
        if (!(target instanceof VulkanRenderPass vulkan)) {
            return false;
        }
        VkCommandBuffer commands;
        try {
            commands = (VkCommandBuffer) COMMAND_BUFFER.get(vulkan);
        } catch (IllegalAccessException e) {
            return false;
        }
        try (MemoryStack stack = MemoryStack.stackPush()) {
            VkViewport.Buffer vp = VkViewport.calloc(1, stack);
            vp.get(0).x(viewport.x()).y(viewport.y()).width(viewport.width()).height(viewport.height()).minDepth(0).maxDepth(1);
            VK10.vkCmdSetViewport(commands, 0, vp);
        }
        return true;
    }
}
