package dev.shaderbridge.render.pipeline;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.backend.vulkan.VulkanRenderPass;
import dev.shaderbridge.model.ViewportScale;
import dev.shaderbridge.render.draw.PassViewport;
import org.junit.jupiter.api.Test;
import org.lwjgl.vulkan.VkCommandBuffer;

/** {@link ViewportRect}: {@code scale.<program>} as a viewport, as the headless executor sets it; and the backend member it is set through. */
class ViewportRectTest {
    @Test
    void theDefaultScaleCoversTheTarget() {
        ViewportRect full = ViewportRect.of(new ViewportScale(1, 0, 0), 1920, 1080);
        assertEquals(new ViewportRect(0, 0, 1920, 1080), full);
        assertTrue(full.covers(1920, 1080));
        assertFalse(full.covers(960, 540));
    }

    @Test
    void scaleAndOffsetAreFractionsOfTheTarget() {
        ViewportRect half = ViewportRect.of(new ViewportScale(0.5f, 0.5f, 0.25f), 1920, 1080);
        assertEquals(new ViewportRect(960, 270, 960, 540), half);
        assertFalse(half.covers(1920, 1080));
        assertEquals(new ViewportRect(0, 0, 1, 1), ViewportRect.of(new ViewportScale(0.0001f, 0, 0), 100, 100), "at least one pixel");
    }

    @Test
    void unusableScalesDrawOverTheWholeTarget() {
        for (ViewportScale bad : new ViewportScale[] {new ViewportScale(0, 0, 0), new ViewportScale(-1, 0, 0), new ViewportScale(Float.NaN, 0, 0),
            new ViewportScale(0.5f, Float.POSITIVE_INFINITY, 0)}) {
            assertEquals(new ViewportRect(0, 0, 64, 32), ViewportRect.of(bad, 64, 32), bad.toString());
        }
        assertEquals(new ViewportRect(0, 0, 64, 32), ViewportRect.of(null, 64, 32));
    }

    @Test
    void renderpearlViewportsGoThroughTheVulkanPassCommandBuffer() throws NoSuchFieldException {
        assertEquals(VkCommandBuffer.class, VulkanRenderPass.class.getDeclaredField("commandBuffer").getType());
        assertTrue(PassViewport.available(), "the member PassViewport reads exists in Minecraft 26.3");
    }
}
