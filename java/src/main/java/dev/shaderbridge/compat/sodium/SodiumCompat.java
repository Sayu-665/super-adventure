package dev.shaderbridge.compat.sodium;

import java.util.Optional;
import net.fabricmc.loader.api.FabricLoader;
import net.fabricmc.loader.api.ModContainer;

/**
 * What ShaderBridge does when Sodium is installed. Sodium replaces Minecraft's terrain renderer
 * (its own chunk meshes, programs, render lists and draw path), so the vanilla terrain hooks
 * ShaderBridge renders packs through never see terrain, and re-preparing Sodium's render lists for
 * the shadow camera would corrupt its per-frame state. Shading Sodium terrain needs the pack's
 * terrain programs compiled for the {@code sodium_terrain} profile drawn through Sodium's pipeline
 * hooks, and that profile reads attributes Sodium's 20-byte mesh lacks (block id, normal,
 * mid-texture coordinate, mid-block), so Sodium's mesh format and encoders would have to be
 * extended first. That is not implemented; instead packs are not rendered while Sodium is loaded,
 * and the player is told why (Minecraft and Sodium render as usual).
 */
public final class SodiumCompat {
    private static final String MOD_ID = "sodium";
    private static Optional<String> blocker;

    private SodiumCompat() {
    }

    /**
     * @return why shader packs cannot render in this game instance, if Sodium is installed
     */
    public static Optional<String> blocker() {
        if (blocker == null) {
            blocker = reason(FabricLoader.getInstance().getModContainer(MOD_ID).map(SodiumCompat::version));
        }
        return blocker;
    }

    private static String version(ModContainer mod) {
        return mod.getMetadata().getVersion().getFriendlyString();
    }

    /**
     * @param version the installed Sodium version, if any
     * @return the message for the player, or empty when packs can render
     */
    static Optional<String> reason(Optional<String> version) {
        return version.map(v -> "Sodium " + v + " is installed" + (v.startsWith("0.9.") ? "" : " (ShaderBridge was written against Sodium 0.9.x)")
            + ", and ShaderBridge cannot shade Sodium's terrain yet; remove Sodium to use shader packs");
    }
}
