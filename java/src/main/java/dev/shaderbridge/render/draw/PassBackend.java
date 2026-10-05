package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.backend.api.RenderPassBackend;

/**
 * Implemented by Mojang's {@code FrontendRenderPass} (through ShaderBridge's mixin): the backend
 * pass behind a frontend pass, which ShaderBridge needs to set state renderpearl does not expose
 * (a viewport smaller than the attachments). Check with {@code instanceof}: when the mixin did not
 * apply, passes do not implement it.
 */
public interface PassBackend {
    /** @return the backend render pass ({@code VulkanRenderPass} on the Vulkan backend) */
    RenderPassBackend shaderbridge$backend();
}
