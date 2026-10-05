package dev.shaderbridge.dh;

import java.util.List;

/**
 * The LOD buffers Distant Horizons handed to its terrain renderer in one frame, copied out of its
 * render list (which it reuses every frame).
 *
 * @param frame  the {@link DistantHorizons} frame they were captured in
 * @param opaque buffers of opaque LODs ({@code dh_terrain}, {@code dh_shadow})
 * @param water  buffers of translucent LODs ({@code dh_water})
 */
public record LodFrame(long frame, List<LodBuffer> opaque, List<LodBuffer> water) {
    /** No LODs. */
    public static final LodFrame EMPTY = new LodFrame(-1, List.of(), List.of());

    public LodFrame {
        opaque = List.copyOf(opaque);
        water = List.copyOf(water);
    }
}
