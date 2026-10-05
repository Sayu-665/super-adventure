package dev.shaderbridge.compat.sodium;

import java.util.List;
import java.util.Optional;
import net.fabricmc.loader.api.FabricLoader;
import net.fabricmc.loader.api.ModContainer;

/**
 * Whether shader packs can render with the installed Sodium. Sodium replaces Minecraft's terrain
 * renderer (its own chunk meshes, pipelines, render lists and draw path), so ShaderBridge shades
 * Sodium's terrain through its own integration ({@link SodiumTerrain}, the mixins of
 * {@code shaderbridge-sodium.mixins.json}, {@link SodiumPipelines}). When that integration cannot
 * be applied to the installed Sodium (another version moved what it hooks, see
 * {@link SodiumTargets}) or the extended terrain vertex does not fit, packs are not rendered and
 * the player is told why; Minecraft and Sodium then render as usual.
 */
public final class SodiumCompat {
    private static final String MOD_ID = "sodium";
    private static Optional<String> blocker;

    private SodiumCompat() {
    }

    /**
     * @return why shader packs cannot render in this game instance, if they cannot
     */
    public static synchronized Optional<String> blocker() {
        if (blocker == null) {
            Optional<String> version = installedVersion();
            SodiumIntegration.Status status = SodiumIntegration.status(version);
            List<String> layout = status instanceof SodiumIntegration.Status.Active ? SodiumTerrain.layoutProblems() : List.of();
            blocker = reason(status, layout);
        }
        if (blocker.isEmpty() && SodiumIntegration.active()) {
            // Sodium's terrain vertex could not follow the active pack (checked again every frame).
            return SodiumTerrain.extensionFailure().map(failure -> cannotShade(installedVersion().orElse("?"), failure));
        }
        return blocker;
    }

    /** @return the installed Sodium version, if Sodium is installed */
    public static Optional<String> installedVersion() {
        return FabricLoader.getInstance().getModContainer(MOD_ID).map(SodiumCompat::version);
    }

    private static String version(ModContainer mod) {
        return mod.getMetadata().getVersion().getFriendlyString();
    }

    /**
     * @param status         the integration's state
     * @param layoutProblems problems of the extended terrain vertex with this Sodium
     *                       ({@link SodiumTerrain#layoutProblems()})
     * @return the message for the player, or empty when packs can render
     */
    static Optional<String> reason(SodiumIntegration.Status status, List<String> layoutProblems) {
        return switch (status) {
            case SodiumIntegration.Status.Absent a -> Optional.empty();
            case SodiumIntegration.Status.Active a -> layoutProblems.isEmpty() ? Optional.empty()
                : Optional.of(cannotShade(a.version(), "its chunk vertex format is not the one ShaderBridge extends (" + String.join("; ", layoutProblems) + ")"));
            case SodiumIntegration.Status.Unavailable u -> Optional.of(cannotShade(u.version(), "ShaderBridge's Sodium integration does not fit this "
                + "version (" + String.join("; ", u.problems()) + ")"));
            case SodiumIntegration.Status.NotLoaded n -> Optional.of(cannotShade(n.version(), "ShaderBridge's Sodium integration was not loaded"));
        };
    }

    private static String cannotShade(String version, String why) {
        return "Sodium " + version + " is installed" + (version.startsWith("0.9.") ? "" : " (ShaderBridge supports Sodium 0.9.x)")
            + ", and ShaderBridge cannot shade its terrain: " + why + "; update ShaderBridge or Sodium, or remove Sodium to use shader packs";
    }
}
