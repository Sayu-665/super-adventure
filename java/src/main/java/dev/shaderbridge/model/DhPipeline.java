package dev.shaderbridge.model;

/**
 * Distant Horizons configuration of a dimension.
 *
 * @param strategy          native, synthesized or disabled
 * @param unifiedProjection render vanilla terrain and LODs with one projection that ends at the DH far plane
 * @param shadowEnabled     LODs are drawn into the shadow map
 */
public record DhPipeline(DhStrategy strategy, boolean unifiedProjection, boolean shadowEnabled) {
    public DhPipeline {
        Copies.required(strategy, "strategy");
    }
}
