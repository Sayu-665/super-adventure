package dev.shaderbridge.render.shadow;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import net.minecraft.client.renderer.DynamicGpuData;
import org.joml.Matrix4f;
import org.joml.Vector3f;
import org.joml.Vector4f;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;

/** {@link ShadowTransforms}: the shadow-camera model-view of prepared features, and what is recorded. */
class ShadowTransformsTest {
    private static final float EPSILON = 1e-5f;

    private static Matrix4f view() {
        return new Matrix4f().rotateX(0.4f).rotateY(-1.1f);
    }

    private static Matrix4f shadow() {
        return new Matrix4f().rotateX((float) Math.PI / 2).rotateZ(0.3f).translate(0.25f, -0.5f, 0.125f);
    }

    @AfterEach
    void reset() {
        ShadowTransforms.stop();
    }

    @Test
    void featuresPreparedWithTheViewRotationGetTheShadowModelView() {
        Matrix4f view = view();
        Matrix4f result = ShadowTransforms.shadowModelView(shadow(), new Matrix4f(view).invert(), view, new Matrix4f());
        assertTrue(result.equals(shadow(), EPSILON), result + " vs " + shadow());
    }

    @Test
    void layeringOffsetsOnTopOfTheViewAreKept() {
        // VIEW_OFFSET_Z_LAYERING-like: the view rotation times a small scale.
        Matrix4f layering = new Matrix4f().scale(0.99975f);
        Matrix4f modelView = view().mul(layering);
        Matrix4f result = ShadowTransforms.shadowModelView(shadow(), view().invert(), modelView, new Matrix4f());
        assertTrue(result.equals(shadow().mul(layering), EPSILON), result.toString());
    }

    @Test
    void onlyWhatIsWrittenWhileRecordingIsCopiedAtWriteTime() {
        GpuBufferSlice first = new GpuBufferSlice(null, 0, 160);
        GpuBufferSlice second = new GpuBufferSlice(null, 256, 160);
        Matrix4f modelView = view();
        DynamicGpuData.Transform transform = new DynamicGpuData.Transform(modelView, new Vector4f(1, 0.5f, 1, 1), new Vector3f(1, 2, 3), new Matrix4f());
        ShadowTransforms.record(transform, first);
        assertEquals(0, ShadowTransforms.recordedCount(), "nothing is recorded outside a pack frame");
        ShadowTransforms.startRecording();
        assertTrue(ShadowTransforms.recording());
        ShadowTransforms.record(transform, first);
        modelView.identity();
        DynamicGpuData.Transform copy = ShadowTransforms.recorded(first);
        assertNotNull(copy);
        assertTrue(copy.modelView().equals(view(), EPSILON), "the values are copied when written, not when the shadow pass runs");
        assertEquals(new Vector3f(1, 2, 3), copy.modelOffset());
        ShadowTransforms.stop();
        assertFalse(ShadowTransforms.recording());
        ShadowTransforms.record(transform, second);
        assertNull(ShadowTransforms.recorded(second));
        assertEquals(0, ShadowTransforms.recordedCount());
    }
}
