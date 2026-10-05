package dev.shaderbridge;

import dev.shaderbridge.config.ConfigStore;
import dev.shaderbridge.gui.ShaderBridgeKeys;
import dev.shaderbridge.gui.VideoSettingsButton;
import dev.shaderbridge.natives.NativeLibrary;
import dev.shaderbridge.render.frame.RenderBridge;
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
 * is joined. Rendering follows the active pack by itself ({@link RenderBridge}: a new, recompiled
 * or unloaded pack, a dimension change and a window resize are picked up at the next frame); the
 * hooks here release its GPU resources when the world is left or the client stops, and rebuild
 * them after a resource reload.
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
        // Fabric fires DISCONNECT on a network thread when the connection drops; the render
        // resources are released on the render thread.
        ClientPlayConnectionEvents.DISCONNECT.register((listener, minecraft) -> minecraft.execute(RenderBridge::release));
        ClientLifecycleEvents.CLIENT_STOPPING.register(minecraft -> {
            RenderBridge.release();
            bridge.shutdown();
        });
        ResourceLoader.get(PackType.CLIENT_RESOURCES).registerReloadListener(
            Identifier.fromNamespaceAndPath("shaderbridge", "reload_counter"),
            (ResourceManagerReloadListener) resourceManager -> {
                bridge.gameState().onResourceReload();
                RenderBridge.onResourceReload();
            });
    }
}
