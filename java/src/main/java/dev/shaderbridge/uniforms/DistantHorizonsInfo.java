package dev.shaderbridge.uniforms;

import dev.shaderbridge.dh.CameraFarPlane;
import dev.shaderbridge.dh.DhPlanes;
import dev.shaderbridge.dh.DhSettings;
import dev.shaderbridge.dh.DistantHorizons;
import java.util.Optional;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.ClientLevel;

/**
 * Reads the Distant Horizons inputs of a frame through ShaderBridge's Distant Horizons
 * integration ({@link DistantHorizons}): render distance, near and far planes (see
 * {@link DhPlanes}). With the unified projection of synthesized LODs, Distant Horizons only counts
 * as rendering once Minecraft's projection really reaches the Distant Horizons far plane
 * ({@link CameraFarPlane}), and its near plane is Minecraft's.
 */
final class DistantHorizonsInfo {
    private DistantHorizonsInfo() {
    }

    /**
     * Fills the DH inputs of a frame while Distant Horizons renders; leaves the state untouched
     * otherwise.
     *
     * @param state the frame state ({@code viewWidth}/{@code viewHeight} already captured)
     * @param mc    the client
     */
    static void capture(FrameState state, Minecraft mc) {
        Optional<DhSettings> dh = DistantHorizons.get().settings();
        if (dh.isEmpty()) {
            return;
        }
        DhSettings s = dh.get();
        float far = DhPlanes.farPlane(s.lodChunks());
        boolean unified = state.settings().unifiedProjection();
        if (unified && !CameraFarPlane.extendedTo(far)) {
            return;
        }
        state.dhActive = true;
        state.dhRenderDistance = s.lodChunks() * 16;
        state.dhFarPlane = far;
        state.dhNearPlane = unified ? DhPlanes.MINECRAFT_NEAR : DhPlanes.nearPlane(mc.options.getEffectiveRenderDistance(),
            (double) state.viewWidth / Math.max(1, state.viewHeight), s.overdraw(), s.lodOnly(), heightOverride(mc));
    }

    private static float heightOverride(Minecraft mc) {
        ClientLevel level = mc.level;
        return level == null || mc.player == null ? -1 : DhPlanes.heightOverride(mc.player.getBlockY(), level.getHeight());
    }
}
