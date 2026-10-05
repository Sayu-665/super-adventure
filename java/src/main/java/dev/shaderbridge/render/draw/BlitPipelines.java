package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.pipeline.ColorTargetState;
import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import java.util.ArrayList;
import java.util.EnumMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import net.minecraft.client.renderer.RenderPipelines;
import net.minecraft.resources.Identifier;

/**
 * Minecraft's screen blit ({@code pipeline/tracy_blit}: {@code core/screenquad} and
 * {@code core/blit_screen}, a fullscreen triangle sampling {@code InSampler}) cloned for any color
 * format, without blending or depth: a filtered copy into formats no vanilla pipeline targets
 * (ShaderBridge generates mipmaps with it). Clones are kept for the life of the process, so
 * Mojang's pipeline cache holds one compiled pipeline per format. Render thread only.
 */
public final class BlitPipelines {
    private static final Map<GpuFormat, RenderPipeline> CLONES = new EnumMap<>(GpuFormat.class);

    private BlitPipelines() {
    }

    /**
     * @param format a color format whose shaders write floats (not an integer format)
     * @return the blit into that format; draw 3 vertices with {@code InSampler} bound
     */
    public static RenderPipeline to(GpuFormat format) {
        return CLONES.computeIfAbsent(format, BlitPipelines::create);
    }

    static RenderPipeline create(GpuFormat format) {
        RenderPipeline blit = RenderPipelines.TRACY_BLIT;
        Identifier v = blit.getLocation();
        Identifier location = Identifier.fromNamespaceAndPath("shaderbridge",
            VanillaClones.PATH_PREFIX + v.getNamespace() + "/" + v.getPath() + "/" + format.name().toLowerCase(Locale.ROOT));
        List<ColorTargetState> states = new ArrayList<>(List.of(new ColorTargetState(Optional.empty(), format, ColorTargetState.WRITE_ALL)));
        return new ClonedPipeline(location, blit, states, null);
    }
}
