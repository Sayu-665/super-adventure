package dev.shaderbridge.render.frame;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.DepthMode;
import java.util.Optional;
import org.junit.jupiter.api.Test;

/** {@link DepthSupport}: which depth conventions render in game. */
class DepthSupportTest {
    @Test
    void onlyReversedZOnZeroToOneDevicesRendersInGame() {
        assertEquals(Optional.empty(), DepthSupport.problem(DepthMode.REVERSED_ZERO_TO_ONE, true, "Vulkan"));
        assertTrue(DepthSupport.problem(DepthMode.FORWARD_ZERO_TO_ONE, true, "Vulkan").orElseThrow().contains("depthMode"));
        assertTrue(DepthSupport.problem(DepthMode.GL_NEG_ONE_TO_ONE, false, "OpenGL").orElseThrow().contains("OpenGL"));
        assertTrue(DepthSupport.problem(DepthMode.REVERSED_ZERO_TO_ONE, false, "OpenGL").isPresent(),
            "reversed shaders on a [-1, 1] device would use half the depth range");
    }
}
