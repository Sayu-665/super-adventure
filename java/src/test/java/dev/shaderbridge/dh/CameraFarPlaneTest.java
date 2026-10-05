package dev.shaderbridge.dh;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;

/** {@link CameraFarPlane}: the unified projection only counts once the camera hook applied it. */
class CameraFarPlaneTest {
    @AfterEach
    void reset() {
        CameraFarPlane.request(Float.NaN);
        CameraFarPlane.apply(0);
    }

    @Test
    void minecraftKeepsItsFarPlaneUnlessOneIsRequested() {
        CameraFarPlane.request(Float.NaN);
        assertEquals(768f, CameraFarPlane.apply(768f));
        assertFalse(CameraFarPlane.extendedTo(DhPlanes.farPlane(256)));
    }

    @Test
    void aRequestedFarPlaneCountsFromTheNextCameraSetup() {
        float far = DhPlanes.farPlane(256);
        CameraFarPlane.request(far);
        assertFalse(CameraFarPlane.extendedTo(far), "the camera of this frame was set up before the request");
        assertEquals(far, CameraFarPlane.apply(768f));
        assertTrue(CameraFarPlane.extendedTo(far));
        float nearer = DhPlanes.farPlane(64);
        CameraFarPlane.request(nearer);
        assertFalse(CameraFarPlane.extendedTo(nearer), "a changed DH distance waits for the next camera setup");
        CameraFarPlane.apply(768f);
        assertTrue(CameraFarPlane.extendedTo(nearer));
    }
}
