package dev.shaderbridge.model;

import java.util.List;

/**
 * Shadow map settings.
 *
 * @param enabled                   the pack has a shadow pass
 * @param resolution                {@code shadowMapResolution}
 * @param fov                       {@code shadowMapFov}, or null for an orthographic projection
 * @param distance                  {@code shadowDistance} (half the orthographic extent)
 * @param nearPlane                 {@code shadowNearPlane} ({@code -1} = minus the DH render distance)
 * @param farPlane                  {@code shadowFarPlane} ({@code -1} = the DH render distance)
 * @param distanceRenderMul         {@code shadowDistanceRenderMul}
 * @param entityDistanceMul         {@code entityShadowDistanceMul}
 * @param intervalSize              {@code shadowIntervalSize} (grid snapping of the shadow camera)
 * @param voxelDistance             {@code voxelDistance}
 * @param hardwareFiltering         {@code shadowHardwareFiltering0/1}
 * @param mipmap                    {@code shadowtex0/1Mipmap}
 * @param nearest                   {@code shadowtex0/1Nearest}
 * @param colorMipmap               {@code shadowcolorNMipmap}
 * @param colorNearest              {@code shadowcolorNNearest}
 * @param culling                   {@code shadow.culling}
 * @param renderTerrain             {@code shadowTerrain}
 * @param renderTranslucent         {@code shadowTranslucent}
 * @param renderEntities            {@code shadowEntities}
 * @param renderPlayer              {@code shadowPlayer}
 * @param renderBlockEntities       {@code shadowBlockEntities}
 * @param renderLightBlockEntities  {@code shadowLightBlockEntities}
 * @param dhShadowEnabled           {@code dhShadow.enabled}
 */
public record ShadowSettings(
    boolean enabled,
    int resolution,
    Float fov,
    float distance,
    float nearPlane,
    float farPlane,
    float distanceRenderMul,
    float entityDistanceMul,
    float intervalSize,
    float voxelDistance,
    List<Boolean> hardwareFiltering,
    List<Boolean> mipmap,
    List<Boolean> nearest,
    List<Boolean> colorMipmap,
    List<Boolean> colorNearest,
    String culling,
    boolean renderTerrain,
    boolean renderTranslucent,
    boolean renderEntities,
    boolean renderPlayer,
    boolean renderBlockEntities,
    boolean renderLightBlockEntities,
    boolean dhShadowEnabled
) {
    public ShadowSettings {
        hardwareFiltering = Copies.list(hardwareFiltering);
        mipmap = Copies.list(mipmap);
        nearest = Copies.list(nearest);
        colorMipmap = Copies.list(colorMipmap);
        colorNearest = Copies.list(colorNearest);
    }
}
