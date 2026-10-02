package dev.shaderbridge.config;

/**
 * Mod settings stored in {@code config/shaderbridge.json}. Immutable; use the {@code with*}
 * methods and {@link ConfigStore#update} to change them.
 *
 * @param enabled               shader packs are enabled
 * @param selectedPack          file name of the selected pack in {@code shaderpacks/}, or null for none
 * @param depthMode             depth convention of the translated shaders
 * @param validate              validate SPIR-V with spirv-val while compiling (slower)
 * @param debugDumpGlsl         write the translated GLSL to {@code shaderbridge/debug/} after each compile
 * @param compileThreads        native compile threads, 0 = automatic
 * @param showDiagnosticsInChat report compile results in chat (otherwise as a toast)
 */
public record ShaderBridgeConfig(
    boolean enabled,
    String selectedPack,
    DepthModeSetting depthMode,
    boolean validate,
    boolean debugDumpGlsl,
    int compileThreads,
    boolean showDiagnosticsInChat
) {
    /** The settings of a fresh installation. */
    public static final ShaderBridgeConfig DEFAULT = new ShaderBridgeConfig(true, null, DepthModeSetting.AUTO, false, false, 0, true);

    public ShaderBridgeConfig {
        if (depthMode == null) {
            depthMode = DepthModeSetting.AUTO;
        }
        if (selectedPack != null && selectedPack.isBlank()) {
            selectedPack = null;
        }
        compileThreads = Math.max(0, compileThreads);
    }

    /**
     * @param value new value of {@link #enabled()}
     * @return a copy with the change
     */
    public ShaderBridgeConfig withEnabled(boolean value) {
        return new ShaderBridgeConfig(value, selectedPack, depthMode, validate, debugDumpGlsl, compileThreads, showDiagnosticsInChat);
    }

    /**
     * @param value new value of {@link #selectedPack()}, or null for none
     * @return a copy with the change
     */
    public ShaderBridgeConfig withSelectedPack(String value) {
        return new ShaderBridgeConfig(enabled, value, depthMode, validate, debugDumpGlsl, compileThreads, showDiagnosticsInChat);
    }

    /** @return true if a pack is selected and shader packs are enabled */
    public boolean isActive() {
        return enabled && selectedPack != null;
    }
}
