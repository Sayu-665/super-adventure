package dev.shaderbridge.render.shadow;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.Map;
import net.minecraft.client.renderer.DynamicGpuData;
import org.joml.Matrix4f;
import org.joml.Matrix4fc;
import org.joml.Vector3f;
import org.joml.Vector4f;

/**
 * The {@code DynamicTransforms} blocks of the frame's prepared features, re-written for the
 * shadow camera. Minecraft prepares each feature draw ({@code PreparedRenderType}) with a
 * {@code DynamicTransforms} block written at preparation time, whose {@code ModelViewMat} is the
 * camera's view rotation (feature vertices are camera-relative world positions). Drawing the same
 * prepared features into the shadow map needs the shadow camera's model-view there instead, as the
 * headless executor's shadow draws have ({@code DrawState::for_shadow}): every block written while
 * recording is copied with {@code ModelViewMat = shadowModelView * inverse(view rotation) *
 * ModelViewMat} (which keeps Minecraft's layering offsets), and the shadow pass binds the copy in
 * place of the original. Render thread only.
 */
public final class ShadowTransforms {
    private static final Map<GpuBufferSlice, DynamicGpuData.Transform> RECORDED = new LinkedHashMap<>();
    /** Recording stops once this many blocks were seen (a safety net; a frame writes a few thousand at most). */
    static final int MAX_RECORDED = 65536;
    private static boolean recording;

    private ShadowTransforms() {
    }

    /** Starts recording the frame's transform blocks (start of a pack frame whose shadow pass draws features). */
    public static void startRecording() {
        RECORDED.clear();
        recording = true;
    }

    /** Stops recording and forgets what was recorded. */
    public static void stop() {
        recording = false;
        RECORDED.clear();
    }

    /** @return how many blocks are recorded (tests) */
    static int recordedCount() {
        return RECORDED.size();
    }

    /**
     * @param slice a recorded block
     * @return the values recorded for it, or null (tests)
     */
    static DynamicGpuData.Transform recorded(GpuBufferSlice slice) {
        return RECORDED.get(slice);
    }

    /** @return whether blocks are being recorded */
    public static boolean recording() {
        return recording;
    }

    /**
     * Records a transform block Minecraft wrote (from {@code DynamicGpuData.writeTransform}).
     *
     * @param transform the values written
     * @param slice     where they were written
     */
    public static void record(DynamicGpuData.Transform transform, GpuBufferSlice slice) {
        if (!recording || transform == null || slice == null) {
            return;
        }
        if (RECORDED.size() >= MAX_RECORDED) {
            recording = false;
            return;
        }
        RECORDED.put(slice, new DynamicGpuData.Transform(new Matrix4f(transform.modelView()), new Vector4f(transform.colorModulator()),
            new Vector3f(transform.modelOffset()), new Matrix4f(transform.textureMatrix())));
    }

    /**
     * Stops recording and writes the shadow copy of every recorded block. Call outside any render
     * pass, after Minecraft prepared the frame's features.
     *
     * @param shadowModelView     the shadow camera's model-view
     * @param cameraViewInverse   the inverse of the camera's view rotation (the model-view the
     *                            features were prepared with, before layering)
     * @return original block to its shadow copy
     */
    public static Map<GpuBufferSlice, GpuBufferSlice> writeShadowCopies(Matrix4fc shadowModelView, Matrix4fc cameraViewInverse) {
        recording = false;
        Map<GpuBufferSlice, GpuBufferSlice> copies = new HashMap<>(RECORDED.size() * 2);
        Matrix4f modelView = new Matrix4f();
        for (Map.Entry<GpuBufferSlice, DynamicGpuData.Transform> e : RECORDED.entrySet()) {
            DynamicGpuData.Transform t = e.getValue();
            shadowModelView(shadowModelView, cameraViewInverse, t.modelView(), modelView);
            copies.put(e.getKey(), RenderSystem.getDynamicUniforms().writeTransform(new DynamicGpuData.Transform(new Matrix4f(modelView),
                t.colorModulator(), t.modelOffset(), t.textureMatrix())));
        }
        RECORDED.clear();
        return copies;
    }

    /**
     * The model-view of a feature draw in the shadow pass.
     *
     * @param shadowModelView   the shadow camera's model-view
     * @param cameraViewInverse the inverse of the camera's view rotation
     * @param modelView         the draw's camera model-view (view rotation, possibly with a layering offset)
     * @param dest              receives {@code shadowModelView * cameraViewInverse * modelView}
     * @return {@code dest}
     */
    public static Matrix4f shadowModelView(Matrix4fc shadowModelView, Matrix4fc cameraViewInverse, Matrix4fc modelView, Matrix4f dest) {
        return dest.set(shadowModelView).mul(cameraViewInverse).mul(modelView);
    }
}
