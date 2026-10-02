package dev.shaderbridge.model;

import java.util.List;

/**
 * Identity of a compiled pack.
 *
 * @param name                 pack name
 * @param sourceHash           blake3 hash of the pack files, options and environment (cache key)
 * @param shaderbridgeVersion  version of the compiler that produced the model
 * @param featuresEnabled      supported feature flags declared by the pack
 * @param featuresUnsupported  required feature flags that are not supported (the pack should not be enabled)
 * @param environment          environment the pack was compiled for
 */
public record PackInfo(
    String name,
    String sourceHash,
    String shaderbridgeVersion,
    List<String> featuresEnabled,
    List<String> featuresUnsupported,
    CompileEnvironment environment
) {
    public PackInfo {
        Copies.required(name, "name");
        featuresEnabled = Copies.list(featuresEnabled);
        featuresUnsupported = Copies.list(featuresUnsupported);
    }
}
