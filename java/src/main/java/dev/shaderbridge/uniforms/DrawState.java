package dev.shaderbridge.uniforms;

import java.util.Arrays;
import org.joml.Matrix3f;
import org.joml.Matrix4f;
import org.joml.Vector3f;
import org.joml.Vector4f;

/**
 * The per-draw builtin values ({@code sb_Draw}), set by the render integration before each draw.
 * {@link #reset(FrameState)} restores the defaults: no entity, no item, the camera matrices and
 * an identity texture matrix.
 */
public final class DrawState {
    /** entity.properties id of the drawn entity, -1 if none. */
    public int entityId = -1;
    /** block.properties id of the drawn block entity, -1 if none. */
    public int blockEntityId = -1;
    /** item.properties id of the drawn item, -1 if none. */
    public int currentRenderedItemId = -1;
    /** Alpha test reference. */
    public float alphaTestRef;
    /** Entity tint: rgb color, a = strength. */
    public final Vector4f entityColor = new Vector4f();
    /** Blend factors as GL enums (srcRGB, dstRGB, srcAlpha, dstAlpha), zero when blending is off. */
    public final int[] blendFunc = new int[4];
    /** Size of the bound atlas, zero if none. */
    public int atlasWidth;
    /** Height of the bound atlas. */
    public int atlasHeight;
    /** Size of the texture bound as {@code gtexture}. */
    public int gtextureWidth;
    /** Height of the texture bound as {@code gtexture}. */
    public int gtextureHeight;
    /** Identifier of the texture bound as {@code gtexture}. */
    public int gtextureId;
    /** {@code MC_RENDER_STAGE_*} of the draw. */
    public int renderStage;
    /** Atlas sprite bounds (u0, v0, u1, v1). */
    public final Vector4f spriteBounds = new Vector4f();
    /** Instance index when instancing. */
    public int instanceId;
    /** Model-view matrix of the draw. */
    public final Matrix4f modelViewMatrix = new Matrix4f();
    /** Projection matrix of the draw (GL convention). */
    public final Matrix4f projectionMatrix = new Matrix4f();
    /** Texture matrix of the draw. */
    public final Matrix4f textureMatrix = new Matrix4f();
    /** Color modulator. */
    public final Vector4f colorModulator = new Vector4f(1, 1, 1, 1);
    /** Model offset added to vertex positions (also reported as {@code chunkOffset}). */
    public final Vector3f modelOffset = new Vector3f();

    final Matrix4f modelViewMatrixInverse = new Matrix4f();
    final Matrix4f projectionMatrixInverse = new Matrix4f();
    final Matrix3f normalMatrix = new Matrix3f();

    /**
     * Restores the defaults for a draw without special state.
     *
     * @param frame the current frame (supplies the camera matrices)
     */
    public void reset(FrameState frame) {
        entityId = -1;
        blockEntityId = -1;
        currentRenderedItemId = -1;
        alphaTestRef = 0;
        entityColor.zero();
        Arrays.fill(blendFunc, 0);
        atlasWidth = 0;
        atlasHeight = 0;
        gtextureWidth = 0;
        gtextureHeight = 0;
        gtextureId = 0;
        renderStage = 0;
        spriteBounds.zero();
        instanceId = 0;
        modelViewMatrix.set(frame.gbufferModelView());
        projectionMatrix.set(frame.gbufferProjection());
        textureMatrix.identity();
        colorModulator.set(1, 1, 1, 1);
        modelOffset.zero();
    }

    /** Derives the inverse and normal matrices; called before the values are written. */
    void update() {
        modelViewMatrix.invert(modelViewMatrixInverse);
        projectionMatrix.invert(projectionMatrixInverse);
        modelViewMatrix.normal(normalMatrix);
    }
}
