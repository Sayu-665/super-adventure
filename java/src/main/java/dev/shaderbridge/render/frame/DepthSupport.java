package dev.shaderbridge.render.frame;

import dev.shaderbridge.model.DepthMode;
import java.util.Optional;

/**
 * Whether ShaderBridge can render a pack in game with the depth convention it was compiled for.
 * Pack geometry shares Minecraft's main depth buffer, which Minecraft 26.3 always clears to 0 and
 * tests with {@code GEQUAL} (reversed-Z), and the depth textures packs sample are copies of it. Only
 * {@link DepthMode#REVERSED_ZERO_TO_ONE} on a device that clips depth to [0, 1] matches that: in
 * the forward modes every pack fragment would fail the depth test against the vanilla clear, and
 * on a device with [-1, 1] clip depth the reversed translation leaves half the depth range unused
 * and depth reads wrong.
 */
final class DepthSupport {
    private DepthSupport() {
    }

    /**
     * @param mode      the depth mode the pack was compiled for
     * @param zeroToOne the device clips depth to [0, 1] ({@code DeviceInfo.isZZeroToOne()})
     * @param backend   the device's backend name, for the message
     * @return why the pack cannot be rendered, or empty if it can
     */
    static Optional<String> problem(DepthMode mode, boolean zeroToOne, String backend) {
        if (!zeroToOne) {
            return Optional.of("the " + backend + " device has no [0, 1] depth clip control (GL_ARB_clip_control), and ShaderBridge renders "
                + "shader packs only into Minecraft's reversed-Z [0, 1] depth buffer");
        }
        if (mode != DepthMode.REVERSED_ZERO_TO_ONE) {
            return Optional.of("the pack was compiled for " + mode + " depth, but packs share Minecraft's reversed-Z depth buffer in game; "
                + "set depthMode to auto or reversed in shaderbridge.json and reload the shader pack");
        }
        return Optional.empty();
    }
}
