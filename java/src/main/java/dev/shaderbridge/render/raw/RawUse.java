package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import dev.shaderbridge.model.Program;
import java.util.List;

/**
 * One use of a raw program (a dispatch): the frame state its descriptors are resolved against.
 *
 * @param program        the program
 * @param frame          the frame being rendered (per-frame clears happen once per value)
 * @param frameUniforms  the frame's {@code sb_Frame} block
 * @param drawUniforms   the {@code sb_Draw} block of this use
 * @param colorAlt       per colortex index: its current contents are in the alternate texture
 * @param shadowColorAlt per shadowcolor index: its current contents are in the alternate texture
 */
record RawUse(Program program, long frame, GpuBufferSlice frameUniforms, GpuBufferSlice drawUniforms, List<Boolean> colorAlt,
              List<Boolean> shadowColorAlt) {
}
