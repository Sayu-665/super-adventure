package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import java.util.List;
import java.util.Optional;

/**
 * One fullscreen draw of a composite-style program requested from the {@link RawPath}: the
 * {@code fullscreen} profile's six vertices over the pass's color attachments.
 *
 * @param attachments    the color attachment of each output location (base mip level views of one
 *                       size); empty where the location has no texture, so its writes are discarded
 * @param width          width of the attachments
 * @param height         height of the attachments
 * @param frame          identifies the frame being rendered (changes from one frame to the next)
 * @param frameUniforms  the frame's {@code sb_Frame} block
 * @param drawUniforms   the {@code sb_Draw} block of the draw
 * @param colorAlt       per colortex index: its current contents are in the alternate texture
 * @param shadowColorAlt per shadowcolor index: its current contents are in the alternate texture
 */
public record RawDraw(List<Optional<GpuTextureView>> attachments, int width, int height, long frame, GpuBufferSlice frameUniforms,
                      GpuBufferSlice drawUniforms, List<Boolean> colorAlt, List<Boolean> shadowColorAlt) {
    public RawDraw {
        attachments = List.copyOf(attachments);
        colorAlt = List.copyOf(colorAlt);
        shadowColorAlt = List.copyOf(shadowColorAlt);
    }
}
