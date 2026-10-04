package dev.shaderbridge.render.frame;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.Pass;
import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.render.pipeline.AttachmentLayout;
import dev.shaderbridge.render.pipeline.ProgramResolution;
import dev.shaderbridge.render.pipeline.RawDispatch;
import java.util.Arrays;
import java.util.EnumSet;
import java.util.Set;
import net.minecraft.client.Minecraft;

/**
 * Dispatches the compute programs of a pass on the raw Vulkan path, between Minecraft's render
 * passes. Computes of {@code shadowcomp} and of shadow geometry passes cover the shadow map, all
 * others the screen. Programs still being prepared are skipped this frame; programs the raw path
 * cannot run were reported once by the program resolver and are skipped. Render thread only,
 * outside any render pass.
 */
final class ComputeDispatcher {
    /** Work group counts every Vulkan device supports in each dimension. */
    static final int[] MIN_MAX_WORK_GROUPS = {65535, 65535, 65535};

    private static final Set<GeometryProgram> SHADOW_SLOTS = EnumSet.of(GeometryProgram.SHADOW, GeometryProgram.SHADOW_SOLID,
        GeometryProgram.SHADOW_CUTOUT, GeometryProgram.SHADOW_WATER, GeometryProgram.SHADOW_ENTITIES, GeometryProgram.SHADOW_LIGHTNING,
        GeometryProgram.SHADOW_BLOCK, GeometryProgram.DH_SHADOW);

    private final PackResources r;

    ComputeDispatcher(PackResources r) {
        this.r = r;
    }

    /**
     * @param pass  the pass whose computes run
     * @param flips the frame's flip state
     * @param frame the frame's {@code sb_Frame} slice
     */
    void dispatch(Pass pass, FlipState flips, GpuBufferSlice frame) {
        for (int index : pass.computes()) {
            Program program = r.dim.programs().get(index);
            if (!(r.programs.program(index, AttachmentLayout.fullscreen(r.dim, program)) instanceof ProgramResolution.Raw raw)) {
                continue;
            }
            int[] extent = coversShadowMap(program) ? shadowExtent() : screenExtent();
            int[] groups = DispatchSize.of(program.compute(), extent[0], extent[1], MIN_MAX_WORK_GROUPS);
            if (Arrays.stream(groups).anyMatch(g -> g == 0)) {
                continue;
            }
            try {
                r.raw.dispatch(raw.program(), new RawDispatch(Arrays.stream(groups).boxed().toList(), frame, flips.colorState(), flips.shadowState()));
            } catch (RuntimeException e) {
                r.diagnostics.report(program.name() + " was not dispatched: " + e.getMessage());
            }
        }
    }

    /**
     * @param program a compute program
     * @return whether it covers the shadow map rather than the screen
     */
    static boolean coversShadowMap(Program program) {
        return switch (program.kind()) {
            case ProgramKind.Compute c -> c.group() == PassGroup.SHADOW_COMP;
            case ProgramKind.GeometryCompute g -> SHADOW_SLOTS.contains(g.program());
            default -> false;
        };
    }

    private int[] shadowExtent() {
        GpuTextureView depth = r.targets.shadowDepthView(0);
        return new int[] {depth.getWidth(0), depth.getHeight(0)};
    }

    private static int[] screenExtent() {
        RenderTarget main = Minecraft.getInstance().gameRenderer.mainRenderTarget();
        return new int[] {main.width, main.height};
    }
}
