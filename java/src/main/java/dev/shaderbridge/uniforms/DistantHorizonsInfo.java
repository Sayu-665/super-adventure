package dev.shaderbridge.uniforms;

import com.seibel.distanthorizons.api.DhApi;

/**
 * Reads Distant Horizons state through its API. Only touch this class when the
 * {@code distanthorizons} mod is loaded; the API is a compile-only dependency.
 */
final class DistantHorizonsInfo {
    private DistantHorizonsInfo() {
    }

    /**
     * Fills the DH inputs of a frame while DH renders: render distance, near plane and the far
     * plane DH uses (the LOD distance plus a margin, times sqrt 2 so corners are not clipped).
     * Leaves the state untouched otherwise.
     */
    static void capture(FrameState state, float partialTick) {
        if (DhApi.Delayed.configs == null || !Boolean.TRUE.equals(DhApi.Delayed.configs.graphics().renderingEnabled().getValue())) {
            return;
        }
        int lodBlocks = DhApi.Delayed.configs.graphics().chunkRenderDistance().getValue() * 16;
        state.dhActive = true;
        state.dhRenderDistance = lodBlocks;
        state.dhFarPlane = (float) ((lodBlocks + 512) * Math.sqrt(2));
        state.dhNearPlane = DhApi.Delayed.renderProxy == null ? 0.01f : DhApi.Delayed.renderProxy.getNearClipPlaneDistanceInBlocks(partialTick);
    }
}
