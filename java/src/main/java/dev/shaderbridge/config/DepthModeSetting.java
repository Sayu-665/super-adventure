package dev.shaderbridge.config;

import dev.shaderbridge.model.DepthMode;
import java.util.Locale;
import java.util.Optional;

/**
 * The user's depth convention choice ({@code depthMode} in the config file): which depth convention
 * packs are translated for. The in-game renderer shares Minecraft's reversed-Z depth buffer, so it
 * renders only packs translated for reversed-Z on a device with [0, 1] clip depth, and refuses
 * others with a message; the other conventions exist for comparing translations with ShaderBridge's
 * headless executor.
 */
public enum DepthModeSetting {
    /** Reversed-Z [0,1] when the device clips Z to [0,1] (the default), else GL's [-1,1] (not rendered in game). */
    AUTO,
    /** Forward-Z [0,1] (not rendered in game: Minecraft's depth buffer is reversed-Z). */
    FORWARD,
    /** Reversed-Z [0,1], as Minecraft 26.2+ and Distant Horizons 3.3+ render. */
    REVERSED;

    /**
     * Picks the compile-time depth mode.
     *
     * @param zeroToOne the device clips Z to [0,1] ({@code DeviceInfo.isZZeroToOne()})
     * @return the depth mode the translated shaders are generated for
     */
    public DepthMode resolve(boolean zeroToOne) {
        return switch (this) {
            case FORWARD -> DepthMode.FORWARD_ZERO_TO_ONE;
            case REVERSED -> DepthMode.REVERSED_ZERO_TO_ONE;
            case AUTO -> zeroToOne ? DepthMode.REVERSED_ZERO_TO_ONE : DepthMode.GL_NEG_ONE_TO_ONE;
        };
    }

    /** @return the config file spelling ({@code auto}, {@code forward}, {@code reversed}) */
    public String configName() {
        return name().toLowerCase(Locale.ROOT);
    }

    /**
     * @param name a config file spelling
     * @return the matching setting, if any
     */
    public static Optional<DepthModeSetting> fromConfigName(String name) {
        for (DepthModeSetting setting : values()) {
            if (setting.configName().equals(name)) {
                return Optional.of(setting);
            }
        }
        return Optional.empty();
    }
}
