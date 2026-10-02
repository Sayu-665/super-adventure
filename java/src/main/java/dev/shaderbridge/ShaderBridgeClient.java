package dev.shaderbridge;

import dev.shaderbridge.config.ConfigStore;
import dev.shaderbridge.gui.ShaderBridgeKeys;
import dev.shaderbridge.gui.VideoSettingsButton;
import dev.shaderbridge.natives.NativeLibrary;
import java.nio.file.Path;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientLifecycleEvents;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
import net.fabricmc.fabric.api.resource.v1.ResourceLoader;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.resources.Identifier;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.resources.ResourceManagerReloadListener;

/**
 * Client entrypoint: loads the configuration and the native library, registers key mappings,
 * the Video Settings button and the lifecycle hooks, and compiles the selected pack when a world
 * is joined.
 */
public final class ShaderBridgeClient implements ClientModInitializer {
    @Override
    public void onInitializeClient() {
        FabricLoader loader = FabricLoader.getInstance();
        Path gameDir = loader.getGameDir();
        ConfigStore config = ConfigStore.load(loader.getConfigDir().resolve("shaderbridge.json"));
        NativeLibrary.load(gameDir);
        ShaderBridge bridge = ShaderBridge.initialize(gameDir, config);

        ShaderBridgeKeys keys = ShaderBridgeKeys.register();
        VideoSettingsButton.register();
        ClientTickEvents.END_CLIENT_TICK.register(minecraft -> {
            keys.handle(minecraft);
            bridge.gameState().tick(minecraft);
        });
        ClientPlayConnectionEvents.JOIN.register((listener, sender, minecraft) -> bridge.onWorldJoin());
        ClientLifecycleEvents.CLIENT_STOPPING.register(minecraft -> bridge.shutdown());
        ResourceLoader.get(PackType.CLIENT_RESOURCES).registerReloadListener(
            Identifier.fromNamespaceAndPath("shaderbridge", "reload_counter"),
            (ResourceManagerReloadListener) resourceManager -> bridge.gameState().onResourceReload());
    }
}
