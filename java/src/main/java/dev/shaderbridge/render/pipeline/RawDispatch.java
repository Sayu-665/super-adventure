package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import java.util.List;

/**
 * One compute dispatch requested from the {@link RawPath}.
 *
 * @param workGroups     work group counts {@code [x, y, z]} (Iris' {@code workGroups} /
 *                       {@code workGroupsRender} rules), each at least 1
 * @param frameUniforms  the frame's {@code sb_Frame} block
 * @param colorAlt       per colortex index: its current contents are in the alternate texture
 *                       (geometry-pass computes read and write the current textures; composite-pass
 *                       computes read per {@code BindingUse.use_alt})
 * @param shadowColorAlt per shadowcolor index: its current contents are in the alternate texture
 */
public record RawDispatch(List<Integer> workGroups, GpuBufferSlice frameUniforms, List<Boolean> colorAlt, List<Boolean> shadowColorAlt) {
    public RawDispatch {
        workGroups = List.copyOf(workGroups);
        colorAlt = List.copyOf(colorAlt);
        shadowColorAlt = List.copyOf(shadowColorAlt);
    }
}
