package dev.shaderbridge.gui;

import net.fabricmc.fabric.api.client.screen.v1.ScreenEvents;
import net.fabricmc.fabric.api.client.screen.v1.Screens;
import net.minecraft.client.gui.components.AbstractWidget;
import net.minecraft.client.gui.components.Button;
import net.minecraft.client.gui.components.OptionsList;
import net.minecraft.client.gui.screens.options.VideoSettingsScreen;
import net.minecraft.network.chat.Component;

/** Adds a "Shader Packs..." row to the end of the Video Settings list. */
public final class VideoSettingsButton {
    private VideoSettingsButton() {
    }

    /** Registers the screen hook. */
    public static void register() {
        ScreenEvents.AFTER_INIT.register((minecraft, screen, width, height) -> {
            if (!(screen instanceof VideoSettingsScreen video)) {
                return;
            }
            for (AbstractWidget widget : Screens.getWidgets(screen)) {
                if (widget instanceof OptionsList list) {
                    list.addBig(Button.builder(Component.translatable("shaderbridge.button.shader_packs"),
                        button -> minecraft.gui.setScreen(new ShaderPackScreen(video))).build());
                    return;
                }
            }
        });
    }
}
