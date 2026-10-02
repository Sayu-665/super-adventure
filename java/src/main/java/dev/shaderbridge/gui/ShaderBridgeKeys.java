package dev.shaderbridge.gui;

import com.mojang.blaze3d.platform.InputConstants;
import dev.shaderbridge.ShaderBridge;
import net.fabricmc.fabric.api.client.keymapping.v1.KeyMappingHelper;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.minecraft.resources.Identifier;

/** The mod's key mappings: open the pack screen (O), reload (R) and toggle shaders (K). */
public final class ShaderBridgeKeys {
    private final KeyMapping openPacks;
    private final KeyMapping reload;
    private final KeyMapping toggle;

    private ShaderBridgeKeys(KeyMapping openPacks, KeyMapping reload, KeyMapping toggle) {
        this.openPacks = openPacks;
        this.reload = reload;
        this.toggle = toggle;
    }

    /** @return the registered key mappings */
    public static ShaderBridgeKeys register() {
        KeyMapping.Category category = KeyMapping.Category.register(Identifier.fromNamespaceAndPath("shaderbridge", "main"));
        return new ShaderBridgeKeys(
            KeyMappingHelper.registerKeyMapping(new KeyMapping("key.shaderbridge.open_packs", InputConstants.KEY_O, category)),
            KeyMappingHelper.registerKeyMapping(new KeyMapping("key.shaderbridge.reload", InputConstants.KEY_R, category)),
            KeyMappingHelper.registerKeyMapping(new KeyMapping("key.shaderbridge.toggle", InputConstants.KEY_K, category)));
    }

    /**
     * Handles presses since the last tick; called at the end of every client tick.
     *
     * @param minecraft the client
     */
    public void handle(Minecraft minecraft) {
        ShaderBridge bridge = ShaderBridge.get();
        while (openPacks.consumeClick()) {
            if (minecraft.gui.screen() == null) {
                minecraft.gui.setScreen(new ShaderPackScreen(null));
            }
        }
        while (reload.consumeClick()) {
            bridge.reload();
        }
        while (toggle.consumeClick()) {
            boolean enabled = !bridge.config().get().enabled();
            bridge.setEnabled(enabled);
            PackNotifier.toggled(enabled);
        }
    }
}
